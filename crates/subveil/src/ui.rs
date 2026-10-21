use gpui_kit::component::{
    ActiveTheme, Disableable, IndexPath, Root, Sizable, Theme, ThemeMode,
    button::{Button, ButtonVariants},
    link::Link,
    radio::RadioGroup,
    scroll::ScrollableElement,
    select::{Select, SelectEvent, SelectState},
    switch::Switch,
    tab::{Tab, TabBar},
};
use gpui_kit::{
    App, AppContext, Bounds, Context, Entity, FocusHandle, IntoElement, KeyBinding, ParentElement,
    Render, ScrollHandle, SharedString, Styled, Subscription, TitlebarOptions, Window,
    WindowBounds, WindowHandle, WindowOptions, div, prelude::*, px, relative, size,
};
use subveil_core::{config::DisplayMode, subtitle::TranslationState};

use crate::{overlay, session::Session};

// Commands never start cloud transmission. Consent remains on the explicit service button.
gpui_kit::actions!(
    subveil_ui,
    [
        Live,
        Services,
        Appearance,
        Diagnostics,
        ToggleOverlay,
        SaveStyle,
        ToggleTheme,
        RestoreControl
    ]
);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-1", Live, Some("Subveil")),
        KeyBinding::new("ctrl-2", Services, Some("Subveil")),
        KeyBinding::new("ctrl-3", Appearance, Some("Subveil")),
        KeyBinding::new("ctrl-4", Diagnostics, Some("Subveil")),
        KeyBinding::new("ctrl-shift-o", ToggleOverlay, Some("Subveil")),
        KeyBinding::new("ctrl-s", SaveStyle, Some("Subveil")),
        KeyBinding::new("ctrl-shift-l", ToggleTheme, Some("Subveil")),
        KeyBinding::new("enter", RestoreControl, Some("SubveilRecovery")),
    ]);
}

fn button(
    id: &'static str,
    label: impl Into<SharedString>,
    session: &Entity<Session>,
    action: impl Fn(&mut Session, &mut Context<Session>) + 'static,
) -> Button {
    let session = session.clone();
    Button::new(id).label(label).on_click(move |_, _, cx| {
        session.update(cx, |session, cx| action(session, cx));
    })
}

fn column() -> gpui_kit::Div {
    div().flex().flex_col().gap_4()
}

fn row() -> gpui_kit::Div {
    div().flex().flex_wrap().items_center().gap_2()
}

fn help(text: impl Into<SharedString>, cx: &App) -> gpui_kit::Div {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

fn heading(title: &'static str, description: &'static str, cx: &App) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xl()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(help(description, cx))
}

fn field(label: &'static str, control: impl IntoElement, cx: &App) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .py_3()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .text_sm()
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .child(label),
        )
        .child(control)
}

pub struct ControlView {
    session: Entity<Session>,
    page: usize,
    focus: FocusHandle,
    scroll: [ScrollHandle; 4],
    provider: Entity<SelectState<Vec<SharedString>>>,
    local_model: Entity<SelectState<Vec<SharedString>>>,
    _subscriptions: Vec<Subscription>,
}

impl ControlView {
    fn new(session: Entity<Session>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let current = session.read(cx).asr_config.provider;
        let provider: Entity<SelectState<Vec<SharedString>>> = cx.new(|cx| {
            SelectState::new(
                subveil_asr::Provider::ALL
                    .iter()
                    .map(|p| SharedString::from(p.name()))
                    .collect(),
                subveil_asr::Provider::ALL
                    .iter()
                    .position(|p| *p == current)
                    .map(IndexPath::new),
                window,
                cx,
            )
        });
        let model_names = session
            .read(cx)
            .local_models
            .catalog
            .iter()
            .map(|model| SharedString::from(model.name.clone()))
            .collect::<Vec<_>>();
        let model_index = (!model_names.is_empty())
            .then(|| IndexPath::new(session.read(cx).local_models.selected));
        let local_model = cx.new(|cx| SelectState::new(model_names, model_index, window, cx));
        let model_session = session.clone();
        let model_selected = cx.subscribe(&local_model, move |_, _, event, cx| {
            if let SelectEvent::Confirm(Some(name)) = event {
                let id = model_session
                    .read(cx)
                    .local_models
                    .catalog
                    .iter()
                    .find(|model| model.name == name.as_ref())
                    .map(|model| model.id.clone());
                if let Some(id) = id {
                    model_session.update(cx, |session, cx| session.select_local_model(&id, cx));
                }
            }
        });
        let observed = cx.observe_in(&session, window, |this, session, window, cx| {
            let value = SharedString::from(session.read(cx).asr_config.provider.name());
            if this.provider.read(cx).selected_value() != Some(&value) {
                this.provider
                    .update(cx, |state, cx| state.set_selected_value(&value, window, cx));
            }
            if let Some(model) = session.read(cx).local_models.current() {
                let value = SharedString::from(model.name.clone());
                if this.local_model.read(cx).selected_value() != Some(&value) {
                    this.local_model
                        .update(cx, |state, cx| state.set_selected_value(&value, window, cx));
                }
            }
            cx.notify();
        });
        let selected_session = session.clone();
        let selected = cx.subscribe(&provider, move |_, _, event, cx| {
            if let SelectEvent::Confirm(Some(name)) = event
                && let Some(provider) = subveil_asr::Provider::ALL
                    .into_iter()
                    .find(|p| p.name() == name.as_ref())
            {
                selected_session.update(cx, |s, cx| s.select_asr_provider(provider, cx));
            }
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            session,
            page: if current == subveil_asr::Provider::Local {
                1
            } else {
                0
            },
            focus,
            scroll: std::array::from_fn(|_| ScrollHandle::new()),
            provider,
            local_model,
            _subscriptions: vec![observed, selected, model_selected],
        }
    }

    fn select_page(&mut self, page: usize, cx: &mut Context<Self>) {
        self.page = page;
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        if !self.session.read(cx).saving {
            self.session.update(cx, Session::save_settings);
        }
    }

    fn toggle_theme(window: &mut Window, cx: &mut App) {
        Theme::change(
            if cx.theme().is_dark() {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            },
            Some(window),
            cx,
        );
    }

    fn render_live(&self, cx: &App) -> gpui_kit::Div {
        let session = self.session.read(cx);
        let mut preview = column()
            .gap_3()
            .min_h_48()
            .p_6()
            .rounded(cx.theme().radius)
            .bg(cx.theme().muted);
        if let Some(subtitle) = session.subtitles.current() {
            preview = preview
                .child(help(
                    if subtitle.is_final {
                        "已确定原文"
                    } else {
                        "正在识别"
                    },
                    cx,
                ))
                .child(div().text_lg().child(subtitle.transcript.text.clone()))
                .child(
                    div().text_xl().child(match &subtitle.translation {
                        TranslationState::Complete(text) | TranslationState::Streaming(text) => {
                            text.clone()
                        }
                        TranslationState::Pending => "正在生成译文…".into(),
                        TranslationState::Failed(error) => format!("翻译失败：{error}"),
                        TranslationState::NotRequested => if subtitle.is_final {
                            "此段暂无服务译文"
                        } else {
                            "等待识别结果…"
                        }
                        .into(),
                    }),
                );
        } else {
            preview = preview
                .justify_center()
                .child(div().text_lg().child("等待第一句字幕"))
                .child(help(
                    "在「语音服务」选择识别服务并开始，或先运行离线演示。",
                    cx,
                ));
        }
        let mut history = column().gap_0();
        for entry in session.subtitles.history().iter().rev().take(8) {
            history = history.child(
                div()
                    .id(("segment", entry.transcript.key.segment_id))
                    .py_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(help(
                        format!(
                            "片段 {} · {}",
                            entry.transcript.key.segment_id,
                            if entry.expired {
                                "已过期"
                            } else if entry.is_final {
                                "已确定"
                            } else {
                                "识别中"
                            }
                        ),
                        cx,
                    ))
                    .child(entry.transcript.text.clone()),
            );
        }
        if session.subtitles.history().is_empty() {
            history = history.child(help("会话开始后，最近的 8 段原文将显示在这里。", cx));
        }
        column()
            .child(heading(
                "实时字幕",
                "查看当前原文与服务译文，控制桌面字幕浮窗。",
                cx,
            ))
            .child(
                row()
                    .child(
                        button(
                            "demo",
                            if session.running {
                                "重新演示"
                            } else {
                                "离线演示"
                            },
                            &self.session,
                            |s, cx| {
                                if s.running {
                                    s.stop(cx);
                                }
                                s.start(cx);
                            },
                        )
                        .disabled(session.asr_active() || session.asr_stopping),
                    )
                    .child(
                        button("stop", "停止并清空", &self.session, Session::stop).disabled(
                            !session.running
                                && !session.asr_active()
                                && session.subtitles.history().is_empty(),
                        ),
                    ),
            )
            .child(preview)
            .child(div().text_sm().child(format!(
                "当前服务：{} · {}",
                session.asr_config.provider.name(),
                session.asr.status
            )))
            .child(div().pt_4().text_lg().child("最近字幕"))
            .child(history)
    }

    fn render_services(&self, cx: &App) -> gpui_kit::Div {
        let session = self.session.read(cx);
        let provider = session.asr_config.provider;
        let busy = session.asr_active()
            || session.asr_stopping
            || session.local_models.downloading()
            || session.translation_models.downloading()
            || session.importing_model
            || session.restoring_local;
        column()
            .child(heading("语音服务", "选择识别服务与文字翻译；本地识别不上传音频。", cx))
            .child(button("offline-mode", if session.offline { "离线模式：开启" } else { "离线模式：关闭" }, &self.session, Session::toggle_offline).disabled(busy))
            .child(help(if session.offline { "仅使用已安装模型；禁止云端识别、文字上传和模型下载。" } else { "本地模型推理不需要联网；安装时需要网络。" }, cx))
            .child(field("识别服务", Select::new(&self.provider).disabled(busy || session.offline)
                .accessibility_label("识别服务").w_full(), cx))
            .child(help(format!("当前识别：{}", provider.name()), cx))
            .child(field("语言", row()
                .child(button("asr-language", format!("源语种：{}", if provider.automatic_language() { "自动识别" } else { &session.asr_config.language }), &self.session, Session::cycle_asr_language)
                    .disabled(busy || provider.automatic_language() || (provider == subveil_asr::Provider::Local && session.local_languages.len() <= 1)))
                .child(button("asr-target", format!("目标语种：{}", session.asr_config.target_language), &self.session, Session::cycle_target_language)
                    .disabled(busy)), cx))
            .child(column().gap_2()
                .child(div().text_base().child(if !provider.is_cloud() { "本地处理 · 不上传音频" } else { "开始后将上传系统音频" }))
                .child(help(format!("音频目标：{}", provider.destination()), cx))
                .child(help(if provider.translates() { "显示服务自带译文；不播放返回语音。" } else { "此服务仅输出原文。" }, cx))
                .child(help(if provider == subveil_asr::Provider::Mock {
                    "Mock ASR 会采集本机系统音频，按音频能量输出预设样本。"
                } else if provider == subveil_asr::Provider::Local { "在下方选择并安装模型，再开始识别。模型推理均在本机执行。" } else { "仅点击下方同意按钮才开始上传。云服务可能收费；请先通过环境变量配置服务凭据。" }, cx)))
            .child(row().child(button("asr-start", if session.asr_stopping { "正在停止…" }
                else if session.asr_active() { "停止识别与音频" }
                else if provider == subveil_asr::Provider::Mock { "开始 Mock ASR" }
                else if provider == subveil_asr::Provider::Local { "开始本地识别" }
                else { "同意上传并开始" }, &self.session, Session::toggle_asr)
                .disabled(session.asr_stopping || session.local_models.downloading() || session.importing_model || (!session.asr_active() && (session.audio_active() || session.audio_stopping || (provider == subveil_asr::Provider::Local && session.asr_config.local_model.is_none()))))
                .loading(session.asr_stopping)))
            .child(help(if session.audio_active() && !session.asr_active() {
                "独立采集正在运行。请在「运行诊断」停止采集，再开始识别。"
            } else { "切换服务与语言前，请先停止识别。" }, cx))
            .child(div().text_sm().child(session.asr.status.clone()))
            .when(provider == subveil_asr::Provider::Local, |view| view.child(self.render_local_models(cx)))
            .child(field("文字翻译", button("translation-provider", format!("翻译服务：{}", session.translation_provider.name()), &self.session, Session::cycle_translation).disabled(busy || provider.translates()), cx))
            .when(session.translation_provider == subveil_translate::Provider::Local, |view| view.child(self.render_local_translation(cx)))
            .child(help(if provider.translates() { "此语音服务自带译文，不再调用独立文字翻译。" } else { "仅翻译已确定的句子。云端文字翻译可能收费，需独立授权上传原文与最多三句上下文。" }, cx))
            .child(help(format!("文本上传目标：{}", session.translation_provider.destination()), cx))
            .child(button("consent-text", if session.allow_text_upload { "撤销文本上传授权" } else { "同意本次会话上传文本" }, &self.session, Session::consent_text_upload)
                .disabled(busy || session.offline || provider.translates() || !matches!(session.translation_provider, subveil_translate::Provider::OpenAi | subveil_translate::Provider::DeepL)))
            .child(help(session.translation_status.clone(), cx))
            .child(help(format!("翻译队列丢弃：{}", session.translation_dropped), cx))
            .child(button("save-local-settings", "保存本地设置", &self.session, Session::save_local_settings).disabled(busy || session.saving_local).loading(session.saving_local))
    }

    fn render_local_translation(&self, cx: &App) -> gpui_kit::Div {
        let session = self.session.read(cx);
        let models = &session.translation_models;
        let busy = session.asr_active() || session.importing_model || models.downloading();
        let compute = match session.compute {
            subveil_translate::Compute::Cpu => "CPU".to_owned(),
            subveil_translate::Compute::Gpu(index) => session
                .gpu_devices
                .iter()
                .find(|d| d.index == index)
                .map(|d| d.name.clone())
                .unwrap_or_else(|| "GPU 不可用".into()),
        };
        let mut view = column()
            .gap_2()
            .child(
                button(
                    "translation-compute",
                    format!("翻译运行设备：{compute}"),
                    &self.session,
                    Session::cycle_compute,
                )
                .disabled(busy || session.gpu_devices.is_empty()),
            )
            .child(help(session.gpu_status.clone(), cx))
            .child(help(
                "语音识别使用 CPU；GPU 加速本地文字翻译。切换设备前请停止识别。",
                cx,
            ));
        if let Some(model) = models.current() {
            view = view
                .child(help(
                    format!(
                        "{} · {:.1} MB · 中 / 英 / 日",
                        model.name,
                        model.size() as f64 / 1_000_000.
                    ),
                    cx,
                ))
                .child(help(model.summary.clone(), cx))
                .child(
                    row()
                        .child(
                            Link::new("translation-source")
                                .href(model.source())
                                .child("模型来源"),
                        )
                        .child(
                            Link::new("translation-license")
                                .href(model.license_url.clone())
                                .child(format!("许可：{}", model.license)),
                        ),
                );
        }
        view.child(
            row()
                .child(
                    button(
                        "install-translation",
                        if session.offline {
                            "校验并选用本机翻译模型"
                        } else {
                            "安装并选用翻译模型"
                        },
                        &self.session,
                        Session::install_translation_model,
                    )
                    .disabled(busy),
                )
                .child(
                    button(
                        "cancel-translation",
                        "取消翻译模型下载",
                        &self.session,
                        Session::cancel_translation_install,
                    )
                    .disabled(!models.downloading()),
                )
                .child(
                    button(
                        "import-translation",
                        "导入翻译清单…",
                        &self.session,
                        Session::import_model,
                    )
                    .disabled(busy),
                ),
        )
        .child(help(
            format!(
                "{} · {:.1} / {:.1} MB",
                models.snapshot.status,
                models.snapshot.completed as f64 / 1_000_000.,
                models.snapshot.total as f64 / 1_000_000.
            ),
            cx,
        ))
        .child(help(
            format!(
                "翻译模型：{}",
                session
                    .translation_model
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "尚未选用".into())
            ),
            cx,
        ))
        .when_some(models.snapshot.available_bytes, |view, bytes| {
            view.child(help(
                format!("模型磁盘可用：{:.2} GiB", bytes as f64 / 1_073_741_824.),
                cx,
            ))
        })
    }

    fn render_local_models(&self, cx: &App) -> gpui_kit::Div {
        let session = self.session.read(cx);
        let models = &session.local_models;
        let installing = models.downloading();
        let busy =
            session.asr_active() || session.asr_stopping || session.importing_model || installing;
        let mut view = column().child(field(
            "下载模型",
            Select::new(&self.local_model)
                .disabled(busy)
                .accessibility_label("本地模型目录")
                .w_full(),
            cx,
        ));
        if let Some(model) = models.current() {
            view = view
                .child(help(
                    format!(
                        "{} · {} · {:.1} MB · CPU",
                        model.kind.name(),
                        model.languages.join(" / "),
                        model.size() as f64 / 1_000_000.
                    ),
                    cx,
                ))
                .child(help(model.summary.clone(), cx))
                .child(help(format!("来源：{}", model.repository), cx))
                .child(
                    row()
                        .child(
                            Link::new("model-source")
                                .href(model.source())
                                .child("模型来源"),
                        )
                        .child(
                            Link::new("model-license")
                                .href(model.license_url.clone())
                                .child(format!("许可：{}", model.license)),
                        ),
                )
                .child(help(
                    format!(
                        "存储目录：{}",
                        models
                            .root
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "不可用".into())
                    ),
                    cx,
                ));
        }
        view
            .child(row()
                .child(button("install-model", "安装并选用", &self.session, Session::install_local_model).disabled(busy || models.current().is_none()))
                .child(button("cancel-model", "取消下载", &self.session, Session::cancel_model_install).disabled(!installing))
                .child(button("import-model", "导入已有模型…", &self.session, Session::import_model).disabled(busy)))
            .child(help("安装会从 Hugging Face 下载模型文件，不上传音频。已下载文件校验后复用；安装不自动开始识别。", cx))
            .child(help(format!("{} · {:.1} / {:.1} MB", models.snapshot.status, models.snapshot.completed as f64 / 1_000_000., models.snapshot.total as f64 / 1_000_000.), cx))
            .child(help(session.model_info.clone(), cx))
            .when_some(models.snapshot.available_bytes, |view, bytes| view.child(help(format!("模型磁盘可用：{:.2} GiB", bytes as f64 / 1_073_741_824.), cx)))
    }

    fn render_appearance(&self, cx: &App) -> gpui_kit::Div {
        let session = self.session.read(cx);
        let settings = &session.settings;
        let mode_session = self.session.clone();
        let switch_session = self.session.clone();
        column()
            .child(heading(
                "字幕样式",
                "修改即时应用到浮窗；点击保存后，下次启动继续使用。",
                cx,
            ))
            .child(
                column()
                    .gap_2()
                    .child(help("样式预览 · 内置样本", cx))
                    .child(overlay::subtitle_panel(
                        settings,
                        "The lights go out, and the race is underway!",
                        "灯光熄灭，比赛开始！",
                        cx,
                    )),
            )
            .child(field(
                "显示内容",
                RadioGroup::horizontal("display-mode")
                    .children(["双语", "仅原文", "仅译文"])
                    .selected_index(Some(match settings.display_mode {
                        DisplayMode::Bilingual => 0,
                        DisplayMode::Source => 1,
                        DisplayMode::Translation => 2,
                    }))
                    .on_change(move |ix, _, cx| {
                        mode_session.update(cx, |s, cx| {
                            s.change_settings(
                                |c| {
                                    c.display_mode = match ix {
                                        1 => DisplayMode::Source,
                                        2 => DisplayMode::Translation,
                                        _ => DisplayMode::Bilingual,
                                    }
                                },
                                cx,
                            )
                        })
                    }),
                cx,
            ))
            .child(field(
                "字体",
                row()
                    .child(button(
                        "font-family",
                        settings.font_family.clone(),
                        &self.session,
                        |s, cx| {
                            s.change_settings(
                                |c| {
                                    c.font_family = if c.font_family == "sans-serif" {
                                        "Noto Sans CJK SC"
                                    } else {
                                        "sans-serif"
                                    }
                                    .into()
                                },
                                cx,
                            )
                        },
                    ))
                    .child(
                        button("font-smaller", "减小字号", &self.session, |s, cx| {
                            s.change_settings(|c| c.font_size = (c.font_size - 2.).max(18.), cx)
                        })
                        .disabled(settings.font_size <= 18.),
                    )
                    .child(format!("{:.0} px", settings.font_size))
                    .child(
                        button("font-larger", "增大字号", &self.session, |s, cx| {
                            s.change_settings(|c| c.font_size = (c.font_size + 2.).min(64.), cx)
                        })
                        .disabled(settings.font_size >= 64.),
                    ),
                cx,
            ))
            .child(field(
                "译文颜色",
                button(
                    "color",
                    if settings.translation_color == 0xffffff {
                        "白色 · 切换为浅黄"
                    } else {
                        "浅黄或自定义 · 切换为白色"
                    },
                    &self.session,
                    |s, cx| {
                        s.change_settings(
                            |c| {
                                c.translation_color = if c.translation_color == 0xffffff {
                                    0xfde68a
                                } else {
                                    0xffffff
                                }
                            },
                            cx,
                        )
                    },
                ),
                cx,
            ))
            .child(field(
                "背景不透明度",
                row()
                    .child(
                        button(
                            "opacity-less",
                            "降低不透明度",
                            &self.session,
                            |s, cx| {
                                s.change_settings(
                                    |c| c.background_opacity = (c.background_opacity - 0.1).max(0.),
                                    cx,
                                )
                            },
                        )
                        .disabled(settings.background_opacity <= 0.),
                    )
                    .child(format!("{:.0}%", settings.background_opacity * 100.))
                    .child(
                        button(
                            "opacity-more",
                            "提高不透明度",
                            &self.session,
                            |s, cx| {
                                s.change_settings(
                                    |c| c.background_opacity = (c.background_opacity + 0.1).min(1.),
                                    cx,
                                )
                            },
                        )
                        .disabled(settings.background_opacity >= 1.),
                    ),
                cx,
            ))
            .child(field(
                "浮窗宽度",
                row()
                    .child(
                        button("narrower", "缩窄", &self.session, |s, cx| {
                            s.change_settings(|c| c.width = (c.width - 80.).max(320.), cx)
                        })
                        .disabled(settings.width <= 320.),
                    )
                    .child(format!("{:.0} px", settings.width))
                    .child(
                        button("wider", "加宽", &self.session, |s, cx| {
                            s.change_settings(|c| c.width = (c.width + 80.).min(2400.), cx)
                        })
                        .disabled(settings.width >= 2400.),
                    ),
                cx,
            ))
            .child(field(
                "底部距离与行数",
                row()
                    .child(
                        button("lower", "向下", &self.session, |s, cx| {
                            s.change_settings(
                                |c| c.bottom_margin = (c.bottom_margin - 16.).max(0.),
                                cx,
                            )
                        })
                        .disabled(settings.bottom_margin <= 0.),
                    )
                    .child(format!("{:.0} px", settings.bottom_margin))
                    .child(
                        button("higher", "向上", &self.session, |s, cx| {
                            s.change_settings(
                                |c| c.bottom_margin = (c.bottom_margin + 16.).min(400.),
                                cx,
                            )
                        })
                        .disabled(settings.bottom_margin >= 400.),
                    )
                    .child(button(
                        "lines",
                        format!("每种语言最多 {} 行 · 切换", settings.max_lines),
                        &self.session,
                        |s, cx| s.change_settings(|c| c.max_lines = c.max_lines % 6 + 1, cx),
                    )),
                cx,
            ))
            .child(
                Switch::new("passthrough")
                    .label("鼠标穿透浮窗")
                    .checked(settings.click_through)
                    .on_change(move |checked, _, cx| {
                        switch_session.update(cx, |s, cx| {
                            s.change_settings(|c| c.click_through = *checked, cx)
                        })
                    }),
            )
            .child(help(
                "浮窗宽度受显示器限制；Layer Shell 与鼠标穿透能力取决于 Wayland compositor。",
                cx,
            ))
            .child(
                button(
                    "save-style",
                    "保存样式",
                    &self.session,
                    Session::save_settings,
                )
                .disabled(session.saving)
                .loading(session.saving),
            )
    }

    fn render_diagnostics(&self, cx: &App) -> gpui_kit::Div {
        let session = self.session.read(cx);
        column()
            .child(heading(
                "运行诊断",
                "检查本机音频输入与会话状态。独立采集不会上传音频。",
                cx,
            ))
            .child(field(
                "系统音频",
                row()
                    .child(
                        button(
                            "audio-toggle",
                            if session.audio_stopping {
                                "正在停止…"
                            } else if session.audio_active() {
                                "停止独立采集"
                            } else {
                                "开始独立采集"
                            },
                            &self.session,
                            Session::toggle_audio,
                        )
                        .disabled(session.asr_active() || session.audio_stopping)
                        .loading(session.audio_stopping),
                    )
                    .child(
                        button(
                            "audio-rate",
                            format!("采样率 {} Hz · 切换", session.audio.sample_rate),
                            &self.session,
                            Session::cycle_audio_rate,
                        )
                        .disabled(
                            session.audio_active()
                                || session.asr_active()
                                || session.audio_stopping,
                        ),
                    ),
                cx,
            ))
            .child(div().text_sm().child(session.audio.status.clone()))
            .child(help(
                format!(
                    "默认输出：{} · 单声道 F32LE",
                    session.audio.device.as_deref().unwrap_or("等待设备")
                ),
                cx,
            ))
            .child(
                div().h_2().w_full().bg(cx.theme().muted).child(
                    div()
                        .h_full()
                        .w(relative(session.audio.peak.clamp(0., 1.)))
                        .bg(cx.theme().primary),
                ),
            )
            .child(help(
                format!(
                    "RMS {:.1} dBFS · 峰值 {:.1} dBFS · {}",
                    20. * session.audio.rms.max(0.000001).log10(),
                    20. * session.audio.peak.max(0.000001).log10(),
                    if session.audio.silent {
                        "静音"
                    } else {
                        "有声"
                    }
                ),
                cx,
            ))
            .child(field(
                "采集统计",
                help(
                    format!(
                        "PCM 块 {} · 样本 {} · 丢块 {} · 丢样本 {} · 无效块 {} · 连接 {}",
                        session.audio.blocks,
                        session.audio.samples,
                        session.audio.dropped_blocks,
                        session.audio.dropped_samples,
                        session.audio.invalid_chunks,
                        session.audio.connections
                    ),
                    cx,
                ),
                cx,
            ))
            .child(field(
                "识别统计",
                help(
                    format!(
                        "识别重置 {} · 输入丢块 {} · 结果丢事件 {}",
                        session.asr.resets, session.asr.dropped_audio, session.asr.dropped_events
                    ),
                    cx,
                ),
                cx,
            ))
            .child(field(
                "会话",
                help(
                    format!(
                        "会话 {} · 历史 {}/{}",
                        session.subtitles.session_id(),
                        session.subtitles.history().len(),
                        session.settings.history_limit
                    ),
                    cx,
                ),
                cx,
            ))
            .child(field(
                "样式文件",
                help(
                    session
                        .config_path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "配置目录不可用".into()),
                    cx,
                ),
                cx,
            ))
            .child(help(
                "关闭控制窗口后，会话继续运行，并保留恢复入口。关闭恢复入口或点击退出可结束程序。",
                cx,
            ))
    }
}

impl Render for ControlView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let session = self.session.read(cx);
        let overlay = session.overlay_visible();
        let saving = session.saving;
        let message = session.message.clone();
        let error = session.config_error.clone();
        let state = if session.asr_stopping || session.audio_stopping {
            "正在停止"
        } else if session.asr_active() {
            "识别中"
        } else if session.local_models.downloading() {
            "模型安装中"
        } else if session.running {
            "离线演示中"
        } else if session.audio_active() {
            "独立采集中"
        } else {
            "待机"
        };
        let content = match self.page {
            1 => self.render_services(cx),
            2 => self.render_appearance(cx),
            3 => self.render_diagnostics(cx),
            _ => self.render_live(cx),
        };
        column()
            .gap_0()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .key_context("Subveil")
            .track_focus(&self.focus)
            .on_action(cx.listener(|s, _: &Live, _, cx| s.select_page(0, cx)))
            .on_action(cx.listener(|s, _: &Services, _, cx| s.select_page(1, cx)))
            .on_action(cx.listener(|s, _: &Appearance, _, cx| s.select_page(2, cx)))
            .on_action(cx.listener(|s, _: &Diagnostics, _, cx| s.select_page(3, cx)))
            .on_action(cx.listener(|s, _: &ToggleOverlay, _, cx| {
                s.session.update(cx, Session::toggle_overlay)
            }))
            .on_action(cx.listener(|s, _: &SaveStyle, _, cx| s.save(cx)))
            .on_action(cx.listener(|_, _: &ToggleTheme, window, cx| Self::toggle_theme(window, cx)))
            .child(
                row()
                    .flex_shrink_0()
                    .px_6()
                    .py_4()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child("Subveil"),
                            )
                            .child(help(state, cx)),
                    )
                    .child(
                        button(
                            "overlay",
                            if overlay {
                                "隐藏浮窗"
                            } else {
                                "显示浮窗"
                            },
                            &self.session,
                            Session::toggle_overlay,
                        )
                        .ghost()
                        .small()
                        .tooltip("Ctrl+Shift+O"),
                    )
                    .child(
                        Button::new("theme")
                            .label(if cx.theme().is_dark() {
                                "浅色外观"
                            } else {
                                "深色外观"
                            })
                            .ghost()
                            .small()
                            .tooltip("Ctrl+Shift+L")
                            .on_click(|_, window, cx| Self::toggle_theme(window, cx)),
                    )
                    .child(
                        button("save", "保存样式", &self.session, Session::save_settings)
                            .small()
                            .disabled(saving)
                            .loading(saving)
                            .tooltip("Ctrl+S"),
                    ),
            )
            .child(
                div()
                    .px_6()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        TabBar::new("workspaces")
                            .segmented()
                            .selected_index(self.page)
                            .child(Tab::new().label("实时字幕"))
                            .child(Tab::new().label("语音服务"))
                            .child(Tab::new().label("字幕样式"))
                            .child(Tab::new().label("运行诊断"))
                            .on_click(cx.listener(|s, ix, _, cx| s.select_page(*ix, cx))),
                    ),
            )
            .child(
                div()
                    .id("workspace-scroll")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .track_scroll(&self.scroll[self.page])
                    .overflow_y_scroll()
                    .vertical_scrollbar(&self.scroll[self.page])
                    .child(content.p_6()),
            )
            .child(
                column()
                    .gap_2()
                    .px_6()
                    .py_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(help(message, cx))
                    .children(
                        error.map(|error| {
                            div().text_sm().text_color(cx.theme().warning).child(error)
                        }),
                    )
                    .child(
                        row()
                            .child(help("Ctrl+1–4 切换工作区", cx))
                            .child(div().flex_1())
                            .child(
                                Button::new("quit")
                                    .label("退出 Subveil")
                                    .ghost()
                                    .small()
                                    .on_click(|_, _, cx| cx.quit()),
                            ),
                    ),
            )
    }
}

struct RecoveryView {
    session: Entity<Session>,
    focus: FocusHandle,
    _subscription: Subscription,
}

impl RecoveryView {
    fn new(session: Entity<Session>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let subscription = cx.observe(&session, |_, _, cx| cx.notify());
        Self {
            session,
            focus,
            _subscription: subscription,
        }
    }
}

impl Render for RecoveryView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        column()
            .size_full()
            .key_context("SubveilRecovery")
            .track_focus(&self.focus)
            .on_action(cx.listener(|s, _: &RestoreControl, _, cx| {
                s.session.update(cx, Session::restore_control)
            }))
            .p_6()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(heading(
                "Subveil 会话已保留",
                "恢复控制窗口以查看字幕、服务与样式。",
                cx,
            ))
            .child(help(
                if self.session.read(cx).asr_active() {
                    "语音识别仍在运行"
                } else if self.session.read(cx).local_models.downloading() {
                    "本地模型仍在安装；恢复控制窗口可查看进度或取消"
                } else if self.session.read(cx).running {
                    "离线演示仍在运行"
                } else if self.session.read(cx).audio_active() {
                    "独立采集仍在运行"
                } else {
                    "会话待机，字幕状态已保留"
                },
                cx,
            ))
            .child(
                row()
                    .child(button(
                        "restore",
                        "恢复控制窗口…",
                        &self.session,
                        Session::restore_control,
                    ))
                    .child(
                        Button::new("quit-session")
                            .label("退出 Subveil")
                            .ghost()
                            .on_click(|_, _, cx| cx.quit()),
                    ),
            )
    }
}

fn options(title: &str, width: f32, height: f32, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(width), px(height)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(title.to_owned().into()),
            ..Default::default()
        }),
        app_id: Some("subveil".into()),
        window_min_size: Some(size(px(width.min(760.)), px(height.min(560.)))),
        ..Default::default()
    }
}

pub fn open_control(session: Entity<Session>, cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    cx.open_window(options("Subveil", 1040., 780., cx), |window, cx| {
        let view = cx.new(|cx| ControlView::new(session, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    })
}

pub fn open_recovery(session: Entity<Session>, cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    cx.open_window(
        options("Subveil · 会话", 440., 240., cx),
        |window, cx| {
            let view = cx.new(|cx| RecoveryView::new(session, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        },
    )
}
