use gpui_kit::{
    App, Context, Entity, IntoElement, Render, SharedString, Window, WindowHandle, div, prelude::*,
    px, rgb,
};
use subveil_core::{
    config::{DisplayMode, OverlaySettings},
    subtitle::TranslationState,
};

use crate::session::Session;

pub struct OverlayView {
    session: Entity<Session>,
}

impl OverlayView {
    fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        cx.observe(&session, |_, _, cx| cx.notify()).detach();
        Self { session }
    }
}

impl Render for OverlayView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let session = self.session.read(cx);
        let settings = &session.settings;
        window.set_input_region(if settings.click_through {
            Some(&[])
        } else {
            None
        });
        let mut root = div().size_full().flex().flex_col().justify_end();
        for subtitle in session.subtitles.visible() {
            let translation = match &subtitle.translation {
                TranslationState::Complete(text) => text.clone(),
                TranslationState::Streaming(text) | TranslationState::PendingWithPreview(text) => {
                    format!("{text}（临时译文）")
                }
                TranslationState::Pending => "正在生成译文…".into(),
                TranslationState::Failed(_) => "翻译失败".into(),
                TranslationState::NotRequested => if subtitle.is_final {
                    ""
                } else {
                    "正在识别…"
                }
                .into(),
            };
            root = root.child(subtitle_panel(
                settings,
                &subtitle.transcript.text,
                &translation,
                cx,
            ));
        }
        root
    }
}

/// The overlay and style preview share user-owned text colors and logical-pixel typography.
/// These are subtitle content settings, deliberately independent of application theme/zoom.
pub(crate) fn subtitle_panel(
    settings: &OverlaySettings,
    source: &str,
    translation: &str,
    cx: &App,
) -> gpui_kit::Div {
    use gpui_kit::component::ActiveTheme;

    let mut background = gpui_kit::Hsla::from(rgb(settings.background_color));
    background.a = settings.background_opacity;
    let mut panel = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .p_4()
        .rounded(cx.theme().radius)
        .bg(background)
        .font_family(SharedString::from(settings.font_family.clone()))
        .text_center()
        .overflow_hidden();
    if settings.display_mode != DisplayMode::Translation {
        panel = panel.child(
            div()
                .text_size(px(settings.font_size * settings.source_ratio))
                .line_height(gpui_kit::relative(1.4))
                .text_color(rgb(settings.source_color))
                .line_clamp(settings.max_lines)
                .text_ellipsis()
                .child(source.to_owned()),
        );
    }
    if settings.display_mode != DisplayMode::Source {
        panel = panel.child(
            div()
                .text_size(px(settings.font_size))
                .line_height(gpui_kit::relative(1.4))
                .text_color(rgb(settings.translation_color))
                .line_clamp(settings.max_lines)
                .text_ellipsis()
                .child(translation.to_owned()),
        );
    }
    panel
}

#[cfg(target_os = "linux")]
pub fn open(
    session: Entity<Session>,
    settings: &OverlaySettings,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<OverlayView>> {
    use anyhow::{Context as _, ensure};
    use gpui_kit::{
        AppContext, Bounds, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
        layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions},
        point, size,
    };

    ensure!(
        cx.graphical_environment()
            .is_some_and(|environment| environment.wayland_display.is_some()),
        "字幕浮窗需要原生 Wayland 会话和 zwlr_layer_shell_v1 协议；当前后端不支持"
    );
    let displays = cx.displays();
    let display = if let Some(uuid) = &settings.output_uuid {
        let matches: Vec<_> = displays
            .into_iter()
            .filter(|d| d.uuid().is_ok_and(|id| id.to_string() == *uuid))
            .collect();
        ensure!(
            matches.len() == 1,
            "所选屏幕已断开或标识不唯一；浮窗已暂停，请重新选择屏幕"
        );
        matches.into_iter().next().context("所选屏幕不可用")?
    } else {
        displays
            .into_iter()
            .min_by(|left, right| {
                let left = left.bounds().origin;
                let right = right.bounds().origin;
                f32::from(left.x)
                    .total_cmp(&f32::from(right.x))
                    .then_with(|| f32::from(left.y).total_cmp(&f32::from(right.y)))
            })
            .context("没有可用屏幕；浮窗已暂停")?
    };
    let output_uuid = display.uuid().ok();
    tracing::debug!(?output_uuid, "opening subtitle overlay on selected output");
    let bounds = display.bounds();
    let width = settings
        .width
        .min(f32::from(bounds.size.width) - 32.)
        .max(1.);
    let margin = settings
        .bottom_margin
        .min((f32::from(bounds.size.height) - 80.).max(0.));
    let text_height = settings.font_size
        * settings.max_lines as f32
        * 1.4
        * match settings.display_mode {
            DisplayMode::Bilingual => 1. + settings.source_ratio,
            DisplayMode::Source => settings.source_ratio,
            DisplayMode::Translation => 1.,
        };
    let height = ((text_height + 48.) * 2.)
        .min(f32::from(bounds.size.height) - margin - 16.)
        .max(1.);
    cx.open_window(
        gpui_kit::WindowOptions {
            titlebar: None,
            focus: false,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(width), px(height)),
            })),
            window_background: WindowBackgroundAppearance::Transparent,
            display_id: Some(display.id()),
            app_id: Some("subveil-overlay".into()),
            kind: WindowKind::LayerShell(LayerShellOptions {
                namespace: "subveil-subtitles".into(),
                layer: Layer::Overlay,
                anchor: Anchor::BOTTOM,
                exclusive_zone: Some(px(-1.)),
                margin: Some((px(0.), px(0.), px(margin), px(0.))),
                keyboard_interactivity: KeyboardInteractivity::None,
                ..Default::default()
            }),
            ..WindowOptions::default()
        },
        |window, cx| {
            window.set_input_region(if settings.click_through {
                Some(&[])
            } else {
                None
            });
            cx.new(|cx| OverlayView::new(session, cx))
        },
    )
    .context("compositor could not create the Wayland subtitle layer")
}

#[cfg(not(target_os = "linux"))]
pub fn open(
    _: Entity<Session>,
    _: &OverlaySettings,
    _: &mut App,
) -> anyhow::Result<WindowHandle<OverlayView>> {
    anyhow::bail!("M01 字幕浮窗仅支持 Linux Wayland；当前平台尚未实现")
}
