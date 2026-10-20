mod mock;
mod overlay;
mod session;
mod storage;
mod ui;

use std::{env, path::PathBuf};

use anyhow::{Context as _, bail};
use gpui::{App, AppContext};
use session::Session;
use subveil_core::config::OverlaySettings;
use tracing_subscriber::EnvFilter;

#[derive(Default)]
struct Arguments {
    demo: bool,
    config: Option<PathBuf>,
}

fn arguments() -> anyhow::Result<Option<Arguments>> {
    let mut parsed = Arguments::default();
    let mut args = env::args_os().skip(1);
    while let Some(argument) = args.next() {
        match argument.to_str() {
            Some("--demo") => parsed.demo = true,
            Some("--config") => {
                parsed.config = Some(args.next().context("--config requires a file path")?.into())
            }
            Some("--help" | "-h") => {
                println!(
                    "Subveil {}\n\nUsage: subveil [--demo] [--config PATH]\n\n  --demo         Start the offline mock subtitle demonstration\n  --config PATH  Read/write subtitle styles at PATH\n  --version      Print application version\n  --help         Print this help",
                    env!("CARGO_PKG_VERSION")
                );
                return Ok(None);
            }
            Some("--version" | "-V") => {
                println!("Subveil {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            _ => bail!(
                "unknown argument: {} (use --help)",
                argument.to_string_lossy()
            ),
        }
    }
    Ok(Some(parsed))
}

fn main() -> anyhow::Result<()> {
    let Some(arguments) = arguments()? else {
        return Ok(());
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize logging: {error}"))?;

    let (config_path, settings, config_error) = match arguments
        .config
        .map(Ok)
        .unwrap_or_else(storage::default_path)
    {
        Ok(path) => match storage::load(&path) {
            Ok(settings) => (Some(path), settings, None),
            Err(error) => (
                Some(path),
                OverlaySettings::default(),
                Some(format!("配置加载失败，暂用默认值；原文件未修改：{error:#}")),
            ),
        },
        Err(error) => (None, OverlaySettings::default(), Some(error.to_string())),
    };
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting Subveil");
    gpui_platform::application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        let session = cx.new(|cx| Session::new(settings, config_path, config_error, cx));
        let observer = session.clone();
        cx.on_window_closed(move |cx, id| {
            observer.update(cx, |session, cx| session.window_closed(id, cx))
        })
        .detach();
        session.update(cx, |session, cx| {
            session.open_control(cx);
            if arguments.demo {
                session.start(cx);
            }
        });
    });
    Ok(())
}
