use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use gpui::{Context, Task, WindowHandle, WindowId};
use gpui_component::Root;
use subveil_core::{config::OverlaySettings, subtitle::SubtitleState};

use crate::{
    mock,
    overlay::{self, OverlayView},
    storage, ui,
};

pub struct Session {
    pub subtitles: SubtitleState,
    pub settings: OverlaySettings,
    pub running: bool,
    pub message: String,
    pub config_error: Option<String>,
    pub config_path: Option<PathBuf>,
    pub saving: bool,
    pub audio: subveil_audio::Snapshot,
    audio_capture: Option<subveil_audio::Capture>,
    pub audio_stopping: bool,
    overlay: Option<WindowHandle<OverlayView>>,
    overlay_requested: bool,
    control: Option<WindowHandle<Root>>,
    recovery: Option<WindowHandle<Root>>,
    started: Instant,
    tick: u64,
    next_tick: Instant,
    _timer: Task<()>,
}

impl Session {
    pub fn new(
        settings: OverlaySettings,
        config_path: Option<PathBuf>,
        config_error: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let timer = cx.spawn(async move |session, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                if session.update(cx, |session, cx| session.tick(cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            subtitles: SubtitleState::new(1, settings.history_limit, settings.ttl_ms),
            settings,
            running: false,
            message: "准备就绪；点击开始演示。样式只在点击保存时写入。".into(),
            config_error,
            config_path,
            saving: false,
            audio: subveil_audio::Snapshot::default(),
            audio_capture: None,
            audio_stopping: false,
            overlay: None,
            overlay_requested: false,
            control: None,
            recovery: None,
            started: Instant::now(),
            tick: 0,
            next_tick: Instant::now(),
            _timer: timer,
        }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let now_ms = self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        let mut changed = self.subtitles.expire(now_ms);
        if let Some(capture) = &self.audio_capture {
            if let Some(snapshot) = capture.snapshot() {
                self.audio = snapshot;
                changed = true;
            }
            if capture.is_finished() {
                if !self.audio_stopping {
                    self.audio.status = "音频线程意外退出；请重新开始采集".into();
                }
                self.audio_capture = None;
                self.audio_stopping = false;
                changed = true;
            }
        }
        if self.running && now >= self.next_tick {
            changed |= mock::advance(&mut self.subtitles, self.tick, now_ms);
            self.tick = self.tick.saturating_add(1);
            self.next_tick = now + Duration::from_millis(250);
        }
        if changed {
            cx.notify();
        }
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        self.subtitles
            .reset(self.subtitles.session_id().saturating_add(1));
        self.started = Instant::now();
        self.next_tick = Instant::now();
        self.tick = 0;
        self.running = true;
        self.message = "模拟演示已启动；没有采集音频，也没有调用翻译服务。".into();
        self.show_overlay(cx);
        cx.notify();
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        self.running = false;
        self.subtitles
            .reset(self.subtitles.session_id().saturating_add(1));
        self.message = "演示已停止，字幕已清空。".into();
        cx.notify();
    }

    pub fn audio_active(&self) -> bool {
        self.audio_capture.is_some()
    }

    pub fn toggle_audio(&mut self, cx: &mut Context<Self>) {
        if let Some(capture) = &self.audio_capture {
            capture.stop();
            self.audio_stopping = true;
            self.audio.status = "正在停止采集…".into();
        } else {
            match subveil_audio::Capture::start(self.audio.sample_rate) {
                Ok(capture) => {
                    self.audio = subveil_audio::Snapshot {
                        sample_rate: self.audio.sample_rate,
                        status: "连接 PipeWire…".into(),
                        ..Default::default()
                    };
                    self.audio_capture = Some(capture);
                }
                Err(error) => self.audio.status = format!("无法开始采集：{error:#}"),
            }
        }
        cx.notify();
    }

    pub fn cycle_audio_rate(&mut self, cx: &mut Context<Self>) {
        if self.audio_capture.is_none() {
            self.audio.sample_rate = match self.audio.sample_rate {
                48_000 => 16_000,
                16_000 => 24_000,
                _ => 48_000,
            };
            cx.notify();
        }
    }

    pub fn overlay_visible(&self) -> bool {
        self.overlay.is_some()
    }

    pub fn toggle_overlay(&mut self, cx: &mut Context<Self>) {
        if self.overlay_requested {
            self.overlay_requested = false;
            self.close_overlay(cx);
        } else {
            self.show_overlay(cx);
        }
        cx.notify();
    }

    fn close_overlay(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.overlay.take() {
            cx.defer(move |cx| {
                if let Err(error) = handle.update(cx, |_, window, _| window.remove_window()) {
                    tracing::debug!(?error, "overlay was already closed");
                }
            });
        }
    }

    fn show_overlay(&mut self, cx: &mut Context<Self>) {
        self.overlay_requested = true;
        let session = cx.entity();
        // GPUI renders new windows synchronously, so the Session update must end first.
        cx.defer(move |cx| {
            let current = session.read(cx);
            if !current.overlay_requested || current.overlay.is_some() {
                return;
            }
            let settings = current.settings.clone();
            let result = overlay::open(session.clone(), &settings, cx);
            session.update(cx, |session, cx| {
                match result {
                    Ok(window) => session.overlay = Some(window),
                    Err(error) => {
                        session.overlay_requested = false;
                        session.message = format!("无法显示字幕浮窗：{error:#}");
                        tracing::error!(?error, "cannot open subtitle overlay");
                    }
                }
                cx.notify();
            });
        });
    }

    pub fn change_settings(
        &mut self,
        change: impl FnOnce(&mut OverlaySettings),
        cx: &mut Context<Self>,
    ) {
        let mut settings = self.settings.clone();
        change(&mut settings);
        if let Err(error) = settings.validate() {
            self.message = error.to_string();
        } else {
            self.settings = settings;
            self.message = "样式已应用；点击「保存样式」持久化。".into();
            if self.overlay.is_some() {
                self.close_overlay(cx);
                self.show_overlay(cx);
            }
        }
        cx.notify();
    }

    pub fn save_settings(&mut self, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let Some(path) = self.config_path.clone() else {
            self.message = "没有可用的配置目录，请使用 --config 指定路径。".into();
            cx.notify();
            return;
        };
        let settings = self.settings.clone();
        self.saving = true;
        cx.notify();
        cx.spawn(async move |session, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { storage::save(&path, &settings) })
                .await;
            let _ = session.update(cx, |session, cx| {
                session.saving = false;
                session.message = match result {
                    Ok(()) => {
                        session.config_error = None;
                        "字幕样式已保存。".into()
                    }
                    Err(error) => format!("保存失败：{error:#}"),
                };
                cx.notify();
            });
        })
        .detach();
    }

    pub fn open_control(&mut self, cx: &mut Context<Self>) {
        let session = cx.entity();
        cx.defer(move |cx| {
            if session.read(cx).control.is_some() {
                return;
            }
            match ui::open_control(session.clone(), cx) {
                Ok(handle) => {
                    let recovery = session.update(cx, |session, _| {
                        session.control = Some(handle);
                        session.recovery.take()
                    });
                    if let Some(recovery) = recovery
                        && let Err(error) =
                            recovery.update(cx, |_, window, _| window.remove_window())
                    {
                        tracing::debug!(?error, "recovery window was already closed");
                    }
                }
                Err(error) => {
                    tracing::error!(?error, "cannot open control window");
                    session.update(cx, |session, cx| {
                        session.message = format!("无法打开控制窗口：{error:#}");
                        cx.notify();
                    });
                    if cx.windows().is_empty() {
                        std::process::exit(1);
                    }
                }
            }
        });
    }

    pub fn restore_control(&mut self, cx: &mut Context<Self>) {
        self.open_control(cx);
    }

    pub fn window_closed(&mut self, id: WindowId, cx: &mut Context<Self>) {
        if self.control.is_some_and(|handle| handle.window_id() == id) {
            self.control = None;
            if self.running || self.overlay_requested || self.audio_capture.is_some() {
                let session = cx.entity();
                cx.defer(move |cx| match ui::open_recovery(session.clone(), cx) {
                    Ok(handle) => session.update(cx, |session, _| session.recovery = Some(handle)),
                    Err(error) => {
                        tracing::error!(?error, "cannot create session recovery window");
                        cx.quit();
                    }
                });
            } else {
                cx.quit();
            }
        } else if self.recovery.is_some_and(|handle| handle.window_id() == id) {
            cx.quit();
        } else if self.overlay.is_some_and(|handle| handle.window_id() == id) {
            self.overlay = None;
            self.overlay_requested = false;
            self.message = "字幕窗口已被 compositor 关闭，可点击「显示浮窗」重建。".into();
            cx.notify();
        }
    }
}
