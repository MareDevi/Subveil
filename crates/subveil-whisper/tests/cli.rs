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
    let message: Output =
        serde_json::from_slice(&output.stdout).context("structured startup error")?;
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
    let message: Output =
        serde_json::from_slice(&output.stdout).context("structured startup error")?;
    assert!(matches!(message, Output::Error(error) if error.contains("未启用 CUDA")));
    Ok(())
}
