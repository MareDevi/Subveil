mod appearance;
mod diagnostics;
mod live;
mod model_library;
mod service_fields;
mod services;
#[cfg(test)]
mod tests;

use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, IndexPath, Root, Sizable, Theme, ThemeMode,
    button::{Button, ButtonVariants},
    input::{Input, InputContentType, InputEvent, InputState},
    link::Link,
    progress::Progress,
    radio::RadioGroup,
    scroll::ScrollableElement,
    select::{Select, SelectEvent, SelectState},
    sidebar::{Sidebar, SidebarMenu, SidebarMenuItem},
    status_bar::StatusBar,
    switch::Switch,
};
use gpui_kit::{
    App, AppContext, Bounds, Context, Entity, FocusHandle, IntoElement, KeyBinding, ParentElement,
    Render, ScrollHandle, SharedString, Styled, Subscription, TitlebarOptions, Window,
    WindowBounds, WindowHandle, WindowOptions, div, prelude::*, px, relative, size,
};
use subveil_core::{config::DisplayMode, subtitle::TranslationState};

use crate::session::Choice;
use crate::{overlay, session::Session};
struct ChoiceView {
    kind: Choice,
    items: Vec<SharedString>,
    state: Entity<SelectState<Vec<SharedString>>>,
}

// Commands never start cloud transmission. Consent remains on the explicit service button.
gpui_kit::actions!(
    subveil_ui,
    [
        Live,
        Services,
        Appearance,
        Diagnostics,
        Models,
        ToggleSidebar,
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
        KeyBinding::new("ctrl-5", Models, Some("Subveil")),
        KeyBinding::new("ctrl-b", ToggleSidebar, Some("Subveil")),
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
        .items_start()
        .gap_4()
        .py_4()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .w_24()
                .flex_shrink_0()
                .pt_1()
                .text_sm()
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(control))
}

fn section(title: &'static str, description: &'static str, cx: &App) -> gpui_kit::Div {
    column()
        .gap_1()
        .pt_4()
        .child(
            div()
                .text_base()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(help(description, cx))
}

pub struct ControlView {
    profile_name: Entity<InputState>,
    choices: Vec<ChoiceView>,
    audio_source: Entity<SelectState<Vec<SharedString>>>,
    audio_source_items: Vec<SharedString>,
    session: Entity<Session>,
    page: usize,
    focus: FocusHandle,
    scroll: [ScrollHandle; 5],
    sidebar_collapsed: bool,
    provider: Entity<SelectState<Vec<SharedString>>>,
    local_model: Entity<SelectState<Vec<SharedString>>>,
    cloud_model: Entity<SelectState<Vec<SharedString>>>,
    cloud_items: Vec<SharedString>,
    cloud_text_model: Entity<SelectState<Vec<SharedString>>>,
    cloud_text_items: Vec<SharedString>,
    service_inputs: Vec<(&'static str, Entity<InputState>)>,
    credentials_generation: u64,
    _subscriptions: Vec<Subscription>,
}

impl ControlView {
    fn render_choice(
        &self,
        kind: Choice,
        label: &'static str,
        disabled: bool,
        cx: &App,
    ) -> gpui_kit::Div {
        if let Some(choice) = self.choices.iter().find(|c| c.kind == kind) {
            field(
                label,
                Select::new(&choice.state)
                    .w_full()
                    .accessibility_label(label)
                    .disabled(disabled || choice.items.is_empty()),
                cx,
            )
        } else {
            help("选择器暂不可用", cx)
        }
    }
    fn new(session: Entity<Session>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let profile_name =
            cx.new(|cx| InputState::new(window, cx).placeholder("为当前服务配置命名"));
        let current = session.read(cx).asr_config.provider;
        let mut choices = Vec::new();
        let mut choice_subscriptions = Vec::new();
        for kind in Choice::ALL {
            let items: Vec<_> = session
                .read(cx)
                .choice_options(kind)
                .into_iter()
                .map(SharedString::from)
                .collect();
            let selected = SharedString::from(session.read(cx).choice_value(kind));
            let index = items
                .iter()
                .position(|v| *v == selected)
                .map(IndexPath::new);
            let state = cx.new(|cx| SelectState::new(items.clone(), index, window, cx));
            let owner = session.clone();
            choice_subscriptions.push(cx.subscribe(&state, move |_, _, event, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    owner.update(cx, |s, cx| s.select_choice(kind, value.as_ref(), cx));
                }
            }));
            choices.push(ChoiceView { kind, items, state });
        }
        let audio_source_items: Vec<_> = session
            .read(cx)
            .source_labels()
            .into_iter()
            .map(SharedString::from)
            .collect();
        let audio_source = cx.new(|cx| {
            SelectState::new(
                audio_source_items.clone(),
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        });
        let source_owner = session.clone();
        let source_selected = cx.subscribe(&audio_source, move |_, _, event, cx| {
            if let SelectEvent::Confirm(Some(label)) = event {
                source_owner.update(cx, |s, cx| s.select_audio_source(label.as_ref(), cx));
            }
        });
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
        let mut service_inputs = Vec::new();
        let mut input_subscriptions = Vec::new();
        let schemas = subveil_asr::Provider::ALL
            .into_iter()
            .flat_map(service_fields::fields)
            .chain(
                [
                    subveil_translate::Provider::Local,
                    subveil_translate::Provider::OpenAi,
                    subveil_translate::Provider::DeepL,
                ]
                .into_iter()
                .flat_map(service_fields::translation_fields),
            );
        for spec in schemas {
            let key = spec.key;
            if service_inputs.iter().any(|(existing, _)| *existing == key) {
                continue;
            }
            let value = session
                .read(cx)
                .asr_config
                .services
                .as_ref()
                .and_then(|s| s.optional(key))
                .unwrap_or_default();
            let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
            let owner = session.clone();
            input_subscriptions.push(cx.subscribe(&input, move |_, state, event, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = state.read(cx).value().to_string();
                    owner.update(cx, |session, cx| {
                        if session.asr_active() || session.credentials_busy || session.cloud_busy {
                            return;
                        }
                        session
                            .asr_config
                            .services
                            .get_or_insert_default()
                            .set(key, value);
                        session.allow_text_upload = false;
                        session.cloud_models =
                            subveil_asr::discovery::defaults(session.asr_config.provider);
                        if matches!(key, "SUBVEIL_TRANSLATE_URL" | "SUBVEIL_TRANSLATE_KEY") {
                            session.cloud_text_models.clear();
                        }
                        session.cloud_status = "官方协议默认项；凭据变更后请重新查询".into();
                        session.credentials_status =
                            "配置已修改，仅保留在内存；上传仍需单独授权".into();
                        cx.notify();
                    });
                }
            }));
            service_inputs.push((key, input));
        }
        let cloud_items = session
            .read(cx)
            .cloud_models
            .iter()
            .cloned()
            .map(SharedString::from)
            .collect::<Vec<_>>();
        let cloud_model = cx.new(|cx| SelectState::new(cloud_items.clone(), None, window, cx));
        let cloud_owner = session.clone();
        let cloud_selected = cx.subscribe(&cloud_model, move |_, _, event, cx| {
            if let SelectEvent::Confirm(Some(model)) = event {
                cloud_owner.update(cx, |session, cx| {
                    if !session.asr_active()
                        && !session.cloud_busy
                        && session.cloud_models.iter().any(|id| id == model.as_ref())
                    {
                        let key = format!("{}_MODEL", session.asr_config.provider.id());
                        session
                            .asr_config
                            .services
                            .get_or_insert_default()
                            .set(&key, model.to_string());
                        cx.notify();
                    }
                });
            }
        });
        let cloud_text_items = session
            .read(cx)
            .cloud_text_models
            .iter()
            .cloned()
            .map(SharedString::from)
            .collect::<Vec<_>>();
        let cloud_text_model =
            cx.new(|cx| SelectState::new(cloud_text_items.clone(), None, window, cx));
        let text_owner = session.clone();
        let text_selected = cx.subscribe_in(
            &cloud_text_model,
            window,
            move |this, _, event, window, cx| {
                if let SelectEvent::Confirm(Some(model)) = event {
                    if text_owner.read(cx).asr_active() {
                        return;
                    }
                    if let Some((_, input)) = this
                        .service_inputs
                        .iter()
                        .find(|(key, _)| *key == "SUBVEIL_TRANSLATE_MODEL")
                    {
                        input.update(cx, |s, cx| s.set_value(model.to_string(), window, cx));
                    }
                }
            },
        );
        let observed = cx.observe_in(&session, window, |this, session, window, cx| {
            for choice in &mut this.choices {
                let items: Vec<_> = session
                    .read(cx)
                    .choice_options(choice.kind)
                    .into_iter()
                    .map(SharedString::from)
                    .collect();
                if items != choice.items {
                    choice.items = items.clone();
                    choice
                        .state
                        .update(cx, |s, cx| s.set_items(items, window, cx));
                }
                let value = SharedString::from(session.read(cx).choice_value(choice.kind));
                if choice.state.read(cx).selected_value() != Some(&value) {
                    choice
                        .state
                        .update(cx, |s, cx| s.set_selected_value(&value, window, cx));
                }
            }
            let sources: Vec<_> = session
                .read(cx)
                .source_labels()
                .into_iter()
                .map(SharedString::from)
                .collect();
            if sources != this.audio_source_items {
                this.audio_source_items = sources.clone();
                this.audio_source
                    .update(cx, |s, cx| s.set_items(sources, window, cx));
            }
            let selected = SharedString::from(session.read(cx).selected_source_label());
            if this.audio_source.read(cx).selected_value() != Some(&selected) {
                this.audio_source
                    .update(cx, |s, cx| s.set_selected_value(&selected, window, cx));
            }
            let text_items = session
                .read(cx)
                .cloud_text_models
                .iter()
                .cloned()
                .map(SharedString::from)
                .collect::<Vec<_>>();
            if text_items != this.cloud_text_items {
                this.cloud_text_items = text_items.clone();
                this.cloud_text_model
                    .update(cx, |s, cx| s.set_items(text_items, window, cx));
            }

            let generation = session.read(cx).credentials_generation;
            let config = session.read(cx).asr_config.clone();
            let items = session
                .read(cx)
                .cloud_models
                .iter()
                .cloned()
                .map(SharedString::from)
                .collect::<Vec<_>>();
            if items != this.cloud_items {
                this.cloud_items = items.clone();
                this.cloud_model
                    .update(cx, |s, cx| s.set_items(items, window, cx));
            }
            let selected = SharedString::from(
                config.model(
                    this.cloud_items
                        .first()
                        .map(|s| s.as_ref())
                        .unwrap_or_default(),
                ),
            );
            if this.cloud_model.read(cx).selected_value() != Some(&selected) {
                this.cloud_model
                    .update(cx, |s, cx| s.set_selected_value(&selected, window, cx));
            }
            if generation != this.credentials_generation {
                this.credentials_generation = generation;
                for (key, input) in &this.service_inputs {
                    let value = config
                        .services
                        .as_ref()
                        .and_then(|s| s.optional(key))
                        .unwrap_or_default();
                    input.update(cx, |s, cx| s.set_value(value, window, cx));
                }
            }

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
        input_subscriptions.extend([
            source_selected,
            observed,
            selected,
            model_selected,
            cloud_selected,
            text_selected,
        ]);
        input_subscriptions.extend(choice_subscriptions);
        let credentials_generation = session.read(cx).credentials_generation;
        Self {
            profile_name,
            choices,
            audio_source,
            audio_source_items,
            session,
            page: if current == subveil_asr::Provider::Local {
                4
            } else {
                0
            },
            focus,
            sidebar_collapsed: false,
            scroll: std::array::from_fn(|_| ScrollHandle::new()),
            provider,
            local_model,
            cloud_model,
            cloud_items,
            cloud_text_model,
            cloud_text_items,
            service_inputs,
            credentials_generation,
            _subscriptions: input_subscriptions,
        }
    }

    fn select_page(&mut self, page: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.page = page;
        window.focus(&self.focus, cx);
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
}

impl ControlView {
    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        cx.notify();
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let menu = SidebarMenu::new().children(
            [
                (0, "实时字幕", IconName::FileText),
                (1, "语音服务", IconName::Mic),
                (4, "模型库", IconName::HardDrive),
                (2, "字幕样式", IconName::Palette),
                (3, "运行诊断", IconName::SquareTerminal),
            ]
            .into_iter()
            .map(|(page, label, icon)| {
                SidebarMenuItem::new(label)
                    .icon(icon)
                    .active(self.page == page)
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.select_page(page, window, cx)),
                    )
            }),
        );
        Sidebar::new("navigation")
            .w_48()
            .collapsed(self.sidebar_collapsed)
            .header(
                row()
                    .h_12()
                    .px_3()
                    .flex_nowrap()
                    .child(Icon::new(IconName::GalleryVerticalEnd).small())
                    .when(!self.sidebar_collapsed, |view| {
                        view.child(
                            div()
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .child("Subveil"),
                        )
                    }),
            )
            .child(menu)
            .footer(
                column()
                    .gap_1()
                    .p_2()
                    .when(!self.sidebar_collapsed, |view| {
                        view.child(help("实时字幕与翻译", cx))
                    })
                    .child(
                        Button::new("quit")
                            .icon(IconName::WindowClose)
                            .when(!self.sidebar_collapsed, |button| {
                                button.label("退出 Subveil")
                            })
                            .accessibility_label("退出 Subveil")
                            .ghost()
                            .small()
                            .on_click(|_, _, cx| cx.quit()),
                    ),
            )
    }
}

impl Render for ControlView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let session = self.session.read(cx);
        let overlay = session.overlay_visible();
        let message = session.message.clone();
        let error = session.config_error.clone();
        let state = if session.asr_stopping || session.audio_stopping {
            "正在停止"
        } else if session.asr_active() {
            "识别中"
        } else if session.local_models.downloading() || session.translation_models.downloading() {
            "模型安装中"
        } else if session.running {
            "离线演示中"
        } else if session.audio_active() {
            "独立采集中"
        } else {
            "待机"
        };
        let network = if session.offline {
            "离线模式"
        } else if session.asr_active() && session.asr_config.provider.is_cloud() {
            "云端音频传输中"
        } else {
            "上传需授权"
        };
        let title = ["实时字幕", "语音服务", "字幕样式", "运行诊断", "模型库"][self.page];
        let content = match self.page {
            1 => self.render_services(cx),
            2 => self.render_appearance(cx),
            3 => self.render_diagnostics(cx),
            4 => self.render_models(cx),
            _ => self.render_live(cx),
        };
        column()
            .gap_0()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .key_context("Subveil")
            .track_focus(&self.focus)
            .on_action(cx.listener(|s, _: &Live, window, cx| s.select_page(0, window, cx)))
            .on_action(cx.listener(|s, _: &Services, window, cx| s.select_page(1, window, cx)))
            .on_action(cx.listener(|s, _: &Appearance, window, cx| s.select_page(2, window, cx)))
            .on_action(cx.listener(|s, _: &Diagnostics, window, cx| s.select_page(3, window, cx)))
            .on_action(cx.listener(|s, _: &Models, window, cx| s.select_page(4, window, cx)))
            .on_action(cx.listener(|s, _: &ToggleSidebar, _, cx| s.toggle_sidebar(cx)))
            .on_action(cx.listener(|s, _: &ToggleOverlay, _, cx| {
                s.session.update(cx, Session::toggle_overlay)
            }))
            .on_action(cx.listener(|s, _: &SaveStyle, _, cx| s.save(cx)))
            .on_action(cx.listener(|_, _: &ToggleTheme, window, cx| Self::toggle_theme(window, cx)))
            .child(
                div()
                    .flex()
                    .items_stretch()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_sidebar(cx))
                    .child(
                        column()
                            .gap_0()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(
                                row()
                                    .flex_nowrap()
                                    .flex_shrink_0()
                                    .h_12()
                                    .px_4()
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        Button::new("toggle-sidebar")
                                            .icon(IconName::PanelLeft)
                                            .accessibility_label("折叠或展开侧栏")
                                            .ghost()
                                            .small()
                                            .tooltip("Ctrl+B")
                                            .on_click(
                                                cx.listener(|s, _, _, cx| s.toggle_sidebar(cx)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_sm()
                                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                                            .child(title),
                                    )
                                    .child(
                                        button(
                                            "overlay",
                                            if overlay {
                                                "隐藏浮窗"
                                            } else {
                                                "显示浮窗…"
                                            },
                                            &self.session,
                                            Session::toggle_overlay,
                                        )
                                        .icon(if overlay {
                                            IconName::EyeOff
                                        } else {
                                            IconName::Eye
                                        })
                                        .ghost()
                                        .small()
                                        .tooltip("Ctrl+Shift+O"),
                                    )
                                    .child(
                                        Button::new("theme")
                                            .icon(if cx.theme().is_dark() {
                                                IconName::Sun
                                            } else {
                                                IconName::Moon
                                            })
                                            .accessibility_label("切换明暗外观")
                                            .ghost()
                                            .small()
                                            .tooltip("切换明暗外观 · Ctrl+Shift+L")
                                            .on_click(|_, window, cx| {
                                                Self::toggle_theme(window, cx)
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .id(("workspace-scroll", self.page))
                                    .flex_1()
                                    .min_h_0()
                                    .min_w_0()
                                    .track_scroll(&self.scroll[self.page])
                                    .overflow_y_scroll()
                                    .vertical_scrollbar(&self.scroll[self.page])
                                    .child(
                                        div()
                                            .w_full()
                                            .max_w(gpui_kit::rems(64.))
                                            .mx_auto()
                                            .p_6()
                                            .child(content),
                                    ),
                            )
                            .child(
                                column()
                                    .gap_1()
                                    .px_6()
                                    .py_2()
                                    .border_t_1()
                                    .border_color(cx.theme().border)
                                    .child(help(message, cx))
                                    .children(error.map(|error| {
                                        div().text_sm().text_color(cx.theme().warning).child(error)
                                    })),
                            ),
                    ),
            )
            .child(
                StatusBar::new()
                    .flex_shrink_0()
                    .px_3()
                    .left(
                        row()
                            .flex_nowrap()
                            .text_xs()
                            .child(Icon::new(IconName::CircleCheck).xsmall())
                            .child(state),
                    )
                    .child(network)
                    .right("Ctrl+1–5 导航"),
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
            .justify_center()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(Icon::new(IconName::GalleryVerticalEnd).text_color(cx.theme().muted_foreground))
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
    // Native window geometry is a platform boundary, independent of interface rem.
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
    cx.open_window(options("Subveil", 1120., 800., cx), |window, cx| {
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
