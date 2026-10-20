use gpui::{
    App, AppContext, Bounds, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    Styled, TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions, div, prelude::*,
    px, rgb, size,
};
use gpui_component::{Root, button::Button};
use subveil_core::{config::DisplayMode, subtitle::TranslationState};

use crate::session::Session;

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

pub struct ControlView {
    session: Entity<Session>,
}

impl ControlView {
    fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        cx.observe(&session, |_, _, cx| cx.notify()).detach();
        Self { session }
    }
}

impl Render for ControlView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let session = self.session.read(cx);
        let settings = &session.settings;
        let row = || div().flex().flex_wrap().items_center().gap_2();
        let mut preview = div()
            .id("subtitle-preview")
            .h(px(132.))
            .flex_shrink_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .rounded_lg()
            .bg(rgb(0x1b2432));
        if let Some(subtitle) = session.subtitles.current() {
            preview = preview.child(subtitle.transcript.text.clone()).child(
                match &subtitle.translation {
                    TranslationState::Complete(text) => text.clone(),
                    TranslationState::Pending => "正在生成模拟译文…".into(),
                    TranslationState::Failed(error) => format!("模拟翻译失败：{error}"),
                    TranslationState::NotRequested => "正在识别（模拟）…".into(),
                },
            );
        } else {
            preview = preview.child("暂无字幕；演示会在短暂静默后自动开始下一段。".to_owned());
        }
        let mode_label = match settings.display_mode {
            DisplayMode::Bilingual => "双语",
            DisplayMode::Source => "仅原文",
            DisplayMode::Translation => "仅译文",
        };
        let mut history = div().flex().flex_col().gap_2();
        for entry in session.subtitles.history().iter().rev().take(4) {
            history = history.child(div().text_sm().text_color(rgb(0x94a3b8)).child(format!(
                "#{} · r{} · {} · {}",
                entry.transcript.key.segment_id,
                entry.transcript.key.revision,
                if entry.expired {
                    "已过期"
                } else if entry.is_final {
                    "已确定"
                } else {
                    "识别中"
                },
                entry.transcript.text
            )));
        }
        div()
            .id("subveil-control")
            .size_full()
            .overflow_y_scroll()
            .p_6()
            .flex()
            .flex_col()
            .gap_4()
            .bg(rgb(0x10151e))
            .text_color(rgb(0xe8edf5))
            .child(div().text_3xl().child("Subveil"))
            .child("M01 · Wayland 字幕浮窗 · 模拟演示")
            .child("不采集音频，不连接云端。此处文字均由内置样本生成。")
            .child(
                row()
                    .child(button(
                        "start",
                        if session.running {
                            "重新开始演示"
                        } else {
                            "开始演示"
                        },
                        &self.session,
                        |s, cx| {
                            if s.running {
                                s.stop(cx);
                            }
                            s.start(cx);
                        },
                    ))
                    .child(button("stop", "停止并清空", &self.session, Session::stop))
                    .child(button(
                        "overlay",
                        if session.overlay_visible() {
                            "隐藏浮窗"
                        } else {
                            "显示浮窗"
                        },
                        &self.session,
                        Session::toggle_overlay,
                    ))
                    .child(button(
                        "save",
                        if session.saving {
                            "正在保存…"
                        } else {
                            "保存样式"
                        },
                        &self.session,
                        Session::save_settings,
                    ))
                    .child(
                        Button::new("quit")
                            .label("退出 Subveil")
                            .on_click(|_, _, cx| cx.quit()),
                    ),
            )
            .child(div().text_sm().child(session.message.clone()))
            .children(session.config_error.as_ref().map(|error| {
                div()
                    .text_sm()
                    .text_color(rgb(0xfbbf24))
                    .child(error.clone())
            }))
            .child(preview)
            .child(
                row()
                    .child(format!("字号：{:.0}px", settings.font_size))
                    .child(button(
                        "font-smaller",
                        "字号 −",
                        &self.session,
                        |s, cx| {
                            s.change_settings(|c| c.font_size = (c.font_size - 2.).max(18.), cx)
                        },
                    ))
                    .child(button("font-larger", "字号 +", &self.session, |s, cx| {
                        s.change_settings(|c| c.font_size = (c.font_size + 2.).min(64.), cx)
                    }))
                    .child(button(
                        "font-family",
                        format!("字体：{}", settings.font_family),
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
                    .child(button(
                        "color",
                        "切换译文颜色",
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
                    )),
            )
            .child(
                row()
                    .child(format!(
                        "背景不透明度：{:.0}%",
                        settings.background_opacity * 100.
                    ))
                    .child(button(
                        "opacity-less",
                        "背景 −",
                        &self.session,
                        |s, cx| {
                            s.change_settings(
                                |c| c.background_opacity = (c.background_opacity - 0.1).max(0.),
                                cx,
                            )
                        },
                    ))
                    .child(button(
                        "opacity-more",
                        "背景 +",
                        &self.session,
                        |s, cx| {
                            s.change_settings(
                                |c| c.background_opacity = (c.background_opacity + 0.1).min(1.),
                                cx,
                            )
                        },
                    ))
                    .child(button("mode", mode_label, &self.session, |s, cx| {
                        s.change_settings(
                            |c| {
                                c.display_mode = match c.display_mode {
                                    DisplayMode::Bilingual => DisplayMode::Source,
                                    DisplayMode::Source => DisplayMode::Translation,
                                    DisplayMode::Translation => DisplayMode::Bilingual,
                                }
                            },
                            cx,
                        )
                    }))
                    .child(button(
                        "passthrough",
                        if settings.click_through {
                            "鼠标穿透：开"
                        } else {
                            "鼠标穿透：关"
                        },
                        &self.session,
                        |s, cx| s.change_settings(|c| c.click_through = !c.click_through, cx),
                    )),
            )
            .child(
                row()
                    .child(format!("宽度：{:.0}px", settings.width))
                    .child(button("narrower", "宽度 −", &self.session, |s, cx| {
                        s.change_settings(|c| c.width = (c.width - 80.).max(320.), cx)
                    }))
                    .child(button("wider", "宽度 +", &self.session, |s, cx| {
                        s.change_settings(|c| c.width = (c.width + 80.).min(2400.), cx)
                    }))
                    .child(format!("底部距离：{:.0}px", settings.bottom_margin))
                    .child(button("lower", "向下", &self.session, |s, cx| {
                        s.change_settings(|c| c.bottom_margin = (c.bottom_margin - 16.).max(0.), cx)
                    }))
                    .child(button("higher", "向上", &self.session, |s, cx| {
                        s.change_settings(
                            |c| c.bottom_margin = (c.bottom_margin + 16.).min(400.),
                            cx,
                        )
                    }))
                    .child(button(
                        "lines",
                        format!("每种语言最多 {} 行", settings.max_lines),
                        &self.session,
                        |s, cx| s.change_settings(|c| c.max_lines = c.max_lines % 6 + 1, cx),
                    )),
            )
            .child(div().text_sm().child(format!(
                    "会话 {} · 历史 {}/{} · {}",
                    session.subtitles.session_id(),
                    session.subtitles.history().len(),
                    settings.history_limit,
                    session
                        .config_path
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "配置目录不可用".into())
                )))
            .child(history)
            .child(div().text_sm().text_color(rgb(0x94a3b8)).child(
                "关闭控制窗口会保留演示，并打开会话恢复入口。关闭恢复入口或点击退出可结束程序。",
            ))
    }
}

struct RecoveryView {
    session: Entity<Session>,
}

impl Render for RecoveryView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_6()
            .flex()
            .flex_col()
            .gap_4()
            .bg(rgb(0x10151e))
            .text_color(rgb(0xe8edf5))
            .child("Subveil 会话仍在运行")
            .child("字幕状态已保留。恢复控制窗口或退出程序。")
            .child(button(
                "restore",
                "恢复控制窗口",
                &self.session,
                Session::restore_control,
            ))
            .child(
                Button::new("quit-session")
                    .label("退出 Subveil")
                    .on_click(|_, _, cx| cx.quit()),
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
        ..Default::default()
    }
}

pub fn open_control(session: Entity<Session>, cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    cx.open_window(options("Subveil", 920., 720., cx), |window, cx| {
        let view = cx.new(|cx| ControlView::new(session, cx));
        cx.new(|cx| Root::new(view, window, cx))
    })
}

pub fn open_recovery(session: Entity<Session>, cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    cx.open_window(
        options("Subveil · 会话", 440., 240., cx),
        |window, cx| {
            let view = cx.new(|_| RecoveryView { session });
            cx.new(|cx| Root::new(view, window, cx))
        },
    )
}
