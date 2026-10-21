use std::{
    env, fs,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
};

use anyhow::{Context as _, bail};
use subveil_core::config::OverlaySettings;

#[derive(Clone, Default, serde::Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocalPreferences {
    pub asr_model: Option<PathBuf>,
    pub translation_model: Option<PathBuf>,
    pub language: String,
    pub target: String,
    pub offline: bool,
    pub gpu_name: Option<String>,
    pub asr_cuda: bool,
    pub whisper_block_ms: Option<u32>,
}

pub fn load_local(path: &Path) -> anyhow::Result<LocalPreferences> {
    if !path.exists() {
        return Ok(LocalPreferences::default());
    }
    let mut text = String::new();
    fs::File::open(path)?
        .take(65_537)
        .read_to_string(&mut text)?;
    if text.len() > 65_536 {
        bail!("local model configuration exceeds 64 KiB");
    }
    Ok(toml::from_str(&text)?)
}

pub fn save_local(path: &Path, preferences: &LocalPreferences) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .context("local preferences directory missing")?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let result = (|| -> anyhow::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(toml::to_string(preferences)?.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

pub fn default_path() -> anyhow::Result<PathBuf> {
    resolve_path(
        env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        env::var_os("HOME").map(PathBuf::from),
    )
}

fn resolve_path(xdg: Option<PathBuf>, home: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let base = xdg
        .filter(|path| path.is_absolute())
        .or_else(|| {
            home.filter(|path| path.is_absolute())
                .map(|path| path.join(".config"))
        })
        .context("cannot resolve an absolute XDG configuration directory")?;
    Ok(base.join("subveil/subtitle.toml"))
}

pub fn load(path: &Path) -> anyhow::Result<OverlaySettings> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(OverlaySettings::default());
        }
        Err(error) => return Err(error).context("cannot open subtitle configuration"),
    };
    let mut text = String::new();
    file.take(65_537)
        .read_to_string(&mut text)
        .context("cannot read subtitle configuration")?;
    if text.len() > 65_536 {
        bail!("subtitle configuration exceeds 64 KiB");
    }
    Ok(OverlaySettings::from_toml(&text)?)
}

pub fn save(path: &Path, settings: &OverlaySettings) -> anyhow::Result<()> {
    let text = settings.to_toml()?;
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).context("cannot create subtitle configuration directory")?;
    let temporary = path.with_extension(format!("toml.tmp-{}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .context("cannot create temporary subtitle configuration")?;
    let result = (|| -> anyhow::Result<()> {
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.context("cannot save subtitle configuration")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_preferences_roundtrip_preserves_offline_and_device_without_credentials()
    -> anyhow::Result<()> {
        let directory =
            env::temp_dir().join(format!("subveil-local-settings-{}", std::process::id()));
        fs::create_dir_all(&directory)?;
        let path = directory.join("local-models.toml");
        let preferences = LocalPreferences {
            asr_model: Some("/models/asr.toml".into()),
            translation_model: Some("/models/translation.toml".into()),
            language: "ja".into(),
            target: "zh".into(),
            offline: true,
            gpu_name: Some("Vulkan · test GPU".into()),
            asr_cuda: true,
            whisper_block_ms: Some(2000),
        };
        save_local(&path, &preferences)?;
        let restored = load_local(&path)?;
        assert!(restored.offline);
        assert_eq!(restored.gpu_name, preferences.gpu_name);
        assert_eq!(restored.translation_model, preferences.translation_model);
        assert!(restored.asr_cuda);
        assert_eq!(restored.whisper_block_ms, Some(2000));
        fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn file_round_trip_and_invalid_file_preservation() -> anyhow::Result<()> {
        let directory =
            env::temp_dir().join(format!("subveil-storage-test-{}", std::process::id()));
        fs::create_dir(&directory)?;
        let path = directory.join("subtitle.toml");
        let result = (|| -> anyhow::Result<()> {
            assert_eq!(load(&path)?, OverlaySettings::default());
            let settings = OverlaySettings {
                font_size: 44.,
                ..Default::default()
            };
            save(&path, &settings)?;
            assert_eq!(load(&path)?, settings);
            let invalid = "font_size = nan";
            fs::write(&path, invalid)?;
            assert!(load(&path).is_err());
            assert_eq!(fs::read_to_string(&path)?, invalid);
            fs::write(&path, "x".repeat(65_537))?;
            assert!(load(&path).is_err());
            Ok(())
        })();
        fs::remove_dir_all(directory)?;
        result
    }

    #[test]
    fn xdg_path_must_be_absolute_and_empty_values_use_home() -> anyhow::Result<()> {
        assert_eq!(
            resolve_path(Some("relative".into()), Some("/home/test".into()))?,
            PathBuf::from("/home/test/.config/subveil/subtitle.toml")
        );
        assert_eq!(
            resolve_path(Some("/tmp/config".into()), None)?,
            PathBuf::from("/tmp/config/subveil/subtitle.toml")
        );
        assert!(resolve_path(None, None).is_err());
        Ok(())
    }
}
