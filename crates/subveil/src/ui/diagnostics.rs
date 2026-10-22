use super::*;

impl ControlView {
    pub(super) fn render_diagnostics(&self, cx: &App) -> gpui_kit::Div {
        let session = self.session.read(cx);
        let (source_paint, translation_paint) = session.presentation.summaries();
        column()
            .child(heading(
                "运行诊断",
                "检查本机音频输入与会话状态。独立采集不会上传音频。",
                cx,
            ))
            .child(field(
                "系统音频",
                row()
                    .child(
                        button(
                            "audio-toggle",
                            if session.audio_stopping {
                                "正在停止…"
                            } else if session.audio_active() {
                                "停止独立采集"
                            } else {
                                "开始独立采集"
                            },
                            &self.session,
                            Session::toggle_audio,
                        )
                        .disabled(session.asr_active() || session.audio_stopping)
                        .loading(session.audio_stopping),
                    )
                    .child(
                        button(
                            "audio-rate",
                            format!("采样率 {} Hz · 切换", session.audio.sample_rate),
                            &self.session,
                            Session::cycle_audio_rate,
                        )
                        .disabled(
                            session.audio_active()
                                || session.asr_active()
                                || session.audio_stopping,
                        ),
                    ),
                cx,
            ))
            .child(div().text_sm().child(session.audio.status.clone()))
            .child(help(
                format!(
                    "默认输出：{} · 单声道 F32LE",
                    session.audio.device.as_deref().unwrap_or("等待设备")
                ),
                cx,
            ))
            .child(
                div().h_2().w_full().bg(cx.theme().muted).child(
                    div()
                        .h_full()
                        .w(relative(session.audio.peak.clamp(0., 1.)))
                        .bg(cx.theme().primary),
                ),
            )
            .child(help(
                format!(
                    "RMS {:.1} dBFS · 峰值 {:.1} dBFS · {}",
                    20. * session.audio.rms.max(0.000001).log10(),
                    20. * session.audio.peak.max(0.000001).log10(),
                    if session.audio.silent {
                        "静音"
                    } else {
                        "有声"
                    }
                ),
                cx,
            ))
            .child(section(
                "运行统计",
                "统计来自当前会话；丢块或重置有助于定位延迟与输入中断。",
                cx,
            ))
            .child(field(
                "采集统计",
                help(
                    format!(
                        "PCM 块 {} · 样本 {} · 丢块 {} · 丢样本 {} · 无效块 {} · 连接 {}",
                        session.audio.blocks,
                        session.audio.samples,
                        session.audio.dropped_blocks,
                        session.audio.dropped_samples,
                        session.audio.invalid_chunks,
                        session.audio.connections
                    ),
                    cx,
                ),
                cx,
            ))
            .child(field(
                "识别统计",
                help(
                    format!(
                        "识别重置 {} · 输入丢块 {} · 结果丢事件 {} · 丢弃分块 {}",
                        session.asr.resets,
                        session.asr.dropped_audio,
                        session.asr.dropped_events,
                        session.asr.dropped_segments
                    ),
                    cx,
                ),
                cx,
            ))
            .child(field(
                "SenseVoice 丢段",
                help(
                    format!(
                        "累计 {} ms · 最近范围 {}",
                        session.asr.dropped_segment_ms,
                        session.asr.last_dropped_segment.map_or_else(
                            || "无".into(),
                            |(start, end)| format!("{start}–{end} ms")
                        )
                    ),
                    cx,
                ),
                cx,
            ))
            .child(field(
                "会话",
                help(
                    format!(
                        "会话 {} · 历史 {}/{}",
                        session.subtitles.session_id(),
                        session.subtitles.history().len(),
                        session.settings.history_limit
                    ),
                    cx,
                ),
                cx,
            ))
            .child(section("状态接收到浮窗绘制", "本会话修订级计时；最近 256 次绘制回调。仅 GPUI paint，不包含 compositor、显示器扫描或实际阅读时间。隐藏浮窗、隐藏语种、过载修订会没有绘制样本。", cx))
            .child(field("原文绘制", help(source_paint, cx), cx))
            .child(field("译文绘制", help(translation_paint, cx), cx))
            .child(field(
                "样式文件",
                help(
                    session
                        .config_path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "配置目录不可用".into()),
                    cx,
                ),
                cx,
            ))
            .child(help(
                "关闭控制窗口后，会话继续运行，并保留恢复入口。关闭恢复入口或点击退出可结束程序。",
                cx,
            ))
    }
}
