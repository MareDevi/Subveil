use super::*;

impl ControlView {
    pub(super) fn render_services(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let session = self.session.read(cx);
        let provider = session.asr_config.provider;
        let busy = session.asr_active()
            || session.asr_stopping
            || session.local_models.downloading()
            || session.translation_models.downloading()
            || session.importing_model
            || session.restoring_local;
        column()
            .child(heading("语音服务", "选择识别服务与文字翻译；本地识别不上传音频。", cx))
            .child(field("离线模式", Switch::new("offline-mode").label("仅使用本地模型")
                .checked(session.offline).disabled(busy)
                .on_click({ let session = self.session.clone(); move |_, _, cx| {
                    session.update(cx, |s, cx| s.toggle_offline(cx));
                }}), cx))
            .child(help(if session.offline { "仅使用已安装模型；禁止云端识别、文字上传和模型下载。" } else { "本地模型推理不需要联网；安装时需要网络。" }, cx))
            .child(field("识别服务", Select::new(&self.provider).disabled(busy || session.offline)
                .accessibility_label("识别服务").w_full(), cx))
            .child(help(format!("当前识别：{}", provider.name()), cx))
            .child(field("语言", row()
                .child(button("asr-language", format!("源语种：{}", if provider.automatic_language() { "自动识别" } else { &session.asr_config.language }), &self.session, Session::cycle_asr_language)
                    .disabled(busy || provider.automatic_language() || (provider == subveil_asr::Provider::Local && session.local_languages.len() <= 1)))
                .child(button("asr-target", format!("目标语种：{}", session.asr_config.target_language), &self.session, Session::cycle_target_language)
                    .disabled(busy)), cx))
            .child(column().gap_2()
                .child(div().text_base().child(if !provider.is_cloud() { "本地处理 · 不上传音频" } else { "开始后将上传系统音频" }))
                .child(help(format!("音频目标：{}", provider.destination()), cx))
                .child(help(if provider.translates() { "显示服务自带译文；不播放返回语音。" } else { "此服务仅输出原文。" }, cx))
                .child(help(if provider == subveil_asr::Provider::Mock {
                    "Mock ASR 会采集本机系统音频，按音频能量输出预设样本。"
                } else if provider == subveil_asr::Provider::Local { "在「模型库」选择并安装模型，再开始识别。模型推理均在本机执行。" } else { "仅点击下方同意按钮才开始上传。云服务可能收费；请先通过环境变量配置服务凭据。" }, cx)))
            .child(row().child(button("asr-start", if session.asr_stopping { "正在停止…" }
                else if session.asr_active() { "停止识别与音频" }
                else if provider == subveil_asr::Provider::Mock { "开始 Mock ASR" }
                else if provider == subveil_asr::Provider::Local { "开始本地识别" }
                else { "同意上传并开始" }, &self.session, Session::toggle_asr)
                .disabled(session.asr_stopping || session.local_models.downloading() || session.importing_model || (!session.asr_active() && (session.audio_active() || session.audio_stopping || (provider == subveil_asr::Provider::Local && session.asr_config.local_model.is_none()))))
                .loading(session.asr_stopping)))
            .child(help(if session.audio_active() && !session.asr_active() {
                "独立采集正在运行。请在「运行诊断」停止采集，再开始识别。"
            } else { "切换服务与语言前，请先停止识别。" }, cx))
            .child(div().text_sm().child(session.asr.status.clone()))
            .when(provider == subveil_asr::Provider::Local, |view| view.child(
                Button::new("open-asr-models").label("选择或安装识别模型…").icon(IconName::HardDrive)
                    .on_click(cx.listener(|s, _, window, cx| s.select_page(4, window, cx)))))
            .child(section("文字翻译", "独立翻译已确定的原文，或使用语音服务返回的译文。", cx))
            .child(field("文字翻译", button("translation-provider", format!("翻译服务：{}", session.translation_provider.name()), &self.session, Session::cycle_translation).disabled(busy || provider.translates()), cx))
            .when(session.translation_provider == subveil_translate::Provider::Local, |view| view.child(
                Button::new("open-translation-models").label("配置本地翻译模型…").icon(IconName::Cpu)
                    .on_click(cx.listener(|s, _, window, cx| s.select_page(4, window, cx)))))
            .child(help(if provider.translates() { "此语音服务自带译文，不再调用独立文字翻译。" } else { "仅翻译已确定的句子。云端文字翻译可能收费，需独立授权上传原文与最多三句上下文。" }, cx))
            .child(help(format!("文本上传目标：{}", session.translation_provider.destination()), cx))
            .child(button("consent-text", if session.allow_text_upload { "撤销文本上传授权" } else { "同意本次会话上传文本" }, &self.session, Session::consent_text_upload)
                .disabled(busy || session.offline || provider.translates() || !matches!(session.translation_provider, subveil_translate::Provider::OpenAi | subveil_translate::Provider::DeepL)))
            .child(help(session.translation_status.clone(), cx))
            .child(help(format!("翻译队列丢弃：{}", session.translation_dropped), cx))
            .child(button("save-local-settings", "保存本地设置", &self.session, Session::save_local_settings).disabled(busy || session.saving_local).loading(session.saving_local))
    }
}
