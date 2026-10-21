//! Installation smoke check. Does not initialize the GUI, GPU, audio or model runtimes.
use anyhow::{Context as _, ensure};
use std::{
    io::{BufRead as _, Read as _},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use subveil_core::inference::Output;

fn probe(path: &Path, helper: &str, backend: &str) -> anyhow::Result<()> {
    let mut child = Command::new(path)
        .arg("--capabilities")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("缺少可执行 helper；请安装同一构建的完整程序目录")?;
    let result = (|| -> anyhow::Result<()> {
        let stdout = child.stdout.take().context("helper 输出不可用")?;
        let (send, recv) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = std::io::BufReader::new(stdout)
                .take(8193)
                .read_until(b'\n', &mut bytes)
                .map(|_| bytes);
            let _ = send.send(result);
        });
        let bytes = recv
            .recv_timeout(Duration::from_secs(5))
            .context("helper 握手超时")??;
        ensure!(bytes.len() <= 8192, "helper 握手过长");
        let output: Output = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("helper 握手格式无效；请重装完整程序"))?;
        let Output::Capabilities(build) = output else {
            anyhow::bail!("helper 未提供版本和后端能力");
        };
        build.verify(helper, backend).map_err(anyhow::Error::msg)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(status) = child.try_wait()? {
                ensure!(status.success(), "helper 检查异常退出");
                break;
            }
            ensure!(Instant::now() < deadline, "helper 检查退出超时");
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}
pub fn check() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let directory = exe.parent().context("应用目录无效")?;
    for (name, helper, backend) in [
        ("subveil-inference", "translation", "cpu"),
        ("subveil-whisper", "whisper", "cpu"),
    ] {
        probe(
            &directory.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
            helper,
            backend,
        )?;
    }
    let cuda = directory.join(format!(
        "subveil-whisper-cuda{}",
        std::env::consts::EXE_SUFFIX
    ));
    if cuda.exists() {
        probe(&cuda, "whisper", "cuda")?;
    }
    println!(
        "Subveil {}: helper protocol/version/build/backend checks passed; inference and display not tested",
        env!("CARGO_PKG_VERSION")
    );
    Ok(())
}
