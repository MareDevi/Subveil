use std::{
    env, fs,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
};

use anyhow::{Context as _, bail};
use subveil_core::config::OverlaySettings;

#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePreference {
    pub server: u32,
    pub serial: u64,
    pub kind: String,
    pub label: String,
}
impl SourcePreference {
    pub fn from_source(source: &subveil_audio::Source) -> Self {
        Self {
            server: source.server,
            serial: source.serial,
            label: source.label.clone(),
            kind: match source.kind {
                subveil_audio::SourceKind::Input => "input",
                subveil_audio::SourceKind::Output => "output",
                subveil_audio::SourceKind::Playback => "playback",
            }
            .into(),
        }
    }
    pub fn source(&self) -> anyhow::Result<subveil_audio::Source> {
        anyhow::ensure!(
            self.label.len() <= 512 && !self.label.chars().any(char::is_control),
            "音源名称无效"
        );
        let kind = match self.kind.as_str() {
            "input" => subveil_audio::SourceKind::Input,
            "output" => subveil_audio::SourceKind::Output,
            "playback" => subveil_audio::SourceKind::Playback,
            _ => bail!("音源类别无效"),
        };
        Ok(subveil_audio::Source {
            server: self.server,
            serial: self.serial,
            kind,
            label: self.label.clone(),
        })
    }
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceProfile {
    pub id: u32,
    pub name: String,
    pub provider: String,
    pub translation: String,
    pub language: String,
    pub target: String,
    #[serde(default)]
    pub source: Option<SourcePreference>,
}
impl Default for ServiceProfile {
    fn default() -> Self {
        Self {
            id: 0,
            name: "默认配置".into(),
            provider: "local".into(),
            translation: "local".into(),
            language: "en".into(),
            target: "zh".into(),
            source: None,
        }
    }
}

#[derive(Clone, Default, serde::Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocalPreferences {
    pub download_origin: String,
    pub asr_model: Option<PathBuf>,
    pub vad_model: Option<PathBuf>,
    pub translation_model: Option<PathBuf>,
    pub language: String,
    pub target: String,
    pub offline: bool,
    pub incremental_translation: bool,
    pub gpu_name: Option<String>,
    pub asr_cuda: bool,
    pub whisper_block_ms: Option<u32>,
    pub vad_hangover_ms: Option<u32>,
    pub vad_maximum_ms: Option<u32>,
    pub profiles: Vec<ServiceProfile>,
    pub selected_profile: u32,
    pub capture_source: Option<SourcePreference>,
}
impl LocalPreferences {
    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.vad_hangover_ms
                .is_none_or(|n| matches!(n, 300 | 600 | 1000))
                && self
                    .vad_maximum_ms
                    .is_none_or(|n| matches!(n, 5000 | 10000 | 15000)),
            "语音分段参数无效"
        );
        anyhow::ensure!(self.profiles.len() <= 32, "服务配置超过 32 个");
        let mut ids = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        for profile in &self.profiles {
            anyhow::ensure!(
                profile.id < 1_000_000
                    && ids.insert(profile.id)
                    && names.insert(&profile.name)
                    && !profile.name.trim().is_empty()
                    && profile.name.len() <= 64
                    && !profile.name.chars().any(char::is_control),
                "配置名称或标识无效"
            );
            let provider = subveil_asr::Provider::ALL
                .into_iter()
                .find(|p| p.id() == profile.provider)
                .context("配置服务无效")?;
            anyhow::ensure!(
                provider
                    .source_languages()
                    .contains(&profile.language.as_str())
                    && provider
                        .target_languages()
                        .contains(&profile.target.as_str())
                    && matches!(
                        profile.translation.as_str(),
                        "disabled" | "local" | "openai" | "deepl"
                    ),
                "配置语言或翻译服务无效"
            );
            if let Some(source) = &profile.source {
                source.source()?;
            }
        }
        anyhow::ensure!(
            self.profiles.is_empty() || ids.contains(&self.selected_profile),
            "所选服务配置不存在"
        );
        if let Some(source) = &self.capture_source {
            source.source()?;
        }
        Ok(())
    }
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
    let preferences: LocalPreferences = toml::from_str(&text)?;
    preferences.validate()?;
    Ok(preferences)
}

pub fn save_local(path: &Path, preferences: &LocalPreferences) -> anyhow::Result<()> {
    preferences.validate()?;
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
    fn profiles_reject_ambiguous_names_and_missing_selection() -> anyhow::Result<()> {
        let mut preferences = LocalPreferences {
            profiles: vec![ServiceProfile::default()],
            ..Default::default()
        };
        preferences.validate()?;
        preferences.profiles.push(ServiceProfile {
            id: 1,
            ..Default::default()
        });
        assert!(preferences.validate().is_err());
        preferences.profiles[1].name = "会议".into();
        preferences.validate()?;
        preferences.selected_profile = 9;
        assert!(preferences.validate().is_err());
        preferences.selected_profile = 1;
        preferences.profiles[1].source = Some(SourcePreference {
            server: 10,
            serial: 20,
            kind: "playback".into(),
            label: "播放器".into(),
        });
        let encoded = toml::to_string(&preferences)?;
        let restored: LocalPreferences = toml::from_str(&encoded)?;
        restored.validate()?;
        assert_eq!(restored.selected_profile, 1);
        assert_eq!(
            restored.profiles[1]
                .source
                .as_ref()
                .context("source missing")?
                .source()?
                .serial,
            20
        );
        assert!(!encoded.contains("allow_cloud"));
        assert!(!encoded.contains("allow_text_upload"));
        assert!(!encoded.contains("API_KEY"));
        Ok(())
    }
    #[test]
    fn local_preferences_roundtrip_preserves_offline_and_device_without_credentials()
    -> anyhow::Result<()> {
        let directory =
            env::temp_dir().join(format!("subveil-local-settings-{}", std::process::id()));
        fs::create_dir_all(&directory)?;
        let path = directory.join("local-models.toml");
        let preferences = LocalPreferences {
            asr_model: Some("/models/asr.toml".into()),
            vad_model: None,
            translation_model: Some("/models/translation.toml".into()),
            language: "ja".into(),
            target: "zh".into(),
            offline: true,
            incremental_translation: true,
            gpu_name: Some("Vulkan · test GPU".into()),
            asr_cuda: true,
            whisper_block_ms: Some(2000),
            vad_hangover_ms: Some(300),
            vad_maximum_ms: Some(5000),
            ..Default::default()
        };
        save_local(&path, &preferences)?;
        let restored = load_local(&path)?;
        assert!(restored.offline);
        assert!(restored.incremental_translation);
        assert_eq!(restored.gpu_name, preferences.gpu_name);
        assert_eq!(restored.translation_model, preferences.translation_model);
        assert!(restored.asr_cuda);
        assert_eq!(restored.whisper_block_ms, Some(2000));
        assert_eq!(restored.vad_hangover_ms, Some(300));
        assert_eq!(restored.vad_maximum_ms, Some(5000));
        let mut invalid = restored.clone();
        invalid.vad_hangover_ms = Some(0);
        assert!(save_local(&path, &invalid).is_err());
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
