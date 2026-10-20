use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use gpui_kit::component::Root;
use gpui_kit::{Context, Task, WindowHandle, WindowId};
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
    recognition: Option<subveil_asr::Recognition>,
    pub asr_config: subveil_asr::Config,
    pub asr: subveil_asr::Snapshot,
    pub asr_stopping: bool,
    translation: Option<subveil_translate::Translation>,
    pub translation_provider: subveil_translate::Provider,
    pub allow_text_upload: bool,
    pub translation_status: String,
    pub translation_dropped: u64,
    pub model_info: String,
    local_language: Option<String>,
    pub importing_model: bool,
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
            message: "准备就绪；选择 ASR 或开始离线演示。样式只在点击保存时写入。".into(),
            config_error,
            config_path,
            saving: false,
            audio: subveil_audio::Snapshot::default(),
            audio_capture: None,
            recognition: None,
            asr_config: subveil_asr::Config::default(),
            asr: subveil_asr::Snapshot {
                status: "尚未启动识别".into(),
                ..Default::default()
            },
            asr_stopping: false,
            translation: None,
            translation_provider: subveil_translate::Provider::Disabled,
            allow_text_upload: false,
            translation_status: "文字翻译已关闭".into(),
            translation_dropped: 0,
            model_info: "尚未导入模型清单".into(),
            local_language: None,
            importing_model: false,
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
        if let Some(recognition) = &self.recognition {
            if let Some(snapshot) = recognition.snapshot() {
                self.asr = snapshot;
                changed = true;
            }
            // Results remain bounded; stopped sessions never apply late events.
            for _ in 0..64 {
                let Some(event) = recognition.pop() else {
                    break;
                };
                if !self.asr_stopping {
                    let final_segment = match &event {
                        subveil_core::subtitle::TranscriptEvent::Final(segment) => {
                            Some(segment.clone())
                        }
                        _ => None,
                    };
                    if self.subtitles.apply(event, now_ms) {
                        changed = true;
                        if let (Some(segment), Some(translation)) =
                            (final_segment, &self.translation)
                            && self.subtitles.request_translation(segment.key)
                        {
                            translation.submit(subveil_translate::Request::new(
                                segment,
                                self.asr_config.target_language.clone(),
                            ));
                        }
                    }
                }
            }
            for _ in 0..64 {
                let Some(translation) = recognition.pop_translation() else {
                    break;
                };
                if !self.asr_stopping {
                    changed |= self.subtitles.apply_provider_translation(
                        translation.key,
                        translation.text,
                        translation.is_final,
                        now_ms,
                    );
                }
            }
            if recognition.is_finished() {
                if !self.asr_stopping && !self.asr.status.starts_with("识别失败") {
                    self.asr.status = "识别线程意外退出；请检查后重启 ASR".into();
                }
                if let Some(capture) = &self.audio_capture {
                    capture.stop();
                    self.audio_stopping = true;
                }
                self.recognition = None;
                self.translation = None;
                self.allow_text_upload = false;
                self.asr_stopping = false;
                changed = true;
            }
        }
        if let Some(translation) = &self.translation {
            let dropped = translation.dropped();
            changed |= self.translation_dropped != dropped;
            self.translation_dropped = dropped;
            for _ in 0..64 {
                let Some(response) = translation.pop() else {
                    break;
                };
                if !self.asr_stopping {
                    if let Err(error) = &response.result {
                        self.translation_status = error.clone();
                    }
                    changed |=
                        self.subtitles
                            .finish_translation(response.key, response.result, now_ms);
                }
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
        if self.running || self.recognition.is_some() {
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
        self.translation = None;
        self.allow_text_upload = false;
        self.translation_status = "文字翻译已停止；文本上传授权已撤销".into();
        if let Some(recognition) = &self.recognition {
            recognition.stop();
            self.asr_stopping = true;
            if let Some(capture) = &self.audio_capture {
                capture.stop();
                self.audio_stopping = true;
            }
        }
        self.subtitles
            .reset(self.subtitles.session_id().saturating_add(1));
        self.message = "字幕会话已停止，字幕已清空。".into();
        cx.notify();
    }

    pub fn audio_active(&self) -> bool {
        self.audio_capture.is_some()
    }

    pub fn toggle_audio(&mut self, cx: &mut Context<Self>) {
        if self.recognition.is_some() {
            self.stop(cx);
            return;
        }
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

    pub fn asr_active(&self) -> bool {
        self.recognition.is_some()
    }

    pub fn select_asr_provider(&mut self, provider: subveil_asr::Provider, cx: &mut Context<Self>) {
        if self.recognition.is_none() {
            self.asr_config.provider = provider;
            if provider == subveil_asr::Provider::Local {
                self.audio.sample_rate = self.asr_config.sample_rate;
                if let Some(language) = &self.local_language {
                    self.asr_config.language.clone_from(language);
                }
            }
            if self.audio_capture.is_none()
                && let Some(rate) = provider.required_rate()
            {
                self.audio.sample_rate = rate;
            }
            cx.notify();
        }
    }

    pub fn cycle_target_language(&mut self, cx: &mut Context<Self>) {
        if self.recognition.is_none() {
            self.asr_config.target_language = match self.asr_config.target_language.as_str() {
                "zh" => "en",
                "en" => "ja",
                _ => "zh",
            }
            .into();
            cx.notify();
        }
    }

    pub fn cycle_asr_language(&mut self, cx: &mut Context<Self>) {
        if self.recognition.is_none() {
            self.asr_config.language = if self.asr_config.language == "en" {
                "ja"
            } else {
                "en"
            }
            .into();
            cx.notify();
        }
    }

    pub fn toggle_asr(&mut self, cx: &mut Context<Self>) {
        if self.recognition.is_some() {
            self.stop(cx);
            return;
        }
        if self.audio_capture.is_some() {
            self.message = "请先停止独立音频采集，等待停止完成，再启动 ASR。".into();
            cx.notify();
            return;
        }
        let mut config = self.asr_config.clone();
        config.sample_rate = if config.provider == subveil_asr::Provider::Local {
            config.sample_rate
        } else {
            config
                .provider
                .required_rate()
                .unwrap_or(self.audio.sample_rate)
        };
        config.allow_cloud = config.provider.is_cloud();
        let result = (|| -> anyhow::Result<_> {
            config.validate()?;
            let id = self.subtitles.session_id().saturating_add(1);
            let translation = if self.translation_provider != subveil_translate::Provider::Disabled
                && !config.provider.translates()
            {
                match subveil_translate::HttpTranslator::from_env(
                    self.translation_provider,
                    self.allow_text_upload,
                )
                .and_then(|backend| {
                    self.translation_status = format!(
                        "文字翻译：{} · {}",
                        self.translation_provider.name(),
                        backend.destination()
                    );
                    subveil_translate::Translation::start(backend, id)
                }) {
                    Ok(worker) => Some(worker),
                    Err(error) => {
                        self.translation_status = format!("文字翻译未启动：{error}；ASR 继续运行");
                        None
                    }
                }
            } else {
                None
            };
            let (capture, frames) = subveil_audio::Capture::start_with_frames(config.sample_rate)?;
            let recognition = subveil_asr::Recognition::start(config, id, frames)?;
            Ok((capture, recognition, translation, id))
        })();
        match result {
            Ok((capture, recognition, translation, id)) => {
                self.translation = translation;
                self.translation_dropped = 0;
                self.running = false;
                self.subtitles.reset(id);
                self.started = Instant::now();
                self.audio_capture = Some(capture);
                self.recognition = Some(recognition);
                self.asr_stopping = false;
                self.audio_stopping = false;
                self.message = if self.asr_config.provider == subveil_asr::Provider::Mock {
                    "Mock ASR 仅按音频能量输出预设样本，不是真实识别。".into()
                } else if self.asr_config.provider == subveil_asr::Provider::Local {
                    "本地 streaming ASR 已启动；后台校验并加载模型，不上传音频。".into()
                } else {
                    format!(
                        "{} 已启动：系统音频发送至 {}；点击停止撤销本次上传。{}",
                        self.asr_config.provider.name(),
                        self.asr_config.provider.destination(),
                        if self.asr_config.provider.translates() {
                            "显示服务自带译文，不播放返回语音。"
                        } else {
                            "此服务仅识别原文。"
                        }
                    )
                };
                self.show_overlay(cx);
            }
            Err(error) => self.message = format!("无法启动 ASR：{error}"),
        }
        cx.notify();
    }

    pub fn cycle_translation(&mut self, cx: &mut Context<Self>) {
        if self.asr_active() {
            return;
        }
        self.translation_provider = match self.translation_provider {
            subveil_translate::Provider::Disabled => subveil_translate::Provider::OpenAi,
            subveil_translate::Provider::OpenAi => subveil_translate::Provider::DeepL,
            subveil_translate::Provider::DeepL => subveil_translate::Provider::Disabled,
        };
        self.allow_text_upload = false;
        self.translation_status = "切换服务后需要重新授权文本上传".into();
        cx.notify();
    }

    pub fn consent_text_upload(&mut self, cx: &mut Context<Self>) {
        if !self.asr_active() {
            self.allow_text_upload = !self.allow_text_upload;
            self.translation_status = if self.allow_text_upload {
                "已授权本次会话；开始识别后才发送文本"
            } else {
                "文本上传授权已撤销"
            }
            .into();
            cx.notify();
        }
    }

    pub fn import_model(&mut self, cx: &mut Context<Self>) {
        if self.asr_active() || self.importing_model {
            return;
        }
        let picker = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("导入 streaming 模型 TOML 清单".into()),
        });
        self.importing_model = true;
        cx.notify();
        cx.spawn(async move |session, cx| {
            let result: anyhow::Result<_> = async {
                let paths = picker.await??;
                let Some(path) = paths.and_then(|paths| paths.into_iter().next()) else { return Ok(None); };
                cx.background_executor().spawn(async move {
                    let metadata = subveil_asr::model_metadata(&path)?;
                    Ok(Some((path, metadata)))
                }).await
            }.await;
            let _ = session.update(cx, |session, cx| {
                session.importing_model = false;
                match result {
                    Ok(Some((path, metadata))) if !session.asr_active() => {
                        session.model_info = format!("{} · {} Hz · {}\n来源：{}\n许可证：{}\n启动后校验文件与 SHA-256 并加载模型", path.display(), metadata.sample_rate, metadata.language, metadata.source, metadata.license);
                        session.asr_config.local_model = Some(path);
                        session.asr_config.sample_rate = metadata.sample_rate;
                        session.local_language = Some(metadata.language.clone());
                        session.asr_config.language = metadata.language;
                        session.audio.sample_rate = metadata.sample_rate;
                    }
                    Ok(_) => {}
                    Err(error) => session.model_info = format!("无法导入模型：{error}"),
                }
                cx.notify();
            });
        }).detach();
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
            if self.running
                || self.overlay_requested
                || self.audio_capture.is_some()
                || self.recognition.is_some()
            {
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
