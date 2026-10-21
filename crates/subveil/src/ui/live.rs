use super::*;

impl ControlView {
    pub(super) fn render_live(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let session = self.session.read(cx);
        let issues = session.start_issues();
        let busy = session.configuration_busy();
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
        let query = self.history_search.read(cx).value().to_lowercase();
        for entry in session.subtitles.history().iter().rev() {
            if session.archive.is_some() {
                break;
            }
            let translated = match &entry.translation {
                TranslationState::Complete(t)
                | TranslationState::Streaming(t)
                | TranslationState::PendingWithPreview(t) => t.as_str(),
                _ => "",
            };
            if !entry.transcript.text.to_lowercase().contains(&query)
                && !translated.to_lowercase().contains(&query)
            {
                continue;
            }
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
        if let Some(archive) = &session.archive {
            for (index, record) in archive.records.iter().enumerate().rev() {
                if !record.source.to_lowercase().contains(&query)
                    && !record
                        .translation
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&query)
                {
                    continue;
                }
                history = history.child(
                    div()
                        .id(("saved-segment", index))
                        .py_3()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(record.source.clone())
                        .child(help(record.translation.clone().unwrap_or_default(), cx)),
                );
            }
        }
        if session.subtitles.history().is_empty() && session.archive.is_none() {
            history = history.child(help("会话开始后，保留的原文与译文将显示在这里。", cx));
        }
        column()
            .child(heading(
                "实时字幕",
                "查看当前原文与服务译文，控制桌面字幕浮窗。",
                cx,
            ))
            .child(field(
                "音源",
                Select::new(&self.audio_source)
                    .w_full()
                    .accessibility_label("采集音源")
                    .disabled(
                        session.audio_active()
                            || session.asr_active()
                            || session.audio_stopping
                            || session.asr_stopping,
                    ),
                cx,
            ))
            .child(
                button(
                    "refresh-audio-sources",
                    "刷新音源",
                    &self.session,
                    Session::refresh_audio_sources,
                )
                .disabled(session.refreshing_sources)
                .loading(session.refreshing_sources),
            )
            .child(help(session.source_status.clone(), cx))
            .child(self.render_choice(Choice::Profile, "服务配置", busy, cx))
            .child(self.render_choice(
                Choice::SourceLanguage,
                "源语种",
                busy || session.asr_config.provider.automatic_language(),
                cx,
            ))
            .child(self.render_choice(Choice::TargetLanguage, "目标语种", busy, cx))
            .child(field(
                "识别服务",
                Select::new(&self.provider)
                    .w_full()
                    .accessibility_label("识别服务")
                    .disabled(busy || session.offline),
                cx,
            ))
            .child(self.render_choice(
                Choice::Translation,
                "文字翻译",
                busy || session.asr_config.provider.translates(),
                cx,
            ))
            .child(help(
                format!(
                    "当前链路：{} → {} → {}",
                    session.selected_source_label(),
                    session.asr_config.provider.name(),
                    if session.asr_config.provider.translates() {
                        "服务自带译文"
                    } else {
                        session.translation_provider.name()
                    }
                ),
                cx,
            ))
            .child(help(
                if issues.is_empty() {
                    "配置已就绪；开始后会后台校验并加载模型".into()
                } else {
                    issues.join("；")
                },
                cx,
            ))
            .child(
                button(
                    "start-live-subtitles",
                    if session.asr_active() {
                        "停止识别与音频"
                    } else if session.asr_config.provider.is_cloud() {
                        "同意上传音频并开始"
                    } else if session.asr_config.provider == subveil_asr::Provider::Mock {
                        "开始模拟识别（不是实际字幕）"
                    } else {
                        "开始本地字幕"
                    },
                    &self.session,
                    Session::toggle_asr,
                )
                .disabled(!session.asr_active() && !issues.is_empty())
                .primary(),
            )
            .when(session.asr_config.provider.is_cloud(), |view| {
                view.child(help(
                    format!(
                        "音频上传目标：{}；云服务可能收费",
                        session.asr_config.destination()
                    ),
                    cx,
                ))
            })
            .when(
                !session.asr_config.provider.translates()
                    && matches!(
                        session.translation_provider,
                        subveil_translate::Provider::OpenAi | subveil_translate::Provider::DeepL
                    ),
                |view| {
                    view.child(help(
                        format!(
                            "文本上传目标：{}；包含原文及最多三句上下文",
                            session.text_destination()
                        ),
                        cx,
                    ))
                    .child(
                        button(
                            "live-text-consent",
                            if session.allow_text_upload {
                                "撤销文本上传授权"
                            } else {
                                "同意本次会话上传文本"
                            },
                            &self.session,
                            Session::consent_text_upload,
                        )
                        .disabled(busy),
                    )
                },
            )
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
            .child(help(session.archive_status.clone(), cx))
            .child(field(
                "搜索字幕",
                Input::new(&self.history_search)
                    .id("history-search")
                    .aria_label("搜索原文或译文"),
                cx,
            ))
            .child(
                row().children(
                    [
                        ("toml", "保存字幕档案"),
                        ("txt", "导出 TXT"),
                        ("md", "导出 Markdown"),
                        ("srt", "导出 SRT"),
                    ]
                    .into_iter()
                    .map(|(format, label)| {
                        let owner = self.session.clone();
                        Button::new(format)
                            .label(label)
                            .disabled(session.archive_busy)
                            .on_click(move |_, _, cx| {
                                owner.update(cx, |s, cx| s.export_archive(format, cx))
                            })
                    }),
                ),
            )
            .child(
                row()
                    .child(
                        button(
                            "open-archive",
                            "打开字幕档案…",
                            &self.session,
                            Session::open_archive,
                        )
                        .disabled(session.archive_busy),
                    )
                    .child(
                        button(
                            "close-archive",
                            "返回当前会话",
                            &self.session,
                            |s, cx| {
                                s.archive = None;
                                s.archive_status = "已返回当前会话；不会自动保存字幕".into();
                                cx.notify();
                            },
                        )
                        .disabled(session.archive.is_none() || session.archive_busy),
                    ),
            )
            .child(history)
    }
}
