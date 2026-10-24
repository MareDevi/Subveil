mod appearance;
mod diagnostics;
mod live;
mod model_library;
mod service_fields;
mod services;
mod shell;
#[cfg(test)]
mod tests;
mod theme;

use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, IndexPath, Root, Selectable, Sizable,
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
    tab::{Tab, TabBar},
};
use gpui_kit::{
    App, AppContext, Bounds, Context, Entity, FocusHandle, IntoElement, KeyBinding, ParentElement,
    Render, ScrollHandle, SharedString, Styled, Subscription, TitlebarOptions, Window,
    WindowBounds, WindowHandle, WindowOptions, div, prelude::*, px, relative, size,
};
use subveil_core::{
    config::{DisplayMode, InterfaceTheme},
    subtitle::TranslationState,
};

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

pub(crate) fn apply_theme(scheme: InterfaceTheme, cx: &mut App) {
    theme::apply(scheme, cx);
}

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
    Button::new(id)
        .small()
        .label(label)
        .on_click(move |_, _, cx| {
            session.update(cx, |session, cx| action(session, cx));
        })
}

fn column() -> gpui_kit::Div {
    div().flex().flex_col().gap_3()
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
                .text_lg()
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
        .py_3()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .w_32()
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
        .pt_5()
        .pb_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .text_base()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(help(description, cx))
}

pub struct ControlView {
    history_search: Entity<InputState>,
    model_search: Entity<InputState>,
    download_origin: Entity<InputState>,
    download_origin_applied: String,
    local_model_items: Vec<SharedString>,
    profile_name: Entity<InputState>,
    choices: Vec<ChoiceView>,
    audio_source: Entity<SelectState<Vec<SharedString>>>,
    audio_source_items: Vec<SharedString>,
    session: Entity<Session>,
    page: usize,
    model_tab: usize,
    service_tab: usize,
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
                    .small()
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
        let model_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索模型名称、用途或发布者"));
        let search_owner = session.clone();
        let model_search_subscription = cx.subscribe(&model_search, move |_, input, event, cx| {
            if matches!(event, InputEvent::Change) {
                let query: String = input.read(cx).value().chars().take(256).collect();
                search_owner.update(cx, |s, cx| {
                    s.model_filter.query = query;
                    cx.notify();
                });
            }
        });
        let origin = session.read(cx).local_models.source.origin().to_string();
        let download_origin_applied = origin.clone();
        let download_origin = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://huggingface.co")
                .default_value(origin)
        });
        let history_search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索原文或译文"));
        let search_subscription = cx.observe(&history_search, |_, _, cx| cx.notify());
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
        let model_names: Vec<_> = session
            .read(cx)
            .model_filter
            .names(&session.read(cx).local_models)
            .into_iter()
            .map(SharedString::from)
            .collect();
        let model_index = session
            .read(cx)
            .local_models
            .current()
            .and_then(|model| {
                model_names
                    .iter()
                    .position(|name| name.as_ref() == model.name)
            })
            .map(IndexPath::new);
        let local_model_items = model_names.clone();
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
            let origin = session.read(cx).local_models.source.origin().to_string();
            if origin != this.download_origin_applied {
                this.download_origin_applied = origin.clone();
                this.download_origin
                    .update(cx, |s, cx| s.set_value(origin, window, cx));
            }
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
            let items: Vec<_> = session
                .read(cx)
                .model_filter
                .names(&session.read(cx).local_models)
                .into_iter()
                .map(SharedString::from)
                .collect();
            if items != this.local_model_items {
                this.local_model_items = items.clone();
                this.local_model
                    .update(cx, |s, cx| s.set_items(items, window, cx));
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
        input_subscriptions.push(search_subscription);
        input_subscriptions.push(model_search_subscription);
        let credentials_generation = session.read(cx).credentials_generation;
        Self {
            model_search,
            download_origin,
            download_origin_applied,
            local_model_items,
            history_search,
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
            model_tab: 0,
            service_tab: 0,
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

    fn choose_theme(&mut self, scheme: InterfaceTheme, cx: &mut Context<Self>) {
        theme::apply(scheme, cx);
        self.session.update(cx, |s, cx| {
            s.set_interface_theme(scheme, cx);
        });
    }

    fn toggle_theme(&mut self, cx: &mut Context<Self>) {
        let scheme = self.session.read(cx).settings.interface_theme.opposite();
        self.choose_theme(scheme, cx);
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
