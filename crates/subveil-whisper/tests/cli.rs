//! Process-level failures must remain structured and never require a real model or GPU.
use anyhow::{Context as _, ensure};
use std::process::Command;
use subveil_core::inference::Output;
#[test]
fn invalid_device_is_rejected_before_model_loading() -> anyhow::Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_subveil-whisper"))
        .args(["/nonexistent/model.toml", "invalid-device"])
        .output()?;
    ensure!(!output.status.success(), "invalid device succeeded");
    let mut messages = serde_json::Deserializer::from_slice(&output.stdout).into_iter::<Output>();
    let first = messages.next().context("missing build")??;
    assert!(matches!(first, Output::Capabilities(_)));
    let message = messages.next().context("missing startup error")??;
    assert!(matches!(message, Output::Error(error) if error.contains("运行设备")));
    Ok(())
}
#[cfg(not(feature = "cuda"))]
#[test]
fn cpu_build_cannot_silently_accept_cuda_request() -> anyhow::Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_subveil-whisper"))
        .args(["/nonexistent/model.toml", "cuda"])
        .output()?;
    assert!(!output.status.success());
    let mut messages = serde_json::Deserializer::from_slice(&output.stdout).into_iter::<Output>();
    let first = messages.next().context("missing build")??;
    assert!(matches!(first, Output::Capabilities(_)));
    let message = messages.next().context("missing startup error")??;
    assert!(matches!(message, Output::Error(error) if error.contains("未启用 CUDA")));
    Ok(())
}

#[test]
fn capabilities_need_no_models_devices_or_display() -> anyhow::Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_subveil-whisper"))
        .arg("--capabilities")
        .output()?;
    ensure!(output.status.success(), "capability probe failed");
    let Output::Capabilities(build) = serde_json::from_slice(&output.stdout)? else {
        anyhow::bail!("missing capabilities");
    };
    build.verify("whisper", "cpu").map_err(anyhow::Error::msg)?;
    assert_eq!(
        build.backends.iter().any(|s| s == "cuda"),
        cfg!(feature = "cuda")
    );
    Ok(())
}
