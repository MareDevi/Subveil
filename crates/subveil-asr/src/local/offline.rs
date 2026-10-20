//! SenseVoice is offline inference over bounded energy-VAD segments, never streaming ASR.
use super::{Manifest, verify};
use crate::{
    Config, Continuity, Shared, Transcript,
    vad::{Boundary, Vad},
};
use anyhow::{Context as _, ensure};
use std::{
    collections::VecDeque,
    path::Path,
    thread,
    time::{Duration, Instant},
};
use subveil_audio::FrameReceiver;

struct SegmentAudio {
    start: u64,
    end: u64,
    samples: Vec<f32>,
}
struct Segmenter {
    vad: Vad,
    rate: u32,
    capacity: usize,
    samples: VecDeque<f32>,
}
impl Segmenter {
    fn new(config: &Config) -> Self {
        let capacity = (u64::from(config.vad.max_segment_ms + config.vad.pre_roll_ms)
            * u64::from(config.sample_rate)
            / 1000) as usize
            + 2048;
        Self {
            vad: Vad::new(config.vad.clone()),
            rate: config.sample_rate,
            capacity,
            samples: VecDeque::new(),
        }
    }
    fn reset(&mut self) {
        self.vad.reset();
        self.samples.clear();
    }
    fn accept(&mut self, samples: &[f32], offset: u64) -> Option<SegmentAudio> {
        self.samples
            .extend(samples.iter().map(|v| if v.is_finite() { *v } else { 0. }));
        while self.samples.len() > self.capacity {
            self.samples.pop_front();
        }
        match self.vad.accept(samples, self.rate, offset) {
            Boundary::Speech {
                start,
                end,
                final_: true,
            } => {
                let retained_start = end.saturating_sub(self.samples.len() as u64);
                let start = start
                    .max(retained_start)
                    .max(end.saturating_sub(u64::from(self.rate) * 30));
                let output = self
                    .samples
                    .iter()
                    .skip((start - retained_start) as usize)
                    .copied()
                    .collect();
                self.samples.clear();
                Some(SegmentAudio {
                    start,
                    end,
                    samples: output,
                })
            }
            Boundary::Rejected => {
                self.samples.clear();
                None
            }
            _ => None,
        }
    }
}

pub(super) fn run(
    config: &Config,
    session: u64,
    frames: &FrameReceiver,
    shared: &Shared,
    manifest: &Manifest,
    root: &Path,
) -> anyhow::Result<()> {
    let mut settings = sherpa_onnx::OfflineRecognizerConfig::default();
    settings.feat_config.sample_rate = config.sample_rate as i32;
    settings.model_config.num_threads = 4;
    settings.model_config.provider = Some("cpu".into());
    settings.model_config.sense_voice.model = Some(verify(
        root,
        manifest.model.as_ref().context("SenseVoice 缺少 model")?,
        || shared.stopped(),
    )?);
    settings.model_config.sense_voice.language = Some(config.language.clone());
    settings.model_config.sense_voice.use_itn = true;
    settings.model_config.tokens = Some(verify(root, &manifest.tokens, || shared.stopped())?);
    ensure!(!shared.stopped(), "模型加载已取消");
    let recognizer = sherpa_onnx::OfflineRecognizer::create(&settings)
        .context("无法加载 SenseVoice ONNX / tokens")?;
    let mut segmenter = Segmenter::new(config);
    let mut continuity = Continuity::default();
    let mut transcript = Transcript::new(session, &config.language);
    let mut last = Instant::now();
    let mut timed_out = false;
    while frames.pop().is_some() {}
    shared.status(
        "本地 SenseVoice 已就绪 · CPU · 离线分段（仅 Final）",
        frames,
        false,
    );
    while !shared.stopped() {
        if let Some(frame) = frames.pop() {
            if !continuity.accept(&frame, config.sample_rate) {
                segmenter.reset();
                transcript.reset();
                shared.status("音频不连续；已重置本地分段", frames, true);
                continue;
            }
            last = Instant::now();
            timed_out = false;
            if let Some(audio) = segmenter.accept(&frame.samples[..frame.len], frame.sample_offset)
            {
                let stream = recognizer.create_stream();
                stream.accept_waveform(config.sample_rate as i32, &audio.samples);
                let inference = Instant::now();
                recognizer.decode(&stream);
                if shared.stopped() {
                    break;
                }
                let result = stream.get_result().context("SenseVoice 结果无效")?;
                let inference_ms = inference.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
                let audio_ms = audio.samples.len() as u64 * 1000 / u64::from(config.sample_rate);
                if let Some(event) = transcript.update(
                    result.text,
                    audio.start * 1000 / u64::from(config.sample_rate),
                    audio.end * 1000 / u64::from(config.sample_rate),
                    true,
                ) {
                    shared.emit(event);
                }
                shared.status(
                    format!(
                        "本地 SenseVoice · 离线分段 · 推理 {inference_ms} ms / 音频 {audio_ms} ms"
                    ),
                    frames,
                    false,
                );
                if let Ok(mut snapshot) = shared.snapshot.lock() {
                    snapshot.local_inference_ms = inference_ms;
                    snapshot.local_audio_ms = audio_ms;
                }
            }
        } else {
            if !timed_out && last.elapsed() >= Duration::from_secs(2) {
                segmenter.reset();
                transcript.reset();
                timed_out = true;
                shared.status("等待音频；SenseVoice 分段已重置", frames, true);
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
    fn bounded_segmenter_retains_preroll_and_emits_only_finished_speech() {
        let config = Config {
            sample_rate: 1000,
            ..Default::default()
        };
        let mut segmenter = Segmenter::new(&config);
        assert!(segmenter.accept(&[0.; 200], 0).is_none());
        assert!(segmenter.accept(&[0.1; 200], 200).is_none());
        let final_ = segmenter.accept(&[0.; 600], 400);
        assert!(final_.is_some_and(|audio| audio.start == 0
            && audio.end == 1000
            && audio.samples.len() == 1000));
        for offset in (1000..100000).step_by(1000) {
            segmenter.accept(&[0.; 1000], offset);
        }
        assert!(segmenter.samples.len() <= segmenter.capacity);
        segmenter.reset();
        assert!(segmenter.samples.is_empty());
    }
}
