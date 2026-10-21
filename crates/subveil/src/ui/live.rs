use super::*;

impl ControlView {
    pub(super) fn render_live(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
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
                        TranslationState::Complete(text) => text.clone(),
                        TranslationState::Streaming(text)
                        | TranslationState::PendingWithPreview(text) => {
                            format!("{text}（临时译文）")
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
                .child(help("配置语音服务以开始识别，或运行离线演示预览字幕。", cx));
        }
        let mut history = column().gap_0();
        for entry in session.subtitles.history().iter().rev() {
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
                                "已离开浮窗"
                            } else if entry.is_final {
                                "已确定"
                            } else {
                                "识别中"
                            }
                        ),
                        cx,
                    ))
                    .child(entry.transcript.text.clone())
                    .child(div().pt_1().text_color(cx.theme().muted_foreground).child(
                        match &entry.translation {
                            TranslationState::Complete(text) => text.clone(),
                            TranslationState::Streaming(text)
                            | TranslationState::PendingWithPreview(text) => {
                                format!("{text}（临时译文）")
                            }
                            TranslationState::Pending => "正在生成译文…".into(),
                            TranslationState::Failed(error) => format!("翻译失败：{error}"),
                            TranslationState::NotRequested => "暂无译文".into(),
                        },
                    )),
            );
        }
        if session.subtitles.history().is_empty() {
            history = history.child(help("会话开始后，保留的原文与译文将显示在这里。", cx));
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
                        Button::new("configure-session")
                            .label("配置语音服务…")
                            .icon(IconName::Mic)
                            .on_click(cx.listener(|s, _, window, cx| s.select_page(1, window, cx))),
                    )
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
                        .icon(IconName::Play)
                        .disabled(session.asr_active() || session.asr_stopping),
                    )
                    .child(
                        button("stop", "停止字幕", &self.session, Session::stop)
                            .icon(IconName::Square)
                            .ghost()
                            .disabled(
                                !session.running
                                    && !session.asr_active()
                                    && !session.audio_active(),
                            ),
                    ),
            )
            .child(preview)
            .child(div().text_sm().child(format!(
                "当前服务：{} · {}",
                session.asr_config.provider.name(),
                session.asr.status
            )))
            .child(
                row()
                    .pt_4()
                    .child(
                        div()
                            .flex_1()
                            .text_base()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child("最近字幕"),
                    )
                    .child(help(
                        format!("最近 {} 段", session.subtitles.history().len()),
                        cx,
                    ))
                    .child(
                        button(
                            "clear-history",
                            "清空记录",
                            &self.session,
                            Session::clear_history,
                        )
                        .ghost()
                        .disabled(
                            session.running
                                || session.asr_active()
                                || session.asr_stopping
                                || session.subtitles.history().is_empty(),
                        ),
                    ),
            )
            .child(history)
    }
}
