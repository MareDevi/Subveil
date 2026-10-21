//! Native choice data and readiness. The view never owns service decisions.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Choice {
    Profile,
    SourceLanguage,
    TargetLanguage,
    Translation,
    TranslationModel,
    Compute,
    AsrCompute,
    WhisperBlock,
}
impl Choice {
    pub const ALL: [Self; 8] = [
        Self::Profile,
        Self::SourceLanguage,
        Self::TargetLanguage,
        Self::Translation,
        Self::TranslationModel,
        Self::Compute,
        Self::AsrCompute,
        Self::WhisperBlock,
    ];
}
impl Session {
    pub fn configuration_busy(&self) -> bool {
        self.asr_active()
            || self.asr_stopping
            || self.importing_model
            || self.restoring_local
            || self.local_models.downloading()
            || self.translation_models.downloading()
            || self.vad_models.downloading()
            || self.credentials_busy
            || self.cloud_busy
            || self.saving_local
    }
    pub fn choice_options(&self, choice: Choice) -> Vec<String> {
        match choice {
            Choice::Profile => self.profiles.iter().map(|p| p.name.clone()).collect(),
            Choice::SourceLanguage => {
                if self.asr_config.provider == subveil_asr::Provider::Local {
                    self.local_languages.clone()
                } else {
                    self.asr_config
                        .provider
                        .source_languages()
                        .iter()
                        .map(|s| (*s).into())
                        .collect()
                }
            }
            Choice::TargetLanguage => self
                .asr_config
                .provider
                .target_languages()
                .iter()
                .filter(|lang| {
                    self.asr_config.provider != subveil_asr::Provider::Volcano
                        || self.asr_config.language == "en"
                        || **lang != "ja"
                })
                .map(|s| (*s).into())
                .collect(),
            Choice::Translation => [
                subveil_translate::Provider::Disabled,
                subveil_translate::Provider::Local,
                subveil_translate::Provider::OpenAi,
                subveil_translate::Provider::DeepL,
            ]
            .into_iter()
            .filter(|p| {
                !self.offline
                    || matches!(
                        p,
                        subveil_translate::Provider::Disabled | subveil_translate::Provider::Local
                    )
            })
            .map(|p| p.name().into())
            .collect(),
            Choice::TranslationModel => self
                .translation_models
                .catalog
                .iter()
                .map(|m| m.name.clone())
                .collect(),
            Choice::Compute => std::iter::once("CPU".into())
                .chain(self.gpu_devices.iter().map(|d| d.name.clone()))
                .collect(),
            Choice::AsrCompute => vec!["CPU".into(), "NVIDIA CUDA0".into()],
            Choice::WhisperBlock => vec!["2 秒".into(), "4 秒".into(), "8 秒".into()],
        }
    }
    pub fn choice_value(&self, choice: Choice) -> String {
        match choice {
            Choice::Profile => self
                .profiles
                .iter()
                .find(|p| p.id == self.selected_profile)
                .map(|p| p.name.clone())
                .unwrap_or_default(),
            Choice::SourceLanguage => self.asr_config.language.clone(),
            Choice::TargetLanguage => self.asr_config.target_language.clone(),
            Choice::Translation => self.translation_provider.name().into(),
            Choice::TranslationModel => self
                .translation_models
                .current()
                .map(|m| m.name.clone())
                .unwrap_or_default(),
            Choice::Compute => match self.compute {
                subveil_translate::Compute::Cpu => "CPU".into(),
                subveil_translate::Compute::Gpu(index) => self
                    .gpu_devices
                    .iter()
                    .find(|d| d.index == index)
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| "所选 GPU 不可用".into()),
            },
            Choice::AsrCompute => {
                if self.asr_config.local_compute == subveil_translate::Compute::Cpu {
                    "CPU"
                } else {
                    "NVIDIA CUDA0"
                }
                .into()
            }
            Choice::WhisperBlock => format!("{} 秒", self.asr_config.whisper_block_ms / 1000),
        }
    }
    pub fn select_choice(&mut self, choice: Choice, value: &str, cx: &mut Context<Self>) {
        if self.configuration_busy() || !self.choice_options(choice).iter().any(|v| v == value) {
            return;
        }
        match choice {
            Choice::Profile => {
                self.select_profile(value, cx);
                return;
            }
            Choice::SourceLanguage => {
                self.asr_config.language = value.into();
                self.local_language = Some(value.into());
            }
            Choice::TargetLanguage => self.asr_config.target_language = value.into(),
            Choice::Translation if !self.asr_config.provider.translates() => {
                if let Some(provider) = [
                    subveil_translate::Provider::Disabled,
                    subveil_translate::Provider::Local,
                    subveil_translate::Provider::OpenAi,
                    subveil_translate::Provider::DeepL,
                ]
                .into_iter()
                .find(|p| p.name() == value)
                {
                    self.translation_provider = provider;
                }
            }
            Choice::TranslationModel => {
                if let Some(index) = self
                    .translation_models
                    .catalog
                    .iter()
                    .position(|m| m.name == value)
                {
                    self.translation_models.selected = index;
                    self.translation_models.snapshot = Default::default();
                    self.translation_model = None;
                    self.translation_status =
                        "请安装并选用所选模型；已缓存的文件会校验后复用".into();
                }
            }
            Choice::Compute => {
                self.compute = self
                    .gpu_devices
                    .iter()
                    .find(|d| d.name == value)
                    .map(|d| subveil_translate::Compute::Gpu(d.index))
                    .unwrap_or_default()
            }
            Choice::AsrCompute => {
                self.asr_config.local_compute = if value == "CPU" {
                    subveil_translate::Compute::Cpu
                } else {
                    subveil_translate::Compute::Gpu(0)
                }
            }
            Choice::WhisperBlock => {
                self.asr_config.whisper_block_ms = match value {
                    "2 秒" => 2000,
                    "4 秒" => 4000,
                    _ => 8000,
                }
            }
            _ => {}
        }
        self.allow_text_upload = false;
        if !self
            .choice_options(Choice::TargetLanguage)
            .contains(&self.asr_config.target_language)
            && let Some(target) = self.choice_options(Choice::TargetLanguage).first()
        {
            self.asr_config.target_language = target.clone();
        }
        cx.notify();
    }
    pub fn start_issues(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.configuration_busy() {
            issues.push("请等待当前操作完成".into());
        }
        if self.audio_active() && !self.asr_active() {
            issues.push("请先停止独立采集".into());
        }
        if let subveil_audio::CaptureTarget::Node(source) = &self.capture_target
            && !self.audio_sources.iter().any(|s| s.same_lifetime(source))
        {
            issues.push("所选音源已断开；请刷新并重新选择".into());
        }
        let mut config = self.asr_config.clone();
        config.allow_cloud = true;
        if let Err(error) = config.validate() {
            issues.push(error.to_string());
        }
        if !config.provider.translates()
            && self.translation_provider == subveil_translate::Provider::Local
            && self.translation_model.is_none()
        {
            issues.push("请安装并选用本地翻译模型，或选择仅显示原文".into());
        }
        if !config.provider.translates()
            && matches!(
                self.translation_provider,
                subveil_translate::Provider::OpenAi | subveil_translate::Provider::DeepL
            )
        {
            if !self.allow_text_upload {
                issues.push("请先授权本次文本上传，或关闭文字翻译".into());
            }
            let required: &[&str] =
                if self.translation_provider == subveil_translate::Provider::DeepL {
                    &["DEEPL_API_KEY"]
                } else {
                    &[
                        "SUBVEIL_TRANSLATE_URL",
                        "SUBVEIL_TRANSLATE_KEY",
                        "SUBVEIL_TRANSLATE_MODEL",
                    ]
                };
            for key in required {
                if let Err(error) = self
                    .asr_config
                    .services
                    .as_ref()
                    .cloned()
                    .unwrap_or_default()
                    .get(key)
                {
                    issues.push(error.to_string());
                }
            }
        }
        if self.offline
            && (config.provider.is_cloud()
                || matches!(
                    self.translation_provider,
                    subveil_translate::Provider::OpenAi | subveil_translate::Provider::DeepL
                ))
        {
            issues.push("离线模式不允许云服务".into());
        }
        issues
    }
}
