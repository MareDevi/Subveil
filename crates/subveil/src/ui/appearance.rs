use super::*;

impl ControlView {
    pub(super) fn render_appearance(&self, cx: &App) -> gpui_kit::Div {
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
            .child(section(
                "文字",
                "原文与译文共用字体，字号可分别按显示比例呈现。",
                cx,
            ))
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
            .child(section(
                "浮窗",
                "调整背景和位置；预览与桌面字幕共用显示样式。",
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
}
