use super::*;
use gpui_kit::TestSupportExt as _;

const PAGES: [(usize, &str, IconName, &str); 5] = [
    (0, "实时字幕", IconName::FileText, "Ctrl+1"),
    (1, "语音服务", IconName::Mic, "Ctrl+2"),
    (4, "模型库", IconName::HardDrive, "Ctrl+5"),
    (2, "字幕样式", IconName::Palette, "Ctrl+3"),
    (3, "运行诊断", IconName::SquareTerminal, "Ctrl+4"),
];

impl ControlView {
    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        cx.notify();
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let session = self.session.read(cx);
        let menu = SidebarMenu::new().gap_1().children(PAGES.into_iter().map(
            |(page, label, icon, shortcut)| {
                SidebarMenuItem::new(label)
                    .icon(icon)
                    .active(self.page == page)
                    .suffix(move |_, cx| {
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(shortcut)
                    })
                    .on_click(cx.listener(move |s, _, window, cx| s.select_page(page, window, cx)))
            },
        ));
        Sidebar::new("navigation")
            .w_56()
            .collapsed(self.sidebar_collapsed)
            .header(
                row()
                    .h_9()
                    .px_3()
                    .flex_nowrap()
                    .child(Icon::new(IconName::PanelLeft).small())
                    .when(!self.sidebar_collapsed, |v| {
                        v.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("工作区"),
                        )
                    }),
            )
            .child(menu)
            .footer(
                column()
                    .gap_2()
                    .p_3()
                    .when(!self.sidebar_collapsed, |v| {
                        v.child(
                            div()
                                .text_sm()
                                .pb_2()
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .child("当前链路"),
                        )
                        .child(help("语音识别", cx))
                        .child(div().text_sm().child(session.asr_config.provider.name()))
                        .child(help("文字翻译", cx))
                        .child(
                            div()
                                .text_sm()
                                .child(if session.asr_config.provider.translates() {
                                    "服务自带译文"
                                } else {
                                    session.translation_provider.name()
                                }),
                        )
                        .child(
                            div()
                                .pt_3()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("Subveil {}", env!("CARGO_PKG_VERSION"))),
                        )
                    })
                    .child(
                        Button::new("quit")
                            .icon(IconName::WindowClose)
                            .when(!self.sidebar_collapsed, |b| b.label("退出 Subveil"))
                            .accessibility_label("退出 Subveil")
                            .ghost()
                            .small()
                            .on_click(|_, _, cx| cx.quit()),
                    ),
            )
    }

    fn render_workspace_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let overlay = self.session.read(cx).overlay_enabled();
        row()
            .id("workspace-bar")
            .test_support()
            .h_11()
            .flex_shrink_0()
            .px_3()
            .flex_nowrap()
            .bg(cx.theme().title_bar)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("toggle-sidebar")
                    .icon(IconName::PanelLeft)
                    .accessibility_label("折叠或展开侧栏")
                    .ghost()
                    .small()
                    .tooltip("Ctrl+B")
                    .on_click(cx.listener(|s, _, _, cx| s.toggle_sidebar(cx))),
            )
            .child(Icon::new(IconName::GalleryVerticalEnd).small())
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("Subveil"),
            )
            .child(help("/", cx))
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("字幕工作区"),
            )
            .when(self.page == 2, |v| {
                v.child(
                    button(
                        "save-appearance-toolbar",
                        "保存样式",
                        &self.session,
                        Session::save_settings,
                    )
                    .disabled(self.session.read(cx).saving)
                    .tooltip("Ctrl+S"),
                )
            })
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
                    .on_click(cx.listener(|s, _, _, cx| s.toggle_theme(cx))),
            )
    }
}

impl Render for ControlView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let session = self.session.read(cx);
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
        let scheme = session.settings.interface_theme.name();
        let (_, title, icon, shortcut) = PAGES
            .iter()
            .find(|p| p.0 == self.page)
            .cloned()
            .unwrap_or_else(|| PAGES[0].clone());
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
            .on_action(cx.listener(|s, _: &Live, w, cx| s.select_page(0, w, cx)))
            .on_action(cx.listener(|s, _: &Services, w, cx| s.select_page(1, w, cx)))
            .on_action(cx.listener(|s, _: &Appearance, w, cx| s.select_page(2, w, cx)))
            .on_action(cx.listener(|s, _: &Diagnostics, w, cx| s.select_page(3, w, cx)))
            .on_action(cx.listener(|s, _: &Models, w, cx| s.select_page(4, w, cx)))
            .on_action(cx.listener(|s, _: &ToggleSidebar, _, cx| s.toggle_sidebar(cx)))
            .on_action(cx.listener(|s, _: &ToggleOverlay, _, cx| {
                s.session.update(cx, Session::toggle_overlay)
            }))
            .on_action(cx.listener(|s, _: &SaveStyle, _, cx| s.save(cx)))
            .on_action(cx.listener(|s, _: &ToggleTheme, _, cx| s.toggle_theme(cx)))
            .child(self.render_workspace_bar(cx))
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
                                    .h_9()
                                    .gap_0()
                                    .flex_nowrap()
                                    .flex_shrink_0()
                                    .bg(cx.theme().tab_bar)
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        row()
                                            .h_full()
                                            .px_4()
                                            .flex_nowrap()
                                            .bg(cx.theme().tab_active)
                                            .border_r_1()
                                            .border_color(cx.theme().border)
                                            .child(Icon::new(icon).small())
                                            .child(div().text_sm().child(title)),
                                    )
                                    .child(div().flex_1())
                                    .child(
                                        div()
                                            .px_4()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(shortcut),
                                    ),
                            )
                            .child(
                                div()
                                    .id(("workspace-scroll", self.page))
                                    .test_support()
                                    .flex_1()
                                    .min_h_0()
                                    .min_w_0()
                                    .track_scroll(&self.scroll[self.page])
                                    .overflow_y_scroll()
                                    .vertical_scrollbar(&self.scroll[self.page])
                                    .child(
                                        div()
                                            .w_full()
                                            .max_w(gpui_kit::rems(72.))
                                            .mx_auto()
                                            .p_5()
                                            .child(content),
                                    ),
                            )
                            .child(
                                column()
                                    .gap_1()
                                    .px_5()
                                    .py_2()
                                    .border_t_1()
                                    .border_color(cx.theme().border)
                                    .child(help(message, cx))
                                    .children(error.map(|e| {
                                        div().text_sm().text_color(cx.theme().warning).child(e)
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
                    .right(
                        Button::new("open-theme-settings")
                            .label(scheme)
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|s, _, w, cx| s.select_page(2, w, cx))),
                    ),
            )
    }
}
