use crate::{
    Config, Continuity, Provider, Shared, Transcript,
    vad::{Boundary, Vad},
};
use std::{
    thread,
    time::{Duration, Instant},
};
use subveil_audio::{AudioFrame, FrameReceiver};

pub(crate) fn validate() -> anyhow::Result<()> {
    #[cfg(not(feature = "sherpa"))]
    anyhow::bail!("本地 ASR 需要使用 --features sherpa 构建");
    #[cfg(feature = "sherpa")]
    {
        for name in [
            "SHERPA_ENCODER",
            "SHERPA_DECODER",
            "SHERPA_JOINER",
            "SHERPA_TOKENS",
        ] {
            let path = std::env::var(name)
                .map_err(|_| anyhow::anyhow!("请设置 {name} 为已验证的 streaming 模型文件"))?;
            anyhow::ensure!(
                !path.contains('\0') && std::path::Path::new(&path).is_file(),
                "{name} 必须指向有效文件"
            );
        }
        Ok(())
    }
}

trait Recognizer {
    fn accept(
        &mut self,
        frame: &AudioFrame,
        transcript: &mut Transcript,
        shared: &Shared,
    ) -> anyhow::Result<()>;
    fn reset(&mut self);
}

struct Mock {
    vad: Vad,
    language: String,
}
impl Recognizer for Mock {
    fn accept(
        &mut self,
        frame: &AudioFrame,
        transcript: &mut Transcript,
        shared: &Shared,
    ) -> anyhow::Result<()> {
        if let Boundary::Speech { start, end, final_ } =
            self.vad
                .accept(&frame.samples[..frame.len], frame.rate, frame.sample_offset)
        {
            let elapsed = (end - start) * 1_000 / u64::from(frame.rate);
            let text = match (self.language.as_str(), elapsed >= 800) {
                ("ja", false) => "[Mock] これは",
                ("ja", true) => "[Mock] これは音声認識のテストです。",
                (_, false) => "[Mock] This is",
                (_, true) => "[Mock] This is an audio-driven test.",
            };
            if let Some(event) = transcript.update(
                text.into(),
                start * 1_000 / u64::from(frame.rate),
                end * 1_000 / u64::from(frame.rate),
                final_,
            ) {
                shared.emit(event);
            }
        }
        Ok(())
    }
    fn reset(&mut self) {
        self.vad.reset();
    }
}

pub(crate) fn run(
    config: &Config,
    session: u64,
    frames: &FrameReceiver,
    shared: &Shared,
) -> anyhow::Result<()> {
    let mut recognizer: Box<dyn Recognizer> = match config.provider {
        Provider::Mock => Box::new(Mock {
            vad: Vad::new(config.vad.clone()),
            language: config.language.clone(),
        }),
        Provider::Sherpa => create_sherpa(config)?,
        Provider::Deepgram => anyhow::bail!("cloud provider requires WebSocket worker"),
    };
    let mut transcript = Transcript::new(session, &config.language);
    let mut continuity = Continuity::default();
    let mut last = Instant::now();
    let mut timed_out = false;
    while !shared.stopped() {
        if let Some(frame) = frames.pop() {
            if !continuity.accept(&frame, config.sample_rate) {
                recognizer.reset();
                transcript.reset();
                shared.status("音频不连续；已重置识别", frames, true);
                continue;
            }
            last = Instant::now();
            timed_out = false;
            recognizer.accept(&frame, &mut transcript, shared)?;
            shared.status(
                if config.provider == Provider::Mock {
                    "Mock ASR · 仅音频驱动样本文字"
                } else {
                    "本地 streaming ASR · 音频不上传"
                },
                frames,
                false,
            );
        } else {
            if !timed_out && last.elapsed() >= Duration::from_secs(2) {
                recognizer.reset();
                transcript.reset();
                timed_out = true;
                shared.status("等待音频；识别上下文已重置", frames, true);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    Ok(())
}

#[cfg(not(feature = "sherpa"))]
fn create_sherpa(_: &Config) -> anyhow::Result<Box<dyn Recognizer>> {
    anyhow::bail!("本地 ASR 需要使用 --features sherpa 构建")
}

#[cfg(feature = "sherpa")]
fn create_sherpa(config: &Config) -> anyhow::Result<Box<dyn Recognizer>> {
    use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig};
    validate()?;
    let mut settings = OnlineRecognizerConfig::default();
    settings.feat_config.sample_rate = config.sample_rate as i32;
    settings.model_config.transducer.encoder = Some(std::env::var("SHERPA_ENCODER")?);
    settings.model_config.transducer.decoder = Some(std::env::var("SHERPA_DECODER")?);
    settings.model_config.transducer.joiner = Some(std::env::var("SHERPA_JOINER")?);
    settings.model_config.tokens = Some(std::env::var("SHERPA_TOKENS")?);
    settings.model_config.num_threads = 2;
    settings.model_config.provider = Some("cpu".into());
    settings.model_config.debug = false;
    settings.enable_endpoint = true;
    settings.rule1_min_trailing_silence = config.vad.trailing_silence_ms as f32 / 1_000.;
    settings.rule2_min_trailing_silence = config.vad.trailing_silence_ms as f32 / 1_000.;
    settings.rule3_min_utterance_length = config.vad.max_segment_ms as f32 / 1_000.;
    settings.decoding_method = Some("greedy_search".into());
    let recognizer = OnlineRecognizer::create(&settings).ok_or_else(|| {
        anyhow::anyhow!("sherpa-onnx 无法加载 streaming 模型，请检查模型类型与文件")
    })?;
    let stream = recognizer.create_stream();
    Ok(Box::new(Sherpa {
        recognizer,
        stream,
        start: None,
    }))
}

#[cfg(feature = "sherpa")]
struct Sherpa {
    // Stream must be dropped before its recognizer.
    stream: sherpa_onnx::OnlineStream,
    recognizer: sherpa_onnx::OnlineRecognizer,
    start: Option<u64>,
}
#[cfg(feature = "sherpa")]
impl Recognizer for Sherpa {
    fn accept(
        &mut self,
        frame: &AudioFrame,
        transcript: &mut Transcript,
        shared: &Shared,
    ) -> anyhow::Result<()> {
        let start = *self.start.get_or_insert(frame.sample_offset);
        self.stream
            .accept_waveform(frame.rate as i32, &frame.samples[..frame.len]);
        while self.recognizer.is_ready(&self.stream) {
            if shared.stopped() {
                return Ok(());
            }
            self.recognizer.decode(&self.stream);
        }
        let final_ = self.recognizer.is_endpoint(&self.stream);
        if let Some(result) = self.recognizer.get_result(&self.stream)
            && let Some(event) = transcript.update(
                result.text,
                start * 1_000 / u64::from(frame.rate),
                (frame.sample_offset + frame.len as u64) * 1_000 / u64::from(frame.rate),
                final_,
            )
        {
            shared.emit(event);
        }
        if final_ {
            self.reset();
            if !transcript.previous.is_empty() {
                transcript.reset();
            }
        }
        Ok(())
    }
    fn reset(&mut self) {
        self.recognizer.reset(&self.stream);
        self.start = None;
    }
}
