//! Integrity-checked streaming transducers and explicitly segmented offline models.
mod offline;
mod whisper;
use crate::{Config, Continuity, Shared, Transcript};
use anyhow::{Context as _, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use subveil_audio::FrameReceiver;
pub use whisper::{WhisperEngine, WhisperResult};

pub use subveil_models::ModelKind;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    path: PathBuf,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    #[serde(default)]
    kind: ModelKind,
    language: String,
    #[serde(default)]
    languages: Vec<String>,
    #[serde(default)]
    sample_rate: u32,
    source: String,
    license: String,
    encoder: Option<Artifact>,
    decoder: Option<Artifact>,
    joiner: Option<Artifact>,
    model: Option<Artifact>,
    tokens: Option<Artifact>,
}
impl Manifest {
    fn read(path: &Path) -> anyhow::Result<Self> {
        ensure!(
            std::fs::metadata(path)?.is_file() && std::fs::metadata(path)?.len() <= 65_536,
            "模型清单超过 64 KiB"
        );
        let mut manifest: Self =
            toml::from_str(&std::fs::read_to_string(path)?).context("模型清单 TOML 无效")?;
        if manifest.languages.is_empty() {
            manifest.languages.push(manifest.language.clone());
        }
        ensure!(
            manifest.languages.len() <= 3
                && manifest
                    .languages
                    .iter()
                    .all(|lang| matches!(lang.as_str(), "en" | "ja" | "zh"))
                && manifest.languages.contains(&manifest.language),
            "模型语种必须来自 en / ja / zh"
        );
        match manifest.kind {
            ModelKind::StreamingTransducer => ensure!(
                manifest.tokens.is_some()
                    && manifest.encoder.is_some()
                    && manifest.decoder.is_some()
                    && manifest.joiner.is_some()
                    && manifest.model.is_none(),
                "Streaming 模型缺少 encoder / decoder / joiner"
            ),
            ModelKind::SenseVoice => ensure!(
                manifest.model.is_some()
                    && manifest.tokens.is_some()
                    && manifest.encoder.is_none()
                    && manifest.decoder.is_none()
                    && manifest.joiner.is_none()
                    && manifest.sample_rate == 16000,
                "SenseVoice 需要 16 kHz 模型与 tokens"
            ),
            ModelKind::QwenTranslation => ensure!(
                manifest.model.is_some()
                    && manifest.encoder.is_none()
                    && manifest.decoder.is_none()
                    && manifest.joiner.is_none(),
                "翻译模型需要 GGUF 文件"
            ),
            ModelKind::Whisper => ensure!(
                manifest.model.is_some()
                    && manifest.tokens.is_none()
                    && manifest.encoder.is_none()
                    && manifest.decoder.is_none()
                    && manifest.joiner.is_none()
                    && manifest.sample_rate == 16000,
                "Whisper 需要 16 kHz GGML 模型文件"
            ),
        }
        ensure!(
            manifest.kind == ModelKind::QwenTranslation
                || (8_000..=48_000).contains(&manifest.sample_rate),
            "模型采样率无效"
        );
        ensure!(
            !manifest.source.trim().is_empty()
                && !manifest.license.trim().is_empty()
                && manifest.source.len() <= 2048
                && manifest.license.len() <= 2048,
            "模型必须声明来源和许可证"
        );
        Ok(manifest)
    }
}

/// User-declared metadata displayed before loading any native model.
#[non_exhaustive]
pub struct ModelMetadata {
    /// Backend and transcript semantics.
    pub kind: ModelKind,
    /// Source language hints supported by this package.
    pub languages: Vec<String>,
    /// Input sample rate required by this model.
    pub sample_rate: u32,
    /// Supported source language.
    pub language: String,
    /// Model origin supplied by its owner.
    pub source: String,
    /// Model license supplied by its owner.
    pub license: String,
}
impl ModelMetadata {
    /// Metadata from a validated curated catalog entry, without filesystem I/O.
    pub fn from_catalog(model: &crate::models::Model) -> Self {
        Self {
            kind: model.kind,
            languages: model.languages.clone(),
            sample_rate: model.sample_rate,
            language: model.languages.first().cloned().unwrap_or_default(),
            source: model.source(),
            license: format!("{} — {}", model.license, model.license_url),
        }
    }
}
/// Read lightweight model metadata; model hashing and loading happen in the worker.
pub fn model_metadata(path: &Path) -> anyhow::Result<ModelMetadata> {
    let m = Manifest::read(path)?;
    Ok(ModelMetadata {
        kind: m.kind,
        languages: m.languages,
        sample_rate: m.sample_rate,
        language: m.language,
        source: m.source,
        license: m.license,
    })
}

fn verify(
    root: &Path,
    artifact: &Artifact,
    cancelled: impl Fn() -> bool,
) -> anyhow::Result<String> {
    ensure!(
        artifact.sha256.len() == 64 && artifact.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
        "模型 SHA-256 无效"
    );
    let path = root.join(&artifact.path);
    let path_string = path.to_str().context("模型路径必须为 UTF-8")?;
    ensure!(!path_string.contains('\0'), "模型路径包含 NUL");
    ensure!(std::fs::metadata(&path)?.is_file(), "模型路径不是普通文件");
    let mut file = std::fs::File::open(&path).context("无法读取模型文件")?;
    ensure!(
        file.metadata()?.is_file() && file.metadata()?.len() > 0,
        "模型文件为空或不是普通文件"
    );
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 65_536];
    loop {
        ensure!(!cancelled(), "模型加载已取消");
        let len = file.read(&mut buffer)?;
        if len == 0 {
            break;
        }
        digest.update(&buffer[..len]);
    }
    ensure!(
        format!("{:x}", digest.finalize()).eq_ignore_ascii_case(&artifact.sha256),
        "模型 SHA-256 不匹配"
    );
    Ok(path_string.into())
}

pub(crate) fn run(
    config: &Config,
    session: u64,
    frames: &FrameReceiver,
    shared: &Shared,
) -> anyhow::Result<()> {
    let path = config
        .local_model
        .as_deref()
        .context("请导入本地模型清单")?;
    shared.status("校验并加载本地模型…", frames, false);
    let manifest = Manifest::read(path)?;
    ensure!(
        manifest.kind != ModelKind::QwenTranslation,
        "文字翻译模型不能用于语音识别"
    );
    ensure!(
        manifest.sample_rate == config.sample_rate && manifest.languages.contains(&config.language),
        "模型采样率或语种与会话不一致"
    );
    let root = path.parent().context("模型清单缺少目录")?;
    if manifest.kind == ModelKind::Whisper {
        return whisper::run(config, session, frames, shared);
    }
    if manifest.kind == ModelKind::SenseVoice {
        return offline::run(config, session, frames, shared, &manifest, root);
    }
    let mut settings = sherpa_onnx::OnlineRecognizerConfig::default();
    settings.feat_config.sample_rate = manifest.sample_rate as i32;
    settings.enable_endpoint = true;
    settings.decoding_method = Some("greedy_search".into());
    settings.rule1_min_trailing_silence =
        2.4_f32.max(config.vad.trailing_silence_ms as f32 / 1000.);
    settings.rule2_min_trailing_silence = config.vad.trailing_silence_ms as f32 / 1000.;
    settings.rule3_min_utterance_length = config.vad.max_segment_ms as f32 / 1000.;
    settings.model_config.num_threads = 2;
    settings.model_config.transducer.encoder = Some(verify(
        root,
        manifest.encoder.as_ref().context("缺少 encoder")?,
        || shared.stopped(),
    )?);
    settings.model_config.transducer.decoder = Some(verify(
        root,
        manifest.decoder.as_ref().context("缺少 decoder")?,
        || shared.stopped(),
    )?);
    settings.model_config.transducer.joiner = Some(verify(
        root,
        manifest.joiner.as_ref().context("缺少 joiner")?,
        || shared.stopped(),
    )?);
    settings.model_config.tokens = Some(verify(
        root,
        manifest.tokens.as_ref().context("缺少 tokens")?,
        || shared.stopped(),
    )?);
    ensure!(!shared.stopped(), "模型加载已取消");
    let recognizer = sherpa_onnx::OnlineRecognizer::create(&settings)
        .context("无法加载 streaming transducer；检查 ONNX 与 tokens 兼容性")?;
    let stream = recognizer.create_stream();
    let mut transcript = Transcript::new(session, &config.language);
    let mut continuity = Continuity::default();
    let mut start = None;
    let mut last = Instant::now();
    let mut timed_out = false;
    // Discard audio captured during loading; never build an inference backlog.
    while frames.pop().is_some() {}
    shared.status(
        "本地 streaming ASR 已就绪 · CPU · 不上传音频",
        frames,
        false,
    );
    while !shared.stopped() {
        if let Some(frame) = frames.pop() {
            if !continuity.accept(&frame, config.sample_rate) {
                recognizer.reset(&stream);
                transcript.reset();
                start = None;
                shared.status("音频不连续；已重置本地识别", frames, true);
                continue;
            }
            last = Instant::now();
            timed_out = false;
            let begin = *start.get_or_insert(frame.sample_offset);
            let end = frame.sample_offset.saturating_add(frame.len as u64);
            stream.accept_waveform(frame.rate as i32, &frame.samples[..frame.len]);
            while recognizer.is_ready(&stream) && !shared.stopped() {
                recognizer.decode(&stream);
            }
            if shared.stopped() {
                break;
            }
            let final_ = recognizer.is_endpoint(&stream)
                || end.saturating_sub(begin) * 1000
                    >= u64::from(frame.rate) * u64::from(config.vad.max_segment_ms);
            let result = recognizer.get_result(&stream).context("本地识别结果无效")?;
            if let Some(event) = transcript.update(
                result.text,
                begin * 1000 / u64::from(frame.rate),
                end * 1000 / u64::from(frame.rate),
                final_,
            ) {
                shared.emit(event);
            }
            if final_ {
                recognizer.reset(&stream);
                start = None;
            }
            shared.status("本地 streaming ASR · CPU · 不上传音频", frames, false);
        } else {
            if !timed_out && last.elapsed() >= Duration::from_secs(2) {
                recognizer.reset(&stream);
                transcript.reset();
                start = None;
                timed_out = true;
                shared.status("等待音频；本地上下文已重置", frames, true);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrupt_missing_and_cancelled_models_fail_without_inference() -> anyhow::Result<()> {
        let dir = std::env::temp_dir().join(format!("subveil-model-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("tokens"), b"test")?;
        let mut artifact = Artifact {
            path: "tokens".into(),
            sha256: format!("{:x}", Sha256::digest(b"test")),
        };
        assert!(verify(&dir, &artifact, || false).is_ok());
        assert!(verify(&dir, &artifact, || true).is_err());
        artifact.sha256 = "0".repeat(64);
        assert!(verify(&dir, &artifact, || false).is_err());
        artifact.path = "missing".into();
        assert!(verify(&dir, &artifact, || false).is_err());
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
    #[test]
    fn local_load_failure_is_reported_without_cloud_consent_or_fallback() -> anyhow::Result<()> {
        let (_sender, frames) = FrameReceiver::channel();
        let worker = crate::Recognition::start(
            Config {
                provider: crate::Provider::Local,
                local_model: Some(std::env::temp_dir().join("subveil-nonexistent-manifest.toml")),
                allow_cloud: false,
                ..Default::default()
            },
            1,
            frames,
        )?;
        let limit = Instant::now() + Duration::from_secs(1);
        while !worker.is_finished() && Instant::now() < limit {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(worker.is_finished());
        assert!(
            worker
                .snapshot()
                .is_some_and(|s| s.status.starts_with("识别失败"))
        );
        assert!(worker.pop().is_none());
        worker.join()?;
        Ok(())
    }
}
