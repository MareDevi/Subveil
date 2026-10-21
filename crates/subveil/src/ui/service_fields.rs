//! Provider-specific credential form schema; settings use existing protocol names.
use subveil_asr::Provider;

pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub secret: bool,
}
pub fn fields(provider: Provider) -> &'static [Field] {
    use Provider::*;
    match provider {
        Deepgram => &[Field {
            key: "DEEPGRAM_API_KEY",
            label: "API Key",
            secret: true,
        }],
        Alibaba => &[
            Field {
                key: "DASHSCOPE_API_KEY",
                label: "API Key",
                secret: true,
            },
            Field {
                key: "DASHSCOPE_WORKSPACE_ID",
                label: "北京业务空间 ID",
                secret: false,
            },
        ],
        Gemini => &[Field {
            key: "GEMINI_API_KEY",
            label: "API Key",
            secret: true,
        }],
        Volcano => &[Field {
            key: "VOLCENGINE_API_KEY",
            label: "语音控制台 API Key",
            secret: true,
        }],
        Tencent => &[
            Field {
                key: "TENCENT_APP_ID",
                label: "AppID",
                secret: false,
            },
            Field {
                key: "TENCENT_SECRET_ID",
                label: "SecretId",
                secret: true,
            },
            Field {
                key: "TENCENT_SECRET_KEY",
                label: "SecretKey",
                secret: true,
            },
        ],
        Baidu => &[
            Field {
                key: "BAIDU_APP_ID",
                label: "机器翻译 AppID",
                secret: false,
            },
            Field {
                key: "BAIDU_API_KEY",
                label: "机器翻译 API Key",
                secret: true,
            },
        ],
        OpenAi => &[Field {
            key: "OPENAI_API_KEY",
            label: "API Key",
            secret: true,
        }],
        AzureOpenAi => &[
            Field {
                key: "AZURE_OPENAI_ENDPOINT",
                label: "HTTPS 资源端点",
                secret: false,
            },
            Field {
                key: "AZURE_OPENAI_API_KEY",
                label: "API Key",
                secret: true,
            },
            Field {
                key: "AZURE_OPENAI_DEPLOYMENT",
                label: "实时翻译部署名",
                secret: false,
            },
            Field {
                key: "AZURE_OPENAI_TRANSCRIPTION_DEPLOYMENT",
                label: "实时转写部署名",
                secret: false,
            },
        ],
        Xai => &[Field {
            key: "XAI_API_KEY",
            label: "API Key",
            secret: true,
        }],
        CustomDashScope => &[
            Field {
                key: "CUSTOM_DASHSCOPE_URL",
                label: "WebSocket 端点",
                secret: false,
            },
            Field {
                key: "CUSTOM_DASHSCOPE_MODEL",
                label: "识别模型 ID",
                secret: false,
            },
            Field {
                key: "CUSTOM_DASHSCOPE_API_KEY",
                label: "API Key",
                secret: true,
            },
        ],
        CustomOpenAi => &[
            Field {
                key: "CUSTOM_OPENAI_URL",
                label: "WebSocket 端点",
                secret: false,
            },
            Field {
                key: "CUSTOM_OPENAI_MODEL",
                label: "转写模型 ID",
                secret: false,
            },
            Field {
                key: "CUSTOM_OPENAI_API_KEY",
                label: "API Key",
                secret: true,
            },
        ],
        _ => &[],
    }
}
pub fn translation_fields(provider: subveil_translate::Provider) -> &'static [Field] {
    match provider {
        subveil_translate::Provider::Local => &[Field {
            key: "SUBVEIL_TRANSLATE_GLOSSARY",
            label: "术语表（原词=译词；分号分隔）",
            secret: false,
        }],
        subveil_translate::Provider::OpenAi => &[
            Field {
                key: "SUBVEIL_TRANSLATE_GLOSSARY",
                label: "术语表（原词=译词；分号分隔）",
                secret: false,
            },
            Field {
                key: "SUBVEIL_TRANSLATE_URL",
                label: "Chat Completions 完整端点",
                secret: false,
            },
            Field {
                key: "SUBVEIL_TRANSLATE_KEY",
                label: "翻译 API Key",
                secret: true,
            },
            Field {
                key: "SUBVEIL_TRANSLATE_MODEL",
                label: "翻译模型 ID",
                secret: false,
            },
        ],
        subveil_translate::Provider::DeepL => &[
            Field {
                key: "DEEPL_API_KEY",
                label: "DeepL API Key",
                secret: true,
            },
            Field {
                key: "SUBVEIL_DEEPL_URL",
                label: "DeepL 端点（留空使用 Free API）",
                secret: false,
            },
        ],
        _ => &[],
    }
}
pub fn console(provider: Provider) -> &'static str {
    match provider {
        Provider::Alibaba => "https://bailian.console.aliyun.com/",
        Provider::Gemini => "https://aistudio.google.com/apikey",
        Provider::Volcano => "https://console.volcengine.com/speech/",
        Provider::Tencent => "https://console.cloud.tencent.com/cam/capi",
        Provider::Baidu => "https://console.bce.baidu.com/ai/",
        Provider::OpenAi => "https://platform.openai.com/api-keys",
        Provider::AzureOpenAi => "https://portal.azure.com/",
        Provider::Xai => "https://console.x.ai/",
        Provider::Deepgram => "https://console.deepgram.com/",
        _ => "https://developers.openai.com/api/docs/guides/realtime-transcription",
    }
}
