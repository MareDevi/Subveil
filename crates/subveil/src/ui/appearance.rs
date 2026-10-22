use super::*;

impl ControlView {
    pub(super) fn render_appearance(&self, cx: &App) -> gpui_kit::Div {
        let session = self.session.read(cx);
        let settings = &session.settings;
        let mode_session = self.session.clone();
        let switch_session = self.session.clone();
        let reading_session = self.session.clone();
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
            .child(self.render_choice(Choice::Output, "显示屏幕", false, cx))
            .child(help(
                "自动选择按桌面位置排序；固定屏幕断开时暂停浮窗，重连后恢复。",
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
            .child(Switch::new("merge-short-subtitles").label("合并相邻短句阅读位置").checked(settings.merge_short_segments)
                .on_change(move |checked,_,cx| reading_session.update(cx,|s,cx|s.change_settings(|c|c.merge_short_segments = *checked,cx))))
            .child(help("仅合并已完成的相邻短译文，最多三段；原始身份、记录和导出保持独立。关闭可作逐段对照。", cx))
            .child(help(
                "浮窗宽度受显示器限制；Layer Shell 与鼠标穿透能力取决于 Wayland compositor。",
                cx,
            ))
            .child(section(
                "桌面入口",
                "按需启用；快捷键由桌面 Portal 请求授权，每次启动重新启用。",
                cx,
            ))
            .child(
                row()
                    .child(button(
                        "toggle-tray",
                        if session.tray_service.is_some() {
                            "停用托盘"
                        } else {
                            "启用托盘"
                        },
                        &self.session,
                        |s, cx| s.toggle_desktop_service(false, cx),
                    ))
                    .child(button(
                        "toggle-global-shortcuts",
                        if session.shortcuts_service.is_some() {
                            "停用全局快捷键"
                        } else {
                            "启用全局快捷键…"
                        },
                        &self.session,
                        |s, cx| s.toggle_desktop_service(true, cx),
                    )),
            )
            .child(help(session.tray_status.clone(), cx))
            .child(help(session.shortcuts_status.clone(), cx))
            .child(help(
                "快捷键仅恢复窗口、显隐字幕或停止会话。桌面不支持托盘时，保留原生恢复窗口。",
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
