//! SenseVoice is offline inference over bounded energy-VAD segments, never streaming ASR.
use super::{Manifest, verify};
use crate::{
    Config, Continuity, Shared, Transcript,
    vad::{Boundary, Vad},
};
use anyhow::{Context as _, ensure};
use crossbeam_queue::ArrayQueue;
use std::{
    collections::VecDeque,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};
use subveil_audio::FrameReceiver;

pub(super) struct SegmentAudio {
    pub(super) start: u64,
    pub(super) end: u64,
    pub(super) samples: Vec<f32>,
}
pub(super) struct Segmenter {
    vad: Vad,
    rate: u32,
    capacity: usize,
    samples: VecDeque<f32>,
}
impl Segmenter {
    pub(super) fn new(config: &Config) -> Self {
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
    pub(super) fn reset(&mut self) {
        self.vad.reset();
        self.samples.clear();
    }
    pub(super) fn accept(&mut self, samples: &[f32], offset: u64) -> Option<SegmentAudio> {
        self.samples
            .extend(samples.iter().map(|v| if v.is_finite() { *v } else { 0. }));
        while self.samples.len() > self.capacity {
            self.samples.pop_front();
        }
        self.vad.retain_from(
            offset
                .saturating_add(samples.len() as u64)
                .saturating_sub(self.samples.len() as u64),
        );
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
    settings.model_config.tokens = Some(verify(
        root,
        manifest.tokens.as_ref().context("缺少 tokens")?,
        || shared.stopped(),
    )?);
    ensure!(!shared.stopped(), "模型加载已取消");
    let recognizer = sherpa_onnx::OfflineRecognizer::create(&settings)
        .context("无法加载 SenseVoice ONNX / tokens")?;
    run_loaded(config, session, frames, shared, move |audio| {
        let stream = recognizer.create_stream();
        stream.accept_waveform(config.sample_rate as i32, &audio.samples);
        recognizer.decode(&stream);
        Ok(stream.get_result().context("SenseVoice 结果无效")?.text)
    })
}

struct Job {
    epoch: u64,
    audio: SegmentAudio,
    queued: Instant,
}
struct Completed {
    job: Job,
    text: String,
    inference_ms: u64,
}
fn dropped_segment(shared: &Shared, audio: &SegmentAudio, rate: u32) {
    if let Ok(mut snapshot) = shared.snapshot.lock() {
        snapshot.dropped_segments += 1;
        let start_ms = audio.start * 1000 / u64::from(rate);
        let end_ms = audio.end * 1000 / u64::from(rate);
        snapshot.dropped_segment_ms += end_ms.saturating_sub(start_ms);
        snapshot.last_dropped_segment = Some((start_ms, end_ms));
    }
}

// The injected decoder keeps the real PCM consumer testable without models or hardware.
fn run_loaded(
    config: &Config,
    session: u64,
    frames: &FrameReceiver,
    shared: &Shared,
    mut decode_audio: impl FnMut(&SegmentAudio) -> anyhow::Result<String> + Send,
) -> anyhow::Result<()> {
    let jobs = ArrayQueue::<Job>::new(2);
    let results = ArrayQueue::<Result<Completed, String>>::new(2);
    let cancel = AtomicBool::new(false);
    let mut segmenter = Segmenter::new(config);
    let mut continuity = Continuity::default();
    let mut transcript = Transcript::new(session, &config.language);
    let mut last = Instant::now();
    let mut timed_out = false;
    let mut epoch = 0_u64;
    while frames.pop().is_some() {}
    shared.status(
        "本地 SenseVoice 已就绪 · CPU · 离线分段（仅 Final）",
        frames,
        false,
    );
    thread::scope(|scope| {
        let jobs = &jobs;
        let results = &results;
        let cancel = &cancel;
        let worker = scope.spawn(move || {
            while !shared.stopped() && !cancel.load(Ordering::Relaxed) {
                let Some(job) = jobs.pop() else {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                };
                if job.queued.elapsed() > Duration::from_secs(1) {
                    dropped_segment(shared, &job.audio, config.sample_rate);
                    continue;
                }
                let started = Instant::now();
                match decode_audio(&job.audio) {
                    Ok(text) => {
                        let completed = Completed {
                            job,
                            text,
                            inference_ms: started.elapsed().as_millis().min(u128::from(u64::MAX))
                                as u64,
                        };
                        if let Some(Ok(evicted)) = results.force_push(Ok(completed)) {
                            dropped_segment(shared, &evicted.job.audio, config.sample_rate);
                        }
                    }
                    Err(error) => {
                        if let Some(Ok(evicted)) = results.force_push(Err(error.to_string())) {
                            dropped_segment(shared, &evicted.job.audio, config.sample_rate);
                        }
                        break;
                    }
                }
            }
        });
        let result = (|| -> anyhow::Result<()> {
            while !shared.stopped() {
                while let Some(result) = results.pop() {
                    let completed = result.map_err(anyhow::Error::msg)?;
                    let audio = completed.job.audio;
                    if completed.job.epoch != epoch {
                        dropped_segment(shared, &audio, config.sample_rate);
                        continue;
                    }
                    let audio_ms =
                        audio.samples.len() as u64 * 1000 / u64::from(config.sample_rate);
                    if let Some(event) = transcript.update(
                        completed.text,
                        audio.start * 1000 / u64::from(config.sample_rate),
                        audio.end * 1000 / u64::from(config.sample_rate),
                        true,
                    ) {
                        shared.emit(event);
                    }
                    shared.status(
                        format!(
                            "本地 SenseVoice · 离线分段 · 推理 {} ms / 音频 {audio_ms} ms",
                            completed.inference_ms
                        ),
                        frames,
                        false,
                    );
                    if let Ok(mut snapshot) = shared.snapshot.lock() {
                        snapshot.local_inference_ms = completed.inference_ms;
                        snapshot.local_audio_ms = audio_ms;
                    }
                }
                ensure!(!worker.is_finished(), "SenseVoice 推理线程意外退出");
                if let Some(frame) = frames.pop() {
                    if !continuity.accept(&frame, config.sample_rate) {
                        segmenter.reset();
                        transcript.reset();
                        epoch = epoch.saturating_add(1);
                        while let Some(job) = jobs.pop() {
                            dropped_segment(shared, &job.audio, config.sample_rate);
                        }
                        shared.status("音频不连续；已重置本地分段", frames, true);
                        continue;
                    }
                    last = Instant::now();
                    timed_out = false;
                    if let Some(audio) =
                        segmenter.accept(&frame.samples[..frame.len], frame.sample_offset)
                        && let Some(evicted) = jobs.force_push(Job {
                            epoch,
                            audio,
                            queued: Instant::now(),
                        })
                    {
                        dropped_segment(shared, &evicted.audio, config.sample_rate);
                        shared.status("SenseVoice 推理过载；已丢弃最旧等待分段", frames, false);
                    }
                } else {
                    if !timed_out && last.elapsed() >= Duration::from_secs(2) {
                        segmenter.reset();
                        transcript.reset();
                        epoch = epoch.saturating_add(1);
                        timed_out = true;
                        while let Some(job) = jobs.pop() {
                            dropped_segment(shared, &job.audio, config.sample_rate);
                        }
                        shared.status("等待音频；SenseVoice 分段已重置", frames, true);
                    }
                    thread::sleep(Duration::from_millis(5));
                }
            }
            Ok(())
        })();
        cancel.store(true, Ordering::Relaxed);
        // Native decode has no cancellation API. Joining happens on the ASR worker, never GPUI.
        worker
            .join()
            .map_err(|_| anyhow::anyhow!("SenseVoice 推理线程异常退出"))?;
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slow_decode_keeps_consuming_pcm_and_reports_whole_segment_drops() -> anyhow::Result<()> {
        use std::sync::{Arc, Mutex, atomic::AtomicU64};
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            events: ArrayQueue::new(64),
            translations: ArrayQueue::new(64),
            snapshot: Mutex::new(crate::Snapshot::default()),
            dropped: AtomicU64::new(0),
        });
        let (sender, frames) = FrameReceiver::channel();
        let release = Arc::new(AtomicBool::new(false));
        let decoder_release = release.clone();
        let state = shared.clone();
        let worker = thread::spawn(move || {
            let mut config = Config::default();
            config.vad.max_segment_ms = 2_000;
            run_loaded(&config, 1, &frames, &state, move |_| {
                while !decoder_release.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(2));
                }
                Ok("test source".into())
            })
        });
        // Wait for loading-time PCM discard to finish before replaying frames.
        for _ in 0..100 {
            if shared
                .snapshot
                .lock()
                .is_ok_and(|snapshot| !snapshot.status.is_empty())
            {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        for sequence in 0..600 {
            sender.push(subveil_audio::AudioFrame {
                samples: [0.1; 2048],
                len: 320,
                generation: 1,
                sequence,
                sample_offset: sequence * 320,
                captured: Instant::now(),
                rate: 16_000,
            });
            thread::sleep(Duration::from_millis(2));
        }
        shared.stop.store(true, Ordering::Relaxed);
        release.store(true, Ordering::Relaxed);
        worker
            .join()
            .map_err(|_| anyhow::anyhow!("worker panic"))??;
        let snapshot = shared
            .snapshot
            .lock()
            .map_err(|_| anyhow::anyhow!("snapshot"))?;
        assert_eq!(snapshot.dropped_audio, 0);
        assert_eq!(snapshot.resets, 0);
        assert!(snapshot.dropped_segments >= 2);
        assert_eq!(
            snapshot.dropped_segment_ms,
            snapshot.dropped_segments * 2_000
        );
        assert!(
            snapshot
                .last_dropped_segment
                .is_some_and(|(start, end)| end - start == 2_000)
        );
        assert!(shared.events.is_empty());
        Ok(())
    }

    #[test]
    fn continuous_short_blocks_do_not_repeat_preroll_or_lose_samples() -> anyhow::Result<()> {
        let mut config = Config {
            sample_rate: 1000,
            ..Default::default()
        };
        config.vad.max_segment_ms = 2000;
        let mut segmenter = Segmenter::new(&config);
        let mut segments = Vec::new();
        for offset in (0..6000).step_by(20) {
            if let Some(audio) = segmenter.accept(&[0.1; 20], offset) {
                segments.push(audio);
            }
        }
        assert_eq!(segments.len(), 3);
        for (index, audio) in segments.iter().enumerate() {
            assert_eq!(
                (audio.start, audio.end, audio.samples.len()),
                (index as u64 * 2000, (index as u64 + 1) * 2000, 2000)
            );
        }
        Ok(())
    }
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
