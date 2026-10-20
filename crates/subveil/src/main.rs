use anyhow::Context as _;
use gpui::{
    App, AppContext, Bounds, Context, IntoElement, Render, Styled, TitlebarOptions, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};
use gpui_component::{Root, button::Button, button::ButtonVariants};
use tracing_subscriber::EnvFilter;

struct ControlWindow;

impl Render for ControlWindow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .p_8()
            .gap_6()
            .bg(rgb(0x10151e))
            .text_color(rgb(0xe8edf5))
            .child(div().text_3xl().child("Subveil"))
            .child("Native live subtitles · 原生实时字幕")
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_6()
                    .rounded_lg()
                    .bg(rgb(0x1b2432))
                    .child("M00 · 工程基础")
                    .child("控制窗口已就绪。音频采集、语音识别与翻译尚未实现。")
                    .child("当前不采集音频，不连接云端服务。"),
            )
            .child(
                Button::new("quit")
                    .primary()
                    .label("退出 Subveil")
                    .on_click(|_, _, cx| cx.quit()),
            )
    }
}

fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize logging: {error}"))?;

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting Subveil");
    gpui_platform::application().run(|cx: &mut App| {
        gpui_component::init(cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(760.), px(460.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Subveil".into()),
                ..Default::default()
            }),
            app_id: Some("subveil".into()),
            ..Default::default()
        };

        match cx
            .open_window(options, |window, cx| {
                let view = cx.new(|_| ControlWindow);
                cx.new(|cx| Root::new(view, window, cx))
            })
            .context("failed to open the Subveil control window")
        {
            Ok(_) => {
                tracing::info!("control window opened");
            }
            Err(error) => {
                tracing::error!(?error, "application startup failed");
                cx.quit();
                std::process::exit(1);
            }
        }
    });
    Ok(())
}
