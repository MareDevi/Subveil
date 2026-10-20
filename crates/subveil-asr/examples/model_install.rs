//! Explicit diagnostic using the same installer as the native application.
use anyhow::{Context as _, ensure};
use std::{path::PathBuf, thread, time::Duration};
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let catalog = subveil_asr::models::catalog()?;
    if args.is_empty() || args[0] == "--list" {
        for model in catalog {
            println!(
                "{}: {} · {} · {:.1} MB · {}",
                model.id,
                model.name,
                model.kind.name(),
                model.size() as f64 / 1_000_000.,
                model.license
            );
        }
        return Ok(());
    }
    ensure!(
        args.len() <= 2,
        "usage: model_install [--list | MODEL_ID [DATA_ROOT]]"
    );
    let model = catalog
        .into_iter()
        .find(|model| model.id == args[0])
        .context("未知模型 ID")?;
    let root = args
        .get(1)
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(subveil_asr::models::data_root)?;
    println!(
        "Explicit installation: {}\nSource: {}\nLicense: {} ({})\nSize: {:.1} MB\nDirectory: {}",
        model.name,
        model.source(),
        model.license,
        model.license_url,
        model.size() as f64 / 1_000_000.,
        root.display()
    );
    let download = subveil_asr::models::Download::start(model, root)?;
    loop {
        if let Some(snapshot) = download.snapshot() {
            println!(
                "{}: {:.1}/{:.1} MB",
                snapshot.status,
                snapshot.completed as f64 / 1_000_000.,
                snapshot.total as f64 / 1_000_000.
            );
            if snapshot.finished {
                let manifest = snapshot.manifest.context(snapshot.status)?;
                println!("Ready: {}", manifest.display());
                return Ok(());
            }
        }
        thread::sleep(Duration::from_secs(1));
    }
}
