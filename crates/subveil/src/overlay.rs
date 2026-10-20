use gpui::{
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
        let Some(subtitle) = session.subtitles.current() else {
            return root;
        };
        let mut background = gpui::Hsla::from(rgb(settings.background_color));
        background.a = settings.background_opacity;
        let mut panel = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .rounded_lg()
            .bg(background)
            .font_family(SharedString::from(settings.font_family.clone()))
            .text_center()
            .overflow_hidden();
        if settings.display_mode != DisplayMode::Translation {
            panel = panel.child(
                div()
                    .text_size(px(settings.font_size * settings.source_ratio))
                    .line_height(gpui::relative(1.4))
                    .text_color(rgb(settings.source_color))
                    .line_clamp(settings.max_lines)
                    .child(subtitle.transcript.text.clone()),
            );
        }
        if settings.display_mode != DisplayMode::Source {
            let translation = match &subtitle.translation {
                TranslationState::Complete(text) => text.as_str(),
                TranslationState::Pending => "正在生成模拟译文…",
                TranslationState::Failed(_) => "模拟翻译失败",
                TranslationState::NotRequested => {
                    if subtitle.is_final {
                        ""
                    } else {
                        "正在识别（模拟）…"
                    }
                }
            };
            panel = panel.child(
                div()
                    .text_size(px(settings.font_size))
                    .line_height(gpui::relative(1.4))
                    .text_color(rgb(settings.translation_color))
                    .line_clamp(settings.max_lines)
                    .child(translation.to_owned()),
            );
        }
        root = root.child(panel);
        root
    }
}

#[cfg(target_os = "linux")]
pub fn open(
    session: Entity<Session>,
    settings: &OverlaySettings,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<OverlayView>> {
    use anyhow::{Context as _, ensure};
    use gpui::{
        AppContext, Bounds, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
        layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions},
        point, size,
    };

    ensure!(
        cx.graphical_environment()
            .is_some_and(|environment| environment.wayland_display.is_some()),
        "字幕浮窗需要原生 Wayland 会话和 zwlr_layer_shell_v1 协议；当前后端不支持"
    );
    // Wayland has no primary-output concept; this GPUI backend always returns None.
    // Use the first output in layout order until explicit monitor selection arrives in M06.
    let display = cx
        .displays()
        .into_iter()
        .min_by(|left, right| {
            let left = left.bounds().origin;
            let right = right.bounds().origin;
            f32::from(left.x)
                .total_cmp(&f32::from(right.x))
                .then_with(|| f32::from(left.y).total_cmp(&f32::from(right.y)))
        })
        .context("no display is available for the subtitle overlay")?;
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
    let height = (text_height + 48.)
        .min(f32::from(bounds.size.height) - margin - 16.)
        .max(1.);
    cx.open_window(
        gpui::WindowOptions {
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
