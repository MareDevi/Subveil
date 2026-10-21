//! Credential-only model discovery; never sends audio or transcript text.
use crate::{Config, Provider};
use anyhow::{Context as _, ensure};
use serde_json::Value;
use std::time::Duration;

/// Protocol-compatible defaults, not account availability or inference verification.
pub fn defaults(provider: Provider) -> Vec<String> {
    let models: &[&str] = match provider {
        Provider::OpenAi => &["gpt-realtime-translate"],
        Provider::Gemini => &["gemini-3.5-live-translate-preview"],
        Provider::Alibaba => &["qwen3.8-livetranslate-flash-realtime"],
        Provider::Deepgram => &["nova-3", "nova-2"],
        Provider::Tencent => &["hunyuan-translation-lite"],
        Provider::Xai => &["grok-voice-latest"],
        _ => &[],
    };
    models.iter().map(|s| (*s).into()).collect()
}
/// Whether a documented model-list API is integrated for this speech protocol.
pub fn supported(provider: Provider) -> bool {
    matches!(
        provider,
        Provider::OpenAi | Provider::Gemini | Provider::Deepgram
    )
}
/// Fetch a bounded catalog and filter to the protocol this application implements.
pub async fn fetch(config: &Config) -> anyhow::Result<Vec<String>> {
    ensure!(
        supported(config.provider),
        "此服务使用固定协议或手填部署名，不提供已集成的模型发现接口"
    );
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let (url, header, auth) = match config.provider {
        Provider::OpenAi => (
            "https://api.openai.com/v1/models",
            "authorization",
            format!("Bearer {}", config.setting("OPENAI_API_KEY")?),
        ),
        Provider::Deepgram => (
            "https://api.deepgram.com/v1/models",
            "authorization",
            format!("Token {}", config.setting("DEEPGRAM_API_KEY")?),
        ),
        Provider::Gemini => (
            "https://generativelanguage.googleapis.com/v1beta/models",
            "x-goog-api-key",
            config.setting("GEMINI_API_KEY")?,
        ),
        _ => anyhow::bail!("此服务不支持模型发现"),
    };
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| anyhow::anyhow!("无法创建模型目录客户端"))?;
    let mut ids = Vec::new();
    let mut token = String::new();
    for _ in 0..8 {
        let mut endpoint = url::Url::parse(url)?;
        if config.provider == Provider::Gemini {
            endpoint.query_pairs_mut().append_pair("pageSize", "1000");
            if !token.is_empty() {
                endpoint.query_pairs_mut().append_pair("pageToken", &token);
            }
        }
        let request = client.get(endpoint).header(header, &auth);
        let mut response = request
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("模型目录连接失败或超时"))?;
        ensure!(
            response.status().is_success(),
            "模型目录返回 HTTP {}",
            response.status().as_u16()
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow::anyhow!("模型目录读取失败"))?
        {
            ensure!(
                bytes.len().saturating_add(chunk.len()) <= 1_048_576,
                "模型目录超过 1 MiB"
            );
            bytes.extend_from_slice(&chunk);
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("模型目录 JSON 无效"))?;
        ids.extend(parse(config.provider, &value)?);
        ensure!(ids.len() <= 4096, "模型目录条目过多");
        token = value
            .get("nextPageToken")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .into();
        if config.provider != Provider::Gemini || token.is_empty() {
            ids.sort();
            ids.dedup();
            ensure!(
                !ids.is_empty(),
                "目录未返回与当前语音协议兼容的模型；可保留官方默认项，但尚未验证账号权限"
            );
            return Ok(ids);
        }
        ensure!(token.len() <= 4096, "模型目录分页标识过长");
    }
    anyhow::bail!("模型目录超过分页上限")
}
fn parse(provider: Provider, value: &Value) -> anyhow::Result<Vec<String>> {
    let field = match provider {
        Provider::Gemini => "models",
        Provider::Deepgram => "stt",
        _ => "data",
    };
    let rows = value
        .get(field)
        .and_then(Value::as_array)
        .context("模型目录结构无效")?;
    ensure!(rows.len() <= 4096, "模型目录条目过多");
    let known = defaults(provider);
    let mut ids = Vec::new();
    for row in rows {
        let id = row
            .get(if provider == Provider::OpenAi {
                "id"
            } else {
                "name"
            })
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim_start_matches("models/");
        // Never route Flux (Listen v2) or generic realtime conversation models into these protocols.
        if known.iter().any(|s| s == id)
            && (provider != Provider::Deepgram
                || row.get("streaming").and_then(Value::as_bool) == Some(true))
        {
            ids.push(id.into());
        }
    }
    Ok(ids)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn discovery_filters_wrong_protocol_and_nonstreaming_models() -> anyhow::Result<()> {
        assert_eq!(
            parse(
                Provider::Deepgram,
                &json!({"stt":[{"name":"nova-3","streaming":true},{"name":"flux-general-en","streaming":true},{"name":"nova-2","streaming":false}]})
            )?,
            ["nova-3"]
        );
        assert_eq!(
            parse(
                Provider::OpenAi,
                &json!({"data":[{"id":"gpt-realtime"},{"id":"gpt-realtime-translate"}]})
            )?,
            ["gpt-realtime-translate"]
        );
        assert_eq!(
            parse(
                Provider::Gemini,
                &json!({"models":[{"name":"models/gemini-3.5-live-translate-preview"},{"name":"models/gemini-pro"}]})
            )?,
            ["gemini-3.5-live-translate-preview"]
        );
        assert!(parse(Provider::OpenAi, &json!({"error":"private response"})).is_err());
        Ok(())
    }
}
