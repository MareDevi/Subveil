use super::*;

fn translation_id(provider: subveil_translate::Provider) -> &'static str {
    match provider {
        subveil_translate::Provider::Disabled => "disabled",
        subveil_translate::Provider::Local => "local",
        subveil_translate::Provider::OpenAi => "openai",
        subveil_translate::Provider::DeepL => "deepl",
    }
}
fn translation(id: &str) -> Option<subveil_translate::Provider> {
    [
        subveil_translate::Provider::Disabled,
        subveil_translate::Provider::Local,
        subveil_translate::Provider::OpenAi,
        subveil_translate::Provider::DeepL,
    ]
    .into_iter()
    .find(|p| translation_id(*p) == id)
}
impl Session {
    fn profile_snapshot(&self, id: u32, name: String) -> storage::ServiceProfile {
        storage::ServiceProfile {
            id,
            name,
            provider: self.asr_config.provider.id().into(),
            translation: translation_id(self.translation_provider).into(),
            language: self.asr_config.language.clone(),
            target: self.asr_config.target_language.clone(),
            source: match &self.capture_target {
                subveil_audio::CaptureTarget::DefaultOutput => None,
                subveil_audio::CaptureTarget::Node(source) => {
                    Some(storage::SourcePreference::from_source(source))
                }
            },
        }
    }
    pub(super) fn refresh_profile_metadata(&mut self) {
        if let Some(index) = self
            .profiles
            .iter()
            .position(|p| p.id == self.selected_profile)
        {
            self.profiles[index] =
                self.profile_snapshot(self.selected_profile, self.profiles[index].name.clone());
        }
    }
    pub(super) fn restore_profile_selection(&mut self) {
        if let Some(profile) = self.profiles.iter().find(|p| p.id == self.selected_profile) {
            if let Some(provider) = subveil_asr::Provider::ALL
                .into_iter()
                .find(|p| p.id() == profile.provider)
            {
                self.asr_config.provider = provider;
            }
            if let Some(provider) = translation(&profile.translation) {
                self.translation_provider = provider;
            }
            self.asr_config.language = profile.language.clone();
            self.asr_config.target_language = profile.target.clone();
            self.capture_target = profile
                .source
                .as_ref()
                .and_then(|s| s.source().ok())
                .map(subveil_audio::CaptureTarget::Node)
                .unwrap_or_default();
            if let Some(rate) = self.asr_config.provider.required_rate() {
                self.asr_config.sample_rate = rate;
            }
            self.cloud_models = subveil_asr::discovery::defaults(self.asr_config.provider);
        }
    }
    pub fn select_profile(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.configuration_busy() || self.audio_active() {
            return;
        }
        let Some(id) = self.profiles.iter().find(|p| p.name == name).map(|p| p.id) else {
            return;
        };
        if id == self.selected_profile {
            return;
        }
        self.refresh_profile_metadata();
        self.profile_secrets.insert(
            self.selected_profile,
            self.asr_config.services.clone().unwrap_or_default(),
        );
        self.selected_profile = id;
        self.restore_profile_selection();
        self.asr_config.services = Some(self.profile_secrets.get(&id).cloned().unwrap_or_default());
        self.allow_text_upload = false;
        self.asr_config.allow_cloud = false;
        self.credentials_generation += 1;
        self.cloud_text_models.clear();
        self.credentials_status = "已切换配置；需要时读取此配置的系统凭据，上传须重新授权".into();
        self.save_local_settings(cx);
        cx.notify();
    }
    pub fn save_named_profile(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.configuration_busy() {
            return;
        }
        let name = name.trim();
        if name.is_empty() || name.len() > 64 || name.chars().any(char::is_control) {
            self.message = "配置名称需要 1–64 字节，且不能包含控制字符".into();
            cx.notify();
            return;
        }
        let existing = self.profiles.iter().position(|p| p.name == name);
        if existing.is_none() && self.profiles.len() >= 32 {
            self.message = "最多保存 32 个配置".into();
            cx.notify();
            return;
        }
        let id = existing.map(|i| self.profiles[i].id).unwrap_or_else(|| {
            self.profiles
                .iter()
                .map(|p| p.id)
                .max()
                .unwrap_or(0)
                .saturating_add(1)
        });
        self.refresh_profile_metadata();
        let secrets = self.asr_config.services.clone().unwrap_or_default();
        self.profile_secrets
            .insert(self.selected_profile, secrets.clone());
        self.profile_secrets.insert(id, secrets);
        let profile = self.profile_snapshot(id, name.into());
        if let Some(index) = existing {
            self.profiles[index] = profile;
        } else {
            self.profiles.push(profile);
        }
        self.selected_profile = id;
        self.allow_text_upload = false;
        self.asr_config.allow_cloud = false;
        self.save_local_settings(cx);
        self.credentials_status = "配置选择已保存；密钥仍只在内存，需另点保存到系统密钥环".into();
        cx.notify();
    }
}
