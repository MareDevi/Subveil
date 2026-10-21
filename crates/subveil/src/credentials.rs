//! Secret Service persistence. All calls run on the background executor.
use anyhow::{Context as _, ensure};
use std::{
    io::{Read as _, Write as _},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use subveil_core::services::ServiceSettings;

pub fn availability() -> String {
    let installed = std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|p| p.join("secret-tool").is_file()));
    if !installed {
        "系统密钥环不可用：请安装 libsecret 的 secret-tool；当前配置仍可仅在内存使用".into()
    } else if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        "未检测到会话总线；密钥环读写可能不可用".into()
    } else {
        "已找到 secret-tool 与会话总线；点击读取或保存时检查密钥环，不会自动上传".into()
    }
}
fn attributes(profile: u32) -> Vec<String> {
    let mut attributes = vec![
        "application".into(),
        "subveil".into(),
        "kind".into(),
        if profile == 0 {
            "cloud-services"
        } else {
            "cloud-services-profile"
        }
        .into(),
    ];
    if profile != 0 {
        attributes.extend(["profile".into(), profile.to_string()]);
    }
    attributes
}
fn command(action: &str, input: Option<&[u8]>, profile: u32) -> anyhow::Result<Vec<u8>> {
    let mut command = Command::new("secret-tool");
    command.arg(action);
    if action == "store" {
        command.args(["--label", "Subveil cloud services"]);
    }
    command.args(attributes(profile));
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .context("无法启动系统凭据存储；需要 secret-tool 和可用的 Secret Service")?;
    let result = (|| -> anyhow::Result<Vec<u8>> {
        let writer = if let Some(input) = input {
            let mut stdin = child.stdin.take().context("凭据输入通道不可用")?;
            let bytes = input.to_vec();
            Some(std::thread::spawn(move || stdin.write_all(&bytes)))
        } else {
            None
        };
        let stdout = child.stdout.take().context("凭据输出通道不可用")?;
        // Drain concurrently: a stored profile may exceed a pipe's capacity.
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout.take(65_537).read_to_end(&mut bytes).map(|_| bytes)
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                anyhow::bail!("系统凭据存储超时；请解锁密钥环后重试");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        if let Some(writer) = writer {
            writer
                .join()
                .map_err(|_| anyhow::anyhow!("凭据写入线程退出"))??;
        }
        let bytes = reader
            .join()
            .map_err(|_| anyhow::anyhow!("凭据读取线程退出"))??;
        ensure!(
            status.success(),
            "系统凭据操作未完成；请检查密钥环是否已解锁且存在已保存配置"
        );
        ensure!(bytes.len() <= 65_536, "系统凭据数据超过 64 KiB");
        Ok(bytes)
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}
pub fn save(settings: &ServiceSettings, profile: u32) -> anyhow::Result<()> {
    for key in settings.0.keys() {
        settings.get(key)?;
    }
    let encoded = toml::to_string(settings)?;
    ensure!(encoded.len() <= 65_536, "云服务配置超过 64 KiB");
    command("store", Some(encoded.as_bytes()), profile)?;
    Ok(())
}
pub fn load(profile: u32) -> anyhow::Result<ServiceSettings> {
    let bytes = command("lookup", None, profile)?;
    // Never propagate parser errors: they can quote credential-bearing source lines.
    let text = std::str::from_utf8(&bytes).map_err(|_| anyhow::anyhow!("系统凭据编码无效"))?;
    let settings: ServiceSettings =
        toml::from_str(text).map_err(|_| anyhow::anyhow!("系统凭据格式无效"))?;
    for key in settings.0.keys() {
        settings.get(key)?;
    }
    Ok(settings)
}
pub fn clear(profile: u32) -> anyhow::Result<()> {
    command("clear", None, profile)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn legacy_lookup_and_clear_cannot_match_named_profiles() {
        let legacy = super::attributes(0);
        assert_eq!(legacy, ["application", "subveil", "kind", "cloud-services"]);
        let first = super::attributes(1);
        let second = super::attributes(2);
        assert_ne!(legacy[3], first[3]);
        assert_ne!(first[5], second[5]);
        assert_eq!(first[4], "profile");
    }
}
