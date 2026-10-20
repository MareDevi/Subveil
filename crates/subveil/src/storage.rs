use std::{
    env, fs,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
};

use anyhow::{Context as _, bail};
use subveil_core::config::OverlaySettings;

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
