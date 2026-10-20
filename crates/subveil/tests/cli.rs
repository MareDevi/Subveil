use std::process::Command;

#[test]
fn help_and_version_work_without_a_display() -> std::io::Result<()> {
    for argument in ["--help", "--version"] {
        let output = Command::new(env!("CARGO_BIN_EXE_subveil"))
            .arg(argument)
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .output()?;
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Subveil"));
    }
    Ok(())
}

#[test]
fn invalid_arguments_fail_before_initializing_the_gui() -> std::io::Result<()> {
    for argument in ["--unknown", "--config"] {
        let output = Command::new(env!("CARGO_BIN_EXE_subveil"))
            .arg(argument)
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .output()?;
        assert!(!output.status.success());
        assert!(!output.stderr.is_empty());
    }
    Ok(())
}
