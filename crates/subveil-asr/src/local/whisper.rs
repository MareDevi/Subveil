//! Bounded PCM IPC isolates Whisper's native runtime from sherpa and GPUI.
use super::offline::Segmenter;
use crate::{Config, Continuity, Shared, Transcript};
use anyhow::{Context as _, ensure};
use crossbeam_queue::ArrayQueue;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    ffi::OsStr,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    thread,
    time::{Duration, Instant},
};
use subveil_audio::FrameReceiver;
use subveil_core::inference::{Compute, Output, SpeechInput};

fn executable() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("SUBVEIL_WHISPER_BIN") {
        let path = PathBuf::from(path);
        ensure!(
            path.is_absolute() && path.is_file(),
            "SUBVEIL_WHISPER_BIN 必须是程序绝对路径"
        );
        return Ok(path);
    }
    let current = std::env::current_exe()?;
    let parent = current.parent().context("无法确定程序目录")?;
    let parent = if parent
        .file_name()
        .is_some_and(|s| s == "examples" || s == "deps")
    {
        parent.parent().context("无法确定开发程序目录")?
    } else {
        parent
    };
    let path = parent.join(format!("subveil-whisper{}", std::env::consts::EXE_SUFFIX));
    ensure!(
        path.is_file(),
        "找不到 subveil-whisper；请运行 mise run build 或 build:cuda，安装时放在应用同目录"
    );
    Ok(path)
}
/// Isolated native Whisper process, also used by the explicit offline benchmark.
pub struct WhisperEngine {
    child: Child,
    input: SyncSender<Vec<u8>>,
    output: Receiver<Result<Output, String>>,
    next_id: u64,
    poisoned: bool,
    decode_budget_ms: u64,
}
impl WhisperEngine {
    fn spawn(executable: &Path, arguments: &[&OsStr]) -> anyhow::Result<Self> {
        let mut child = Command::new(executable)
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("无法启动 Whisper 推理进程")?;
        let stdin = child.stdin.take().context("Whisper 缺少输入通道")?;
        let stdout = child.stdout.take().context("Whisper 缺少输出通道")?;
        let (input, requests) = mpsc::sync_channel::<Vec<u8>>(1);
        let (sender, output) = mpsc::sync_channel(1);
        let reader = thread::Builder::new()
            .name("subveil-whisper-read".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let mut line = Vec::new();
                    let result = match reader.by_ref().take(65_537).read_until(b'\n', &mut line) {
                        Ok(0) => break,
                        Ok(len) if len <= 65_536 && line.last() == Some(&b'\n') => {
                            serde_json::from_slice(&line)
                                .map_err(|_| "Whisper 返回无效协议".to_owned())
                        }
                        _ => Err("Whisper 响应过长或通道中断".into()),
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
        let writer = thread::Builder::new()
            .name("subveil-whisper-write".into())
            .spawn(move || {
                let mut stdin = stdin;
                while let Ok(bytes) = requests.recv() {
                    if stdin
                        .write_all(&bytes)
                        .and_then(|()| stdin.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            });
        if let Err(error) = writer {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.into());
        }
        Ok(Self {
            child,
            input,
            output,
            next_id: 0,
            poisoned: false,
            decode_budget_ms: 10_000,
        })
    }
    fn receive(&self, deadline: Instant, stopped: impl Fn() -> bool) -> anyhow::Result<Output> {
        loop {
            ensure!(!stopped(), "Whisper 已取消");
            ensure!(
                Instant::now() < deadline,
                "Whisper 推理进程超时；请重新开始识别"
            );
            match self.output.recv_timeout(Duration::from_millis(10)) {
                Ok(Ok(Output::Error(error))) | Ok(Err(error)) => anyhow::bail!(error),
                Ok(Ok(output)) => return Ok(output),
                Err(RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("Whisper 推理进程退出；检查驱动、模型和内存")
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }
    /// Load and warm a verified model. GPU choice means CUDA device 0 only.
    pub fn load(
        manifest: &Path,
        compute: Compute,
        stopped: impl Fn() -> bool,
    ) -> anyhow::Result<Self> {
        let device = match compute {
            Compute::Cpu => "cpu",
            Compute::Gpu(0) => "cuda",
            Compute::Gpu(_) => anyhow::bail!("Whisper 当前仅支持 CUDA 设备 0"),
        };
        let mut engine = Self::spawn(&executable()?, &[manifest.as_os_str(), OsStr::new(device)])?;
        engine.decode_budget_ms = if compute == Compute::Cpu {
            30_000
        } else {
            10_000
        };
        match engine.receive(Instant::now() + Duration::from_secs(30), stopped)? {
            Output::Ready(_) => Ok(engine),
            _ => anyhow::bail!("Whisper 加载协议无效"),
        }
    }
    /// Transcribe at most 30 seconds of mono 16 kHz PCM, with a hard deadline (CPU 30 seconds, CUDA 10 seconds).
    /// An error terminates the native process and poisons this engine.
    pub fn transcribe(
        &mut self,
        samples: &[f32],
        language: &str,
        stopped: impl Fn() -> bool,
    ) -> anyhow::Result<WhisperResult> {
        ensure!(!self.poisoned, "Whisper 进程已失败；请重新加载模型");
        let result = self.dispatch(samples, language, stopped);
        if result.is_err() {
            self.poisoned = true;
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        result
    }
    fn dispatch(
        &mut self,
        samples: &[f32],
        language: &str,
        stopped: impl Fn() -> bool,
    ) -> anyhow::Result<WhisperResult> {
        ensure!(
            (160..=480_000).contains(&samples.len())
                && samples.iter().all(|s| s.is_finite() && s.abs() <= 1.)
                && matches!(language, "en" | "ja" | "zh"),
            "Whisper PCM 或语种无效"
        );
        let deadline = Instant::now() + Duration::from_millis(self.decode_budget_ms);
        self.next_id = self
            .next_id
            .checked_add(1)
            .context("Whisper 请求编号溢出")?;
        let header = SpeechInput {
            id: self.next_id,
            language: language.into(),
            samples: samples.len(),
            timeout_ms: self.decode_budget_ms,
        };
        let mut bytes = serde_json::to_vec(&header)?;
        bytes.push(b'\n');
        bytes.reserve(samples.len() * 4);
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        self.input
            .try_send(bytes)
            .map_err(|_| anyhow::anyhow!("Whisper 输入通道已关闭或过载"))?;
        match self.receive(deadline, stopped)? {
            Output::Recognition {
                id,
                result,
                inference_ms,
            } if id == self.next_id => {
                let text = result.map_err(anyhow::Error::msg)?;
                ensure!(text.len() <= 16_384, "Whisper 文本过长");
                Ok(WhisperResult { text, inference_ms })
            }
            _ => anyhow::bail!("Whisper 返回过期或无效协议"),
        }
    }
}
impl Drop for WhisperEngine {
    fn drop(&mut self) {
        // Called on workers, never the GPUI thread. Killing closes both pipes and
        // releases a writer blocked on a stalled native reader.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
/// One complete, bounded transcription; no native partial semantics are implied.
#[non_exhaustive]
pub struct WhisperResult {
    /// Source transcript, never logged by the engine.
    pub text: String,
    /// Native decode duration without IPC or loading.
    pub inference_ms: u64,
}
struct Job {
    epoch: u64,
    audio: super::offline::SegmentAudio,
    queued: Instant,
}
struct Completed {
    job: Job,
    result: WhisperResult,
}
fn dropped_segment(shared: &Shared) {
    if let Ok(mut snapshot) = shared.snapshot.lock() {
        snapshot.dropped_segments += 1;
    }
}
pub(super) fn run(
    config: &Config,
    session: u64,
    frames: &FrameReceiver,
    shared: &Shared,
) -> anyhow::Result<()> {
    let manifest = config.local_model.as_deref().context("缺少 Whisper 清单")?;
    let engine = WhisperEngine::load(manifest, config.local_compute, || shared.stopped())?;
    run_loaded(config, session, frames, shared, engine)
}
fn run_loaded(
    config: &Config,
    session: u64,
    frames: &FrameReceiver,
    shared: &Shared,
    mut engine: WhisperEngine,
) -> anyhow::Result<()> {
    let mut short = config.clone();
    short.vad.max_segment_ms = config.whisper_block_ms;
    short.vad.pre_roll_ms = short.vad.pre_roll_ms.min(200);
    short.vad.min_speech_ms = short.vad.min_speech_ms.min(150);
    let mut segmenter = Segmenter::new(&short);
    let mut continuity = Continuity::default();
    let mut transcript = Transcript::new(session, &config.language);
    let mut last = Instant::now();
    let mut timed_out = false;
    let mut epoch = 0_u64;
    let jobs = ArrayQueue::<Job>::new(1);
    let results = ArrayQueue::<Result<Completed, String>>::new(2);
    let cancel = AtomicBool::new(false);
    while frames.pop().is_some() {}
    let label = match config.local_compute {
        Compute::Cpu => "CPU",
        _ => "CUDA0",
    };
    shared.status(
        format!(
            "本地 Whisper 已就绪 · {label} · {} ms 分块",
            config.whisper_block_ms
        ),
        frames,
        false,
    );
    thread::scope(|scope| {
        let decode = scope.spawn(|| {
            while !shared.stopped() && !cancel.load(Ordering::Relaxed) {
                let Some(job) = jobs.pop() else {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                };
                if job.queued.elapsed() > Duration::from_secs(1) {
                    dropped_segment(shared);
                    continue;
                }
                match engine.transcribe(&job.audio.samples, &config.language, || {
                    shared.stopped() || cancel.load(Ordering::Relaxed)
                }) {
                    Ok(result) => {
                        if results.force_push(Ok(Completed { job, result })).is_some() {
                            dropped_segment(shared);
                        }
                    }
                    Err(error) => {
                        results.force_push(Err(error.to_string()));
                        break;
                    }
                }
            }
            // Model and pipes must be released before the worker reports completion.
            drop(engine);
        });
        let result = (|| -> anyhow::Result<()> {
            while !shared.stopped() {
                while let Some(result) = results.pop() {
                    let completed = result.map_err(anyhow::Error::msg)?;
                    if completed.job.epoch != epoch {
                        dropped_segment(shared);
                        continue;
                    }
                    let audio = completed.job.audio;
                    let audio_ms =
                        audio.samples.len() as u64 * 1000 / u64::from(config.sample_rate);
                    if let Some(event) = transcript.update(
                        completed.result.text,
                        audio.start * 1000 / u64::from(config.sample_rate),
                        audio.end * 1000 / u64::from(config.sample_rate),
                        true,
                    ) {
                        shared.emit(event);
                    }
                    shared.status(
                        format!(
                            "本地 Whisper · {label} · 推理 {} ms / 音频 {audio_ms} ms",
                            completed.result.inference_ms
                        ),
                        frames,
                        false,
                    );
                    if let Ok(mut snapshot) = shared.snapshot.lock() {
                        snapshot.local_inference_ms = completed.result.inference_ms;
                        snapshot.local_audio_ms = audio_ms;
                    }
                }
                if let Some(frame) = frames.pop() {
                    if !continuity.accept(&frame, config.sample_rate) {
                        segmenter.reset();
                        transcript.reset();
                        epoch += 1;
                        while jobs.pop().is_some() {
                            dropped_segment(shared);
                        }
                        shared.status("音频不连续；已重置 Whisper 分块", frames, true);
                        continue;
                    }
                    last = Instant::now();
                    timed_out = false;
                    if let Some(audio) =
                        segmenter.accept(&frame.samples[..frame.len], frame.sample_offset)
                        && jobs
                            .force_push(Job {
                                epoch,
                                audio,
                                queued: Instant::now(),
                            })
                            .is_some()
                    {
                        dropped_segment(shared);
                        shared.status("Whisper 推理过载；已丢弃最旧等待分块", frames, false);
                    }
                } else {
                    if !timed_out && last.elapsed() >= Duration::from_secs(2) {
                        segmenter.reset();
                        transcript.reset();
                        epoch += 1;
                        timed_out = true;
                        while jobs.pop().is_some() {
                            dropped_segment(shared);
                        }
                        shared.status("等待音频；Whisper 分块已重置", frames, true);
                    }
                    thread::sleep(Duration::from_millis(5));
                }
            }
            Ok(())
        })();
        cancel.store(true, Ordering::Relaxed);
        decode
            .join()
            .map_err(|_| anyhow::anyhow!("Whisper 推理线程异常退出"))?;
        result
    })
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    fn shell(script: &str) -> anyhow::Result<WhisperEngine> {
        WhisperEngine::spawn(
            Path::new("/bin/sh"),
            &[OsStr::new("-c"), OsStr::new(script)],
        )
    }
    #[test]
    fn crash_oversize_cancel_and_deadline_fail_and_release_helper() -> anyhow::Result<()> {
        for script in ["exit 7", "head -c 70000 /dev/zero"] {
            let engine = shell(script)?;
            assert!(
                engine
                    .receive(Instant::now() + Duration::from_secs(2), || false)
                    .is_err()
            );
        }
        for cancel in [true, false] {
            let start = Instant::now();
            let engine = shell("exec sleep 5")?;
            assert!(
                engine
                    .receive(Instant::now() + Duration::from_millis(30), || cancel)
                    .is_err()
            );
            drop(engine);
            assert!(start.elapsed() < Duration::from_secs(1));
        }
        Ok(())
    }
    #[test]
    fn stalled_pcm_reader_does_not_block_cancellation() -> anyhow::Result<()> {
        let mut engine = shell("exec sleep 5")?;
        let start = Instant::now();
        assert!(
            engine
                .transcribe(&vec![0.; 480_000], "en", || true)
                .is_err()
        );
        drop(engine);
        assert!(start.elapsed() < Duration::from_secs(1));
        Ok(())
    }
    #[test]
    fn stalled_decode_keeps_consuming_pcm_and_bounds_pending_blocks() -> anyhow::Result<()> {
        use crossbeam_queue::ArrayQueue;
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicU64, Ordering},
        };
        let shared = Arc::new(Shared {
            wake: subveil_core::signal::Signal::default(),
            stop: AtomicBool::new(false),
            events: ArrayQueue::new(64),
            translations: ArrayQueue::new(64),
            snapshot: Mutex::new(crate::Snapshot::default()),
            dropped: AtomicU64::new(0),
        });
        let (sender, frames) = FrameReceiver::channel();
        let engine = shell("exec sleep 5")?;
        let state = shared.clone();
        let worker =
            thread::spawn(move || run_loaded(&Config::default(), 1, &frames, &state, engine));
        // Let run_loaded discard loading-time frames before this explicit replay.
        thread::sleep(Duration::from_millis(20));
        for sequence in 0..400 {
            sender.push(subveil_audio::AudioFrame {
                samples: [0.1; 2048],
                len: 320,
                generation: 1,
                sequence,
                sample_offset: sequence * 320,
                captured: Instant::now(),
                rate: 16000,
            });
            thread::sleep(Duration::from_millis(1));
        }
        let dropped = shared
            .snapshot
            .lock()
            .map_err(|_| anyhow::anyhow!("snapshot"))?
            .dropped_segments;
        let stopped = Instant::now();
        shared.stop.store(true, Ordering::Relaxed);
        worker.join().map_err(|_| anyhow::anyhow!("worker"))??;
        assert!(dropped >= 1);
        assert_eq!(
            shared
                .snapshot
                .lock()
                .map_err(|_| anyhow::anyhow!("snapshot"))?
                .dropped_audio,
            0
        );
        assert!(stopped.elapsed() < Duration::from_secs(1));
        Ok(())
    }
    #[test]
    fn mismatched_request_identity_is_rejected() -> anyhow::Result<()> {
        let mut engine = shell(
            "printf '%s\\n' '{\"Recognition\":{\"id\":99,\"result\":{\"Ok\":\"stale\"},\"inference_ms\":1}}'; exec sleep 1",
        )?;
        let error = engine
            .transcribe(&[0.; 160], "en", || false)
            .err()
            .context("stale must fail")?;
        assert!(error.to_string().contains("过期"));
        assert!(
            engine
                .transcribe(&[0.; 160], "en", || false)
                .err()
                .context("poisoned")?
                .to_string()
                .contains("已失败")
        );
        Ok(())
    }
}
