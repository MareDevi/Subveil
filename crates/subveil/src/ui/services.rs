use super::*;

impl ControlView {
    fn render_service_inputs(
        &self,
        specs: &[service_fields::Field],
        busy: bool,
        cx: &App,
    ) -> gpui_kit::Div {
        let mut view = column().gap_2();
        for spec in specs {
            if let Some((_, input)) = self.service_inputs.iter().find(|(key, _)| *key == spec.key) {
                view = view.child(field(
                    spec.label,
                    Input::new(input).disabled(busy).when(spec.secret, |input| {
                        input.content_type(InputContentType::Password)
                    }),
                    cx,
                ));
            }
        }
        view
    }

    pub(super) fn render_services(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let session = self.session.read(cx);
        let provider = session.asr_config.provider;
        let busy = session.asr_active()
            || session.asr_stopping
            || session.local_models.downloading()
            || session.translation_models.downloading()
            || session.importing_model
            || session.restoring_local
            || session.credentials_busy
            || session.cloud_busy;
        column()
            .child(heading("语音服务", "选择识别服务与文字翻译；本地识别不上传音频。", cx))
            .child(self.render_choice(Choice::Profile,"服务配置",busy,cx))
            .child(field("配置名称",Input::new(&self.profile_name).disabled(busy),cx))
            .child(Button::new("save-profile").label("保存为配置").disabled(busy).on_click(cx.listener(|s,_,_,cx| {
                let name=s.profile_name.read(cx).value().to_string();s.session.update(cx,|session,cx|session.save_named_profile(&name,cx));
            })))
            .child(field("离线模式", Switch::new("offline-mode").label("仅使用本地模型")
                .checked(session.offline).disabled(busy)
                .on_click({ let session = self.session.clone(); move |_, _, cx| {
                    session.update(cx, |s, cx| s.toggle_offline(cx));
                }}), cx))
            .child(help(if session.offline { "仅使用已安装模型；禁止云端识别、文字上传和模型下载。" } else { "本地模型推理不需要联网；安装时需要网络。" }, cx))
            .child(field("识别服务", Select::new(&self.provider).disabled(busy || session.offline)
                .accessibility_label("识别服务").w_full(), cx))
            .child(help(format!("当前识别：{}", provider.name()), cx))
            .when(provider.is_cloud(), |view| view
                .child(self.render_service_inputs(service_fields::fields(provider), busy, cx))
                .child(Link::new("service-console").href(service_fields::console(provider)).child("获取服务凭据"))
                .when(!session.cloud_models.is_empty(), |view| view.child(field("语音模型", Select::new(&self.cloud_model).disabled(busy).w_full().accessibility_label("语音模型"), cx)))
                .child(button("discover-cloud-models", "查询兼容模型", &self.session, Session::discover_models)
                    .disabled(busy || session.offline || !subveil_asr::discovery::supported(provider)).loading(session.cloud_busy))
                .child(help(if subveil_asr::discovery::supported(provider) { session.cloud_status.clone() } else { "此协议使用官方固定项；Azure 与自定义服务请填写部署名或模型 ID。".into() }, cx)))
            .child(self.render_choice(Choice::SourceLanguage, "源语种", busy || provider.automatic_language(), cx))
            .child(self.render_choice(Choice::TargetLanguage, "目标语种", busy, cx))
            .child(column().gap_2()
                .child(div().text_base().child(if !provider.is_cloud() { "本地处理 · 不上传音频" } else { "开始后将上传系统音频" }))
                .child(help(format!("音频目标：{}", session.asr_config.destination()), cx))
                .child(help(if provider.translates() { "显示服务自带译文；不播放返回语音。" } else { "此服务仅输出原文。" }, cx))
                .child(help(if provider == subveil_asr::Provider::Mock {
                    "Mock ASR 会采集本机系统音频，按音频能量输出预设样本。"
                } else if provider == subveil_asr::Provider::Local { "在「模型库」选择并安装模型，再开始识别。模型推理均在本机执行。" } else { "仅点击下方同意按钮才开始上传。云服务可能收费；在此填写凭据后，仍需点击同意按钮才上传。" }, cx)))
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
            .child(section("文字翻译", "独立翻译原文，或使用语音服务返回的译文。", cx))
            .child(self.render_choice(Choice::Translation, "文字翻译", busy || provider.translates(), cx))
            .when(session.translation_provider == subveil_translate::Provider::Local, |view| view.child(
                Button::new("open-translation-models").label("配置本地翻译模型…").icon(IconName::Cpu)
                    .on_click(cx.listener(|s, _, window, cx| s.select_page(4, window, cx)))))
            .child(self.render_service_inputs(service_fields::translation_fields(session.translation_provider), busy || provider.translates(), cx))
            .when(session.translation_provider == subveil_translate::Provider::OpenAi && !provider.translates(), |view| view
                .child(button("discover-text-models", "获取文字模型", &self.session, Session::discover_text_models).disabled(busy || session.offline))
                .when(!session.cloud_text_models.is_empty(), |view| view.child(field("服务返回的文字模型", Select::new(&self.cloud_text_model).disabled(busy).w_full().accessibility_label("服务返回的文字模型"), cx)))
                .child(help(session.cloud_status.clone(), cx)))
            .child(help(if provider.translates() { "此语音服务自带译文，不再调用独立文字翻译。" } else { "云端文字翻译可能收费，需独立授权上传原文与最多三句已确定的上下文。" }, cx))
            .child(help(format!("文本上传目标：{}", session.text_destination()), cx))
            .child(button("consent-text", if session.allow_text_upload { "撤销文本上传授权" } else { "同意本次会话上传文本" }, &self.session, Session::consent_text_upload)
                .disabled(busy || session.offline || provider.translates() || !matches!(session.translation_provider, subveil_translate::Provider::OpenAi | subveil_translate::Provider::DeepL)))
            .child(field("增量翻译", Switch::new("incremental-translation").label("提前翻译稳定原文（实验性）")
                .checked(session.incremental_translation).disabled(busy || provider.translates())
                .on_click({ let owner = self.session.clone(); move |_, _, cx| owner.update(cx, Session::toggle_incremental_translation) }), cx))
            .child(help("关闭时只翻译 Final；开启后临时译文允许修订，Final 仍会重新翻译。短句或无稳定边界时等待句末。", cx))
            .child(help(session.translation_status.clone(), cx))
            .child(help(format!("翻译队列丢弃：{}", session.translation_dropped), cx))
            .child(section("云服务凭据", "输入即用于当前进程；保存与读取均使用系统密钥环，不写入设置文件。", cx))
            .child(help(session.credentials_availability.clone(),cx))
            .child(row()
                .child(button("save-credentials", "保存到系统密钥环", &self.session, Session::store_credentials).disabled(busy))
                .child(button("load-credentials", "读取系统凭据", &self.session, Session::load_credentials).disabled(busy))
                .child(button("clear-credentials", "清除云服务凭据", &self.session, Session::clear_credentials).disabled(busy)))
            .child(help(session.credentials_status.clone(), cx))
            .child(button("save-local-settings", "保存本地设置", &self.session, Session::save_local_settings).disabled(busy || session.saving_local).loading(session.saving_local))
    }
}
