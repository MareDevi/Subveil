//! Cloud service catalog and credential-free capability descriptions.
use anyhow::{Context as _, ensure};

/// Supported local and cloud speech services.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    /// Deterministic offline sample text driven by audio energy.
    Mock,
    /// User-imported sherpa-onnx streaming transducer.
    Local,
    /// Deepgram Listen v1 transcription.
    Deepgram,
    /// Alibaba Cloud LiveTranslate.
    Alibaba,
    /// Google Gemini Live translation.
    Gemini,
    /// Doubao Simultaneous Interpretation 2.0.
    Volcano,
    /// Tencent Cloud realtime speech translation.
    Tencent,
    /// Baidu realtime speech translation.
    Baidu,
    /// OpenAI continuous realtime translation.
    OpenAi,
    /// Azure OpenAI realtime translation deployment.
    AzureOpenAi,
    /// xAI Grok Voice, with transcription and turn-based translation.
    Xai,
    /// User-selected DashScope inference WebSocket.
    CustomDashScope,
    /// User-selected OpenAI realtime transcription WebSocket.
    CustomOpenAi,
}
impl Provider {
    /// Ordered catalog used by both GUI and headless diagnostics.
    pub const ALL: [Self; 13] = [
        Self::Mock,
        Self::Local,
        Self::Deepgram,
        Self::Alibaba,
        Self::Gemini,
        Self::Volcano,
        Self::Tencent,
        Self::Baidu,
        Self::OpenAi,
        Self::AzureOpenAi,
        Self::Xai,
        Self::CustomDashScope,
        Self::CustomOpenAi,
    ];
    /// Whether audio leaves this computer.
    pub fn is_cloud(self) -> bool {
        !matches!(self, Self::Mock | Self::Local)
    }
    /// Stable command-line identifier.
    pub fn id(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::Local => "local",
            Self::Deepgram => "deepgram",
            Self::Alibaba => "alibaba",
            Self::Gemini => "gemini",
            Self::Volcano => "volcano",
            Self::Tencent => "tencent",
            Self::Baidu => "baidu",
            Self::OpenAi => "openai",
            Self::AzureOpenAi => "azure-openai",
            Self::Xai => "xai",
            Self::CustomDashScope => "custom-dashscope",
            Self::CustomOpenAi => "custom-openai",
        }
    }
    /// Human-readable service name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Mock => "Mock",
            Self::Local => "本地语音识别",
            Self::Deepgram => "Deepgram",
            Self::Alibaba => "阿里云 LiveTranslate",
            Self::Gemini => "Google Gemini",
            Self::Volcano => "火山引擎",
            Self::Tencent => "腾讯云",
            Self::Baidu => "百度",
            Self::OpenAi => "OpenAI",
            Self::AzureOpenAi => "Azure OpenAI",
            Self::Xai => "xAI Grok",
            Self::CustomDashScope => "自定义 DashScope",
            Self::CustomOpenAi => "自定义 OpenAI Realtime",
        }
    }
    /// Fixed input rate required by this integration, if any.
    pub fn required_rate(self) -> Option<u32> {
        match self {
            Self::Mock | Self::Local | Self::Deepgram => None,
            Self::OpenAi | Self::AzureOpenAi | Self::Xai | Self::CustomOpenAi => Some(24_000),
            _ => Some(16_000),
        }
    }
    /// Whether this integration receives service translations.
    pub fn translates(self) -> bool {
        !matches!(
            self,
            Self::Mock | Self::Local | Self::Deepgram | Self::CustomDashScope | Self::CustomOpenAi
        )
    }
    /// Whether the service automatically detects the source language.
    pub fn automatic_language(self) -> bool {
        matches!(
            self,
            Self::Alibaba | Self::Gemini | Self::OpenAi | Self::AzureOpenAi
        )
    }
    /// Credential-free upload destination, for explicit per-session consent.
    pub fn destination(self) -> String {
        match self {
            Self::Mock => "本机 Mock（不上传）".into(),
            Self::Local => "本机所选识别设备（不上传）".into(),
            Self::Deepgram => "api.deepgram.com".into(),
            Self::Alibaba => "北京百炼业务空间".into(),
            Self::Gemini => "generativelanguage.googleapis.com".into(),
            Self::Volcano => "openspeech.bytedance.com".into(),
            Self::Tencent => "asr.cloud.tencent.com".into(),
            Self::Baidu => "aip.baidubce.com".into(),
            Self::OpenAi => "api.openai.com".into(),
            Self::Xai => "api.x.ai".into(),
            Self::AzureOpenAi => azure_endpoint()
                .ok()
                .and_then(|u| u.host_str().map(str::to_owned))
                .unwrap_or_else(|| "待配置 Azure 资源端点".into()),
            Self::CustomDashScope | Self::CustomOpenAi => custom_endpoint(self)
                .ok()
                .and_then(|u| u.host_str().map(str::to_owned))
                .unwrap_or_else(|| "待配置自定义端点".into()),
        }
    }
    pub(crate) fn validate_using(
        self,
        env: impl Fn(&str) -> anyhow::Result<String>,
    ) -> anyhow::Result<()> {
        let names: &[&str] = match self {
            Self::Mock | Self::Local => &[],
            Self::Deepgram => &["DEEPGRAM_API_KEY"],
            Self::Alibaba => &["DASHSCOPE_API_KEY", "DASHSCOPE_WORKSPACE_ID"],
            Self::Gemini => &["GEMINI_API_KEY"],
            Self::Volcano => &["VOLCENGINE_API_KEY"],
            Self::Tencent => &["TENCENT_APP_ID", "TENCENT_SECRET_ID", "TENCENT_SECRET_KEY"],
            Self::Baidu => &["BAIDU_APP_ID", "BAIDU_API_KEY"],
            Self::OpenAi => &["OPENAI_API_KEY"],
            Self::AzureOpenAi => &[
                "AZURE_OPENAI_API_KEY",
                "AZURE_OPENAI_ENDPOINT",
                "AZURE_OPENAI_DEPLOYMENT",
                "AZURE_OPENAI_TRANSCRIPTION_DEPLOYMENT",
            ],
            Self::Xai => &["XAI_API_KEY"],
            Self::CustomDashScope => &[
                "CUSTOM_DASHSCOPE_URL",
                "CUSTOM_DASHSCOPE_MODEL",
                "CUSTOM_DASHSCOPE_API_KEY",
            ],
            Self::CustomOpenAi => &[
                "CUSTOM_OPENAI_URL",
                "CUSTOM_OPENAI_MODEL",
                "CUSTOM_OPENAI_API_KEY",
            ],
        };
        for name in names {
            env(name)?;
        }
        match self {
            Self::Alibaba => {
                let w = env("DASHSCOPE_WORKSPACE_ID")?;
                ensure!(
                    w.len() <= 63
                        && w.bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                    "DASHSCOPE_WORKSPACE_ID 必须是业务空间 ID"
                );
            }
            Self::AzureOpenAi => {
                azure_url(&env("AZURE_OPENAI_ENDPOINT")?)?;
            }
            Self::CustomDashScope | Self::CustomOpenAi => {
                endpoint(&env(if self == Self::CustomDashScope {
                    "CUSTOM_DASHSCOPE_URL"
                } else {
                    "CUSTOM_OPENAI_URL"
                })?)?;
            }
            Self::Tencent => {
                ensure!(
                    env("TENCENT_APP_ID")?.bytes().all(|b| b.is_ascii_digit()),
                    "TENCENT_APP_ID 必须为数字"
                );
            }
            _ => {}
        }
        Ok(())
    }
}
pub(crate) fn env(name: &str) -> anyhow::Result<String> {
    let value = std::env::var(name).map_err(|_| anyhow::anyhow!("请配置环境变量 {name}"))?;
    ensure!(
        !value.trim().is_empty() && value.len() <= 4096 && !value.contains(['\r', '\n', '\0']),
        "{name} 无效"
    );
    Ok(value)
}
pub(crate) fn endpoint(raw: &str) -> anyhow::Result<url::Url> {
    let u = url::Url::parse(raw).map_err(|_| anyhow::anyhow!("端点 URL 无效"))?;
    ensure!(
        u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none()
            && u.query().is_none()
            && u.fragment().is_none(),
        "端点不能包含凭据、查询参数或片段"
    );
    let loopback = matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    ensure!(
        u.scheme() == "wss" || (u.scheme() == "ws" && loopback),
        "远程语音端点必须使用 wss"
    );
    Ok(u)
}
pub(crate) fn custom_endpoint(provider: Provider) -> anyhow::Result<url::Url> {
    endpoint(&env(if provider == Provider::CustomDashScope {
        "CUSTOM_DASHSCOPE_URL"
    } else {
        "CUSTOM_OPENAI_URL"
    })?)
}
pub(crate) fn azure_endpoint() -> anyhow::Result<url::Url> {
    azure_url(&env("AZURE_OPENAI_ENDPOINT")?)
}
pub(crate) fn azure_url(raw: &str) -> anyhow::Result<url::Url> {
    let mut u = url::Url::parse(raw).map_err(|_| anyhow::anyhow!("Azure 资源端点无效"))?;
    ensure!(
        u.scheme() == "https" && matches!(u.path(), "" | "/"),
        "Azure 端点应为 HTTPS 资源根地址"
    );
    let host = u.host_str().context("Azure 端点缺少主机")?;
    ensure!(
        [".openai.azure.com", ".openai.azure.cn", ".openai.azure.us"]
            .iter()
            .any(|suffix| host.ends_with(suffix)),
        "请使用 Azure OpenAI 资源域名"
    );
    u.set_scheme("wss")
        .map_err(|_| anyhow::anyhow!("Azure 端点无效"))?;
    endpoint(u.as_str())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_urls_cannot_leak_keys_or_use_cleartext_remote_connections() {
        for u in [
            "ws://example.com/speech",
            "wss://user:pass@example.com/ws",
            "wss://example.com/ws?key=secret",
            "wss://example.com/ws#key",
            "https://example.com",
        ] {
            assert!(endpoint(u).is_err());
        }
        for u in [
            "wss://example.com/ws",
            "ws://127.0.0.1:8080/ws",
            "ws://[::1]:8080/ws",
        ] {
            assert!(endpoint(u).is_ok());
        }
    }
    #[test]
    fn catalog_has_unique_ids() {
        let ids: std::collections::HashSet<_> = Provider::ALL.iter().map(|p| p.id()).collect();
        assert_eq!(ids.len(), 13);
    }
}
