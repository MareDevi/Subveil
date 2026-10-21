//! OpenAI-compatible text model discovery using explicit settings only.
use anyhow::ensure;
use serde_json::Value;
use subveil_core::services::ServiceSettings;

/// Query `/models` beside a configured `/chat/completions` endpoint, without sending text.
/// A listed ID is not proof that the model supports Chat Completions or translation.
pub async fn text_models(settings: &ServiceSettings) -> anyhow::Result<Vec<String>> {
    let raw = settings.get("SUBVEIL_TRANSLATE_URL")?;
    let mut url = super::http::endpoint(&raw)?;
    let prefix = url
        .path()
        .strip_suffix("/chat/completions")
        .ok_or_else(|| {
            anyhow::anyhow!("自动发现需要以 /chat/completions 结尾的端点；其他服务请手填模型 ID")
        })?
        .to_owned();
    url.set_path(&format!("{prefix}/models"));
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| anyhow::anyhow!("无法创建模型目录客户端"))?;
    let mut response = client
        .get(url)
        .bearer_auth(settings.get("SUBVEIL_TRANSLATE_KEY")?)
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("文字模型目录连接失败或超时"))?;
    ensure!(
        response.status().is_success(),
        "文字模型目录返回 HTTP {}",
        response.status().as_u16()
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("文字模型目录读取失败"))?
    {
        ensure!(
            bytes.len().saturating_add(chunk.len()) <= 1_048_576,
            "文字模型目录超过 1 MiB"
        );
        bytes.extend_from_slice(&chunk);
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("文字模型目录 JSON 无效"))?;
    parse(&value)
}
fn parse(value: &Value) -> anyhow::Result<Vec<String>> {
    let rows = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("文字模型目录结构无效"))?;
    ensure!(rows.len() <= 4096, "文字模型目录条目过多");
    let mut models = rows
        .iter()
        .filter_map(|row| row.get("id").and_then(Value::as_str))
        .filter(|id| !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    ensure!(!models.is_empty(), "文字模型目录没有有效模型 ID");
    Ok(models)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_catalog_rejects_invalid_ids_and_deduplicates() -> anyhow::Result<()> {
        assert_eq!(
            parse(
                &serde_json::json!({"data":[{"id":"model-a"},{"id":"model-a"},{"id":"bad\nkey"},{"id":""}]})
            )?,
            ["model-a"]
        );
        assert!(parse(&serde_json::json!({"error":"private service error"})).is_err());
        Ok(())
    }
}
