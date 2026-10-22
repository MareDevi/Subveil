//! Optional desktop services. D-Bus and portal consent never run on the GPUI thread.
use anyhow::Context as _;
use futures_util::StreamExt as _;
use ksni::TrayMethods as _;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};
use std::time::Duration;

pub const RESTORE: u8 = 1;
pub const TOGGLE: u8 = 2;
pub const STOP: u8 = 4;
pub const QUIT: u8 = 8;

#[derive(Clone, Default)]
struct Shared {
    commands: Arc<AtomicU8>,
    status: Arc<Mutex<String>>,
}
impl Shared {
    fn command(&self, command: u8) {
        if command == TOGGLE {
            // Toggle bursts retain parity; stop/restore/quit coalesce without queue growth.
            self.commands.fetch_xor(command, Ordering::AcqRel);
        } else {
            self.commands.fetch_or(command, Ordering::AcqRel);
        }
    }
    fn status(&self, text: impl Into<String>) {
        if let Ok(mut status) = self.status.lock() {
            *status = text.into();
        }
    }
}

pub struct Service {
    shared: Shared,
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
}
impl Service {
    pub fn start(shortcuts: bool) -> anyhow::Result<Self> {
        let shared = Shared::default();
        shared.status("正在连接桌面服务…");
        let state = shared.clone();
        let (cancel, stop) = tokio::sync::oneshot::channel();
        std::thread::Builder::new()
            .name("subveil-desktop".into())
            .spawn(move || {
                let result = (|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()?;
                    runtime.block_on(async {
                        if shortcuts {
                            global_shortcuts(state.clone(), stop).await
                        } else {
                            tray(state.clone(), stop).await
                        }
                    })
                })();
                if let Err(error) = result {
                    state.status(format!("桌面服务不可用：{error:#}"));
                }
            })
            .context("无法启动桌面服务线程")?;
        Ok(Self {
            shared,
            cancel: Some(cancel),
        })
    }
    pub fn poll(&self) -> (u8, String) {
        (
            self.shared.commands.swap(0, Ordering::AcqRel),
            self.shared
                .status
                .lock()
                .map(|s| s.clone())
                .unwrap_or_else(|_| "桌面服务状态不可用".into()),
        )
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
    }
}

struct Tray(Shared);
impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "subveil".into()
    }
    fn title(&self) -> String {
        "Subveil · 字幕".into()
    }
    fn icon_name(&self) -> String {
        "preferences-desktop-accessibility".into()
    }
    fn activate(&mut self, _: i32, _: i32) {
        self.0.command(RESTORE);
    }
    fn watcher_online(&self) {
        self.0.status("托盘已连接");
    }
    fn watcher_offline(&self, _: ksni::OfflineReason) -> bool {
        self.0.status("桌面托盘已断开；控制窗口与恢复入口仍可用");
        true
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        [
            ("恢复控制窗口", RESTORE),
            ("显示或隐藏字幕", TOGGLE),
            ("停止字幕会话", STOP),
            ("退出 Subveil", QUIT),
        ]
        .into_iter()
        .map(|(label, command)| {
            ksni::menu::StandardItem {
                label: label.into(),
                activate: Box::new(move |tray: &mut Self| tray.0.command(command)),
                ..Default::default()
            }
            .into()
        })
        .collect()
    }
}
async fn tray(shared: Shared, mut stop: tokio::sync::oneshot::Receiver<()>) -> anyhow::Result<()> {
    let handle = tokio::select! {
        _ = &mut stop => return Ok(()),
        result = tokio::time::timeout(Duration::from_secs(10), Tray(shared.clone()).spawn()) => result??,
    };
    shared.status("托盘已连接");
    let _ = stop.await;
    tokio::time::timeout(Duration::from_secs(2), handle.shutdown()).await?;
    Ok(())
}
async fn global_shortcuts(
    shared: Shared,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    let portal = tokio::select! {
        _ = &mut stop => return Ok(()),
        result = tokio::time::timeout(Duration::from_secs(10), GlobalShortcuts::new()) => result??,
    };
    let session = tokio::select! {
        _ = &mut stop => return Ok(()),
        result = tokio::time::timeout(Duration::from_secs(10), portal.create_session(Default::default())) => result??,
    };
    let result = async {
        let path = serde_json::to_value(&session)?;
        let path = path.as_str().context("Portal 会话标识无效")?;
        let mut events = portal.receive_activated().await?;
        let mut closed = session.receive_closed().await?;
        let shortcuts = [
            NewShortcut::new("restore", "恢复 Subveil 控制窗口").preferred_trigger("CTRL+SHIFT+F10"),
            NewShortcut::new("overlay", "显示或隐藏 Subveil 字幕").preferred_trigger("CTRL+SHIFT+F11"),
            NewShortcut::new("stop", "停止 Subveil 字幕会话").preferred_trigger("CTRL+SHIFT+F12"),
        ];
        shared.status("等待桌面授权快捷键；可在应用中停用取消");
        let bound = tokio::select! {
            _ = &mut stop => return Ok(()),
            result = tokio::time::timeout(Duration::from_secs(120), portal.bind_shortcuts(&session, &shortcuts, None, Default::default())) => result??.response()?,
        };
        anyhow::ensure!(["restore", "overlay", "stop"].iter().all(|id| bound.shortcuts().iter().any(|s| s.id() == *id)), "桌面未授权全部快捷键；请重新启用");
        shared.status(format!("已绑定：{}", bound.shortcuts().iter().map(|s| format!("{} {}", s.description(), s.trigger_description())).collect::<Vec<_>>().join("；")));
        loop {
            tokio::select! {
                _ = &mut stop => break,
                _ = closed.next() => anyhow::bail!("桌面已关闭快捷键会话；请重新启用"),
                event = events.next() => {
                    let Some(event) = event else { anyhow::bail!("快捷键事件流已断开") };
                    if event.session_handle().as_str() != path { continue; }
                    let command = match event.shortcut_id() { "restore" => RESTORE, "overlay" => TOGGLE, "stop" => STOP, _ => continue };
                    shared.command(command);
                }
            }
        }
        Ok(())
    }.await;
    // Also close after denial, timeout or local cancellation; do not leave hidden bindings.
    let close = tokio::time::timeout(Duration::from_secs(2), session.close()).await;
    result?;
    close??;
    Ok(())
}

/// Explicit desktop diagnostic. Registers a temporary tray, never binds keys or captures audio.
pub fn check() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let shared = Shared::default();
        match tokio::time::timeout(Duration::from_secs(10), Tray(shared).spawn()).await {
            Ok(Ok(handle)) => {
                println!("tray: registered with StatusNotifierWatcher");
                tokio::time::timeout(Duration::from_secs(2), handle.shutdown()).await?;
            }
            Ok(Err(error)) => println!("tray: unavailable ({error})"),
            Err(_) => println!("tray: connection timed out"),
        }
        match tokio::time::timeout(
            Duration::from_secs(10),
            ashpd::desktop::global_shortcuts::GlobalShortcuts::new(),
        )
        .await
        {
            Ok(Ok(_)) => println!(
                "global shortcuts: portal interface available; binding/activation not tested"
            ),
            Ok(Err(error)) => println!("global shortcuts: unavailable ({error})"),
            Err(_) => println!("global shortcuts: connection timed out"),
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn burst_commands_are_bounded_and_stop_is_never_displaced() {
        let shared = Shared::default();
        for _ in 0..1000 {
            shared.command(TOGGLE);
            shared.command(RESTORE);
        }
        shared.command(STOP);
        assert_eq!(shared.commands.swap(0, Ordering::AcqRel), RESTORE | STOP);
        assert_eq!(shared.commands.load(Ordering::Acquire), 0);
        shared.command(TOGGLE);
        assert_eq!(shared.commands.load(Ordering::Acquire), TOGGLE);
    }
}
