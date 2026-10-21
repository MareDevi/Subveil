//! Isolated native inference: bounded IPC and hard cancellation protect the GUI.
use crate::{Request, Response, Shared, Translation, emit};
use anyhow::{Context as _, ensure};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::Ordering,
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};
pub use subveil_core::inference::{Compute, Device};
use subveil_core::inference::{Input, Output};

/// Visible loading/readiness/error state, without transcript text.
#[derive(Clone, Debug)]
pub struct LocalSnapshot {
    /// Sanitized operation, device or error.
    pub status: String,
    /// Weights loaded successfully.
    pub ready: bool,
    /// Helper exited and model resources were released.
    pub finished: bool,
}

fn executable() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("SUBVEIL_INFERENCE_BIN") {
        let path = PathBuf::from(path);
        ensure!(
            path.is_absolute() && path.is_file(),
            "SUBVEIL_INFERENCE_BIN 必须是推理程序的绝对路径"
        );
        return Ok(path);
    }
    let current = std::env::current_exe()?;
    let parent = current.parent().context("无法确定程序目录")?;
    let parent = if parent
        .file_name()
        .is_some_and(|name| name == "examples" || name == "deps")
    {
        parent.parent().context("无法确定开发程序目录")?
    } else {
        parent
    };
    let path = parent.join(format!("subveil-inference{}", std::env::consts::EXE_SUFFIX));
    ensure!(
        path.is_file(),
        "找不到 subveil-inference；请先运行 mise run build，安装时将两个程序放在同一目录"
    );
    Ok(path)
}

struct Helper {
    child: Child,
    input: SyncSender<Vec<u8>>,
    output: Receiver<Result<Output, String>>,
}
impl Helper {
    fn spawn(arguments: &[&std::ffi::OsStr]) -> anyhow::Result<Self> {
        Self::spawn_from(&executable()?, arguments)
    }
    fn spawn_from(executable: &Path, arguments: &[&std::ffi::OsStr]) -> anyhow::Result<Self> {
        let mut child = Command::new(executable)
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("无法启动本地推理程序")?;
        let mut stdin = child.stdin.take().context("推理程序缺少输入通道")?;
        let (input, requests) = mpsc::sync_channel::<Vec<u8>>(1);
        let stdout = child.stdout.take().context("推理程序缺少输出通道")?;
        let (sender, output) = mpsc::sync_channel(1);
        let reader = thread::Builder::new()
            .name("subveil-inference-ipc".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let mut line = Vec::new();
                    let result = match reader.by_ref().take(65_537).read_until(b'\n', &mut line) {
                        Ok(0) => break,
                        Ok(len) if len <= 65_536 && line.last() == Some(&b'\n') => {
                            serde_json::from_slice(&line)
                                .map_err(|_| "本地推理返回无效协议".to_owned())
                        }
                        _ => Err("本地推理通道中断或响应过长".into()),
                    };
                    let failed = result.is_err();
                    if sender.send(result).is_err() || failed {
                        break;
                    }
                }
            });
        if let Err(error) = reader {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.into());
        }
        // A native process may stop reading stdin after readiness. Only this
        // writer can block; the owner still observes cancellation and deadlines.
        if let Err(error) = thread::Builder::new()
            .name("subveil-inference-write".into())
            .spawn(move || {
                while let Ok(bytes) = requests.recv() {
                    if stdin
                        .write_all(&bytes)
                        .and_then(|()| stdin.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            })
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.into());
        }
        Ok(Self {
            child,
            input,
            output,
        })
    }
    fn receive(&self, deadline: Instant, stopped: impl Fn() -> bool) -> anyhow::Result<Output> {
        loop {
            ensure!(!stopped(), "本地推理已取消");
            ensure!(
                Instant::now() < deadline,
                "本地推理超时；进程已终止，请重新开始识别"
            );
            match self.output.recv_timeout(Duration::from_millis(10)) {
                Ok(Ok(Output::Error(error))) | Ok(Err(error)) => anyhow::bail!(error),
                Ok(Ok(output)) => return Ok(output),
                Err(RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("本地推理进程退出；检查驱动、模型与内存后重试")
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }
}
impl Drop for Helper {
    fn drop(&mut self) {
        // Runs only on a background worker. A native driver hang cannot block GPUI.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Probe native GPU devices in a disposable process, without loading weights or using network.
pub fn devices() -> anyhow::Result<Vec<Device>> {
    let helper = Helper::spawn(&[std::ffi::OsStr::new("--devices")])?;
    match helper.receive(Instant::now() + Duration::from_secs(15), || false)? {
        Output::Devices(devices) => Ok(devices),
        _ => anyhow::bail!("GPU 探测协议无效"),
    }
}
impl Translation {
    /// Start verified local inference. The helper cannot silently retry on CPU or cloud.
    pub fn start_local(manifest: PathBuf, compute: Compute, session: u64) -> anyhow::Result<Self> {
        executable()?;
        let shared = Arc::new(Shared::new());
        let snapshot = Arc::new(Mutex::new(LocalSnapshot {
            status: "校验并加载本地翻译模型…".into(),
            ready: false,
            finished: false,
        }));
        let state = shared.clone();
        let status = snapshot.clone();
        thread::Builder::new()
            .name("subveil-local-translate".into())
            .spawn(move || {
                let result = run(&manifest, compute, session, &state, &status);
                state.closed.store(true, Ordering::Release);
                if let Ok(mut snapshot) = status.lock() {
                    snapshot.ready = false;
                    snapshot.finished = true;
                    snapshot.status = if state.stop.load(Ordering::Relaxed) {
                        "本地翻译已停止；模型已卸载".into()
                    } else {
                        format!(
                            "本地翻译失败：{}",
                            result.err().map(|e| e.to_string()).unwrap_or_default()
                        )
                    };
                }
                // Fail pending work explicitly when the model process exits.
                crate::fail_pending(&state);
            })?;
        Ok(Self {
            shared,
            local: Some(snapshot),
        })
    }
    /// Nonblocking local state; HTTP workers return None.
    pub fn local_snapshot(&self) -> Option<LocalSnapshot> {
        self.local
            .as_ref()?
            .try_lock()
            .ok()
            .map(|state| state.clone())
    }
}
fn validate(request: &Request) -> anyhow::Result<()> {
    ensure!(Instant::now() < request.deadline, "翻译请求已过期");
    ensure!(
        !request.segment.text.trim().is_empty() && request.segment.text.len() <= 16_384,
        "翻译文本为空或过长"
    );
    ensure!(
        matches!(request.target.as_str(), "en" | "ja" | "zh"),
        "本地翻译仅支持中 / 英 / 日"
    );
    Ok(())
}
fn run(
    manifest: &Path,
    compute: Compute,
    session: u64,
    shared: &Shared,
    status: &Mutex<LocalSnapshot>,
) -> anyhow::Result<()> {
    let device = match compute {
        Compute::Cpu => "cpu".into(),
        Compute::Gpu(index) => index.to_string(),
    };
    let helper = Helper::spawn(&[manifest.as_os_str(), std::ffi::OsStr::new(&device)])?;
    let stopped = || shared.stop.load(Ordering::Relaxed);
    let ready_label = match helper.receive(Instant::now() + Duration::from_secs(30), stopped)? {
        Output::Ready(label) => {
            if let Ok(mut snapshot) = status.lock() {
                snapshot.ready = true;
                snapshot.status = label.clone();
            }
            label
        }
        _ => anyhow::bail!("模型加载协议无效"),
    };
    let mut id = 0_u64;
    while !stopped() {
        let Some(request) = shared.input.pop() else {
            thread::sleep(Duration::from_millis(10));
            continue;
        };
        if request.segment.key.session_id != session || shared.superseded(&request) {
            continue;
        }
        if let Err(error) = validate(&request) {
            emit(
                shared,
                Response {
                    provisional: request.provisional,
                    key: request.segment.key,
                    result: Err(error.to_string()),
                },
            );
            continue;
        }
        id = id.saturating_add(1);
        let input = Input {
            id,
            text: request.segment.text.clone(),
            source: request
                .segment
                .language
                .clone()
                .unwrap_or_else(|| "en".into()),
            target: request.target.clone(),
            timeout_ms: request
                .deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(30_000) as u64,
        };
        let result = (|| -> anyhow::Result<_> {
            let mut encoded = serde_json::to_vec(&input)?;
            ensure!(encoded.len() < 65_536, "推理请求超过协议长度限制");
            encoded.push(b'\n');
            helper
                .input
                .try_send(encoded)
                .map_err(|_| anyhow::anyhow!("本地推理写入队列不可用"))?;
            match helper.receive(request.deadline, stopped)? {
                Output::Translation {
                    id: response_id,
                    result,
                } if response_id == id => Ok(result),
                _ => anyhow::bail!("翻译结果序号或协议无效"),
            }
        })();
        match result {
            Ok(result) => {
                if let Ok(mut snapshot) = status.lock() {
                    snapshot.status = result
                        .as_ref()
                        .err()
                        .cloned()
                        .unwrap_or_else(|| ready_label.clone());
                }
                if shared.superseded(&request) {
                    continue;
                }
                emit(
                    shared,
                    Response {
                        provisional: request.provisional,
                        key: request.segment.key,
                        result,
                    },
                );
            }
            Err(error) => {
                if !stopped() {
                    emit(
                        shared,
                        Response {
                            provisional: request.provisional,
                            key: request.segment.key,
                            result: Err(error.to_string()),
                        },
                    );
                }
                return Err(error);
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use subveil_core::subtitle::{SegmentKey, TranscriptSegment};
    #[cfg(unix)]
    fn shell(script: &str) -> anyhow::Result<Helper> {
        Helper::spawn_from(
            Path::new("/bin/sh"),
            &[std::ffi::OsStr::new("-c"), std::ffi::OsStr::new(script)],
        )
    }
    #[cfg(unix)]
    #[test]
    fn native_crash_and_oversized_protocol_fail_without_crashing_parent() -> anyhow::Result<()> {
        for script in ["exit 7", "head -c 70000 /dev/zero"] {
            let helper = shell(script)?;
            assert!(
                helper
                    .receive(Instant::now() + Duration::from_secs(2), || false)
                    .is_err()
            );
        }
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn cancellation_and_deadline_terminate_stalled_native_helper() -> anyhow::Result<()> {
        for cancelled in [true, false] {
            let started = Instant::now();
            let helper = shell("exec sleep 5")?;
            assert!(
                helper
                    .receive(Instant::now() + Duration::from_millis(30), || cancelled)
                    .is_err()
            );
            drop(helper);
            assert!(started.elapsed() < Duration::from_secs(1));
        }
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn blocked_stdin_cannot_prevent_cancellation_or_deadline() -> anyhow::Result<()> {
        for cancelled in [true, false] {
            let started = Instant::now();
            let helper = shell("exec sleep 5")?;
            // Larger than Linux's usual pipe capacity, without unsafe or a
            // platform-specific fcntl dependency. Exercises the actual writer.
            helper.input.try_send(vec![b'x'; 2 * 1024 * 1024])?;
            assert!(
                helper
                    .receive(Instant::now() + Duration::from_millis(40), || cancelled)
                    .is_err()
            );
            drop(helper);
            assert!(started.elapsed() < Duration::from_secs(1));
        }
        Ok(())
    }
    #[test]
    fn expired_and_unsupported_requests_reject_before_dispatch() {
        let mut request = Request::new(
            TranscriptSegment {
                key: SegmentKey {
                    session_id: 1,
                    segment_id: 1,
                    revision: 1,
                },
                text: "hello".into(),
                language: Some("en".into()),
                start_ms: 0,
                end_ms: 100,
            },
            "zh".into(),
        );
        assert!(validate(&request).is_ok());
        request.deadline = Instant::now() - Duration::from_secs(1);
        assert!(validate(&request).is_err());
        request.deadline = Instant::now() + Duration::from_secs(1);
        request.target = "fr".into();
        assert!(validate(&request).is_err());
    }
}
