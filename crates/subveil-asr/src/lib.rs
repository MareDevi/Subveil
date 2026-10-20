//! Bounded, cancellable ASR workers sharing the application's transcript model.
#![deny(missing_docs)]

mod cloud;
mod local;
mod vad;
pub use vad::VadConfig;

use crossbeam_queue::ArrayQueue;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use subveil_audio::{AudioFrame, FrameReceiver};
use subveil_core::subtitle::{SegmentKey, TranscriptEvent, TranscriptSegment};

/// Recognition backend. Cloud requires explicit per-start opt-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    /// Audio-driven deterministic text; never speech recognition.
    Mock,
    /// Deepgram Listen v1, over TLS.
    Deepgram,
    /// Local streaming transducer; requires the `sherpa` build feature.
    Sherpa,
}

/// Per-session settings; credentials are never retained here.
#[derive(Clone, Debug)]
pub struct Config {
    /// Chosen backend.
    pub provider: Provider,
    /// Explicit language: en or ja; local models must match.
    pub language: String,
    /// Capture rate, matching local model feature extraction.
    pub sample_rate: u32,
    /// Explicit permission to upload this session's audio.
    pub allow_cloud: bool,
    /// Energy segmentation settings.
    pub vad: VadConfig,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            provider: Provider::Mock,
            language: "en".into(),
            sample_rate: 16_000,
            allow_cloud: false,
            vad: VadConfig::default(),
        }
    }
}
impl Config {
    /// Validate settings and backend availability before capture starts.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(self.language.as_str(), "en" | "ja"),
            "ASR language must be en or ja"
        );
        anyhow::ensure!(
            (8_000..=48_000).contains(&self.sample_rate),
            "ASR rate must be 8000..=48000 Hz"
        );
        self.vad.validate()?;
        match self.provider {
            Provider::Deepgram => {
                anyhow::ensure!(
                    self.allow_cloud,
                    "cloud audio upload requires explicit opt-in"
                );
                cloud::credential()?;
            }
            Provider::Sherpa => local::validate()?,
            Provider::Mock => {}
        }
        Ok(())
    }
}

/// Worker diagnostics containing no transcript or credentials.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    /// State, including recoverable errors.
    pub status: String,
    /// Continuity resets or connection retries.
    pub resets: u64,
    /// PCM blocks evicted from recognition handoff.
    pub dropped_audio: u64,
    /// Transcript events evicted from result queue.
    pub dropped_events: u64,
}
struct Shared {
    stop: AtomicBool,
    events: ArrayQueue<TranscriptEvent>,
    snapshot: Mutex<Snapshot>,
    dropped: AtomicU64,
}
impl Shared {
    fn emit(&self, event: TranscriptEvent) {
        if self.events.force_push(event).is_some() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
    fn status(&self, status: impl Into<String>, frames: &FrameReceiver, reset: bool) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.status = status.into();
            snapshot.resets += u64::from(reset);
            snapshot.dropped_audio = frames.dropped_blocks();
            snapshot.dropped_events = self.dropped.load(Ordering::Relaxed);
        }
    }
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

/// Owned worker. Drop requests cancellation without joining the UI.
pub struct Recognition {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}
impl Recognition {
    /// Start recognition with immutable session identity and bounded PCM input.
    pub fn start(config: Config, session_id: u64, frames: FrameReceiver) -> anyhow::Result<Self> {
        config.validate()?;
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            events: ArrayQueue::new(64),
            snapshot: Mutex::new(Snapshot::default()),
            dropped: AtomicU64::new(0),
        });
        let state = shared.clone();
        let worker = thread::Builder::new()
            .name("subveil-asr".into())
            .spawn(move || {
                let result = match config.provider {
                    Provider::Deepgram => cloud::run(&config, session_id, &frames, &state),
                    _ => local::run(&config, session_id, &frames, &state),
                };
                match result {
                    Ok(()) => state.status("识别已停止", &frames, false),
                    Err(error) => state.status(format!("识别失败：{error}"), &frames, false),
                }
            })?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }
    /// Poll one result without waiting.
    pub fn pop(&self) -> Option<TranscriptEvent> {
        self.shared.events.pop()
    }
    /// Read diagnostics without blocking the UI.
    pub fn snapshot(&self) -> Option<Snapshot> {
        self.shared.snapshot.try_lock().ok().map(|s| s.clone())
    }
    /// Request cancellation.
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
    /// Whether worker termination completed.
    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }
    /// Join from a non-GUI thread, after requesting cancellation.
    pub fn join(mut self) -> anyhow::Result<()> {
        self.stop();
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| anyhow::anyhow!("ASR worker panicked"))?;
        }
        Ok(())
    }
}
impl Drop for Recognition {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Default)]
struct Continuity {
    last: Option<(u64, u64, u64, u32)>,
}
impl Continuity {
    fn accept(&mut self, frame: &AudioFrame, rate: u32) -> bool {
        let continuous = frame.len > 0
            && frame.len <= frame.samples.len()
            && frame.rate == rate
            && frame.captured.elapsed() <= Duration::from_millis(250)
            && self
                .last
                .is_none_or(|(generation, sequence, offset, old_rate)| {
                    generation == frame.generation
                        && sequence.checked_add(1) == Some(frame.sequence)
                        && offset == frame.sample_offset
                        && old_rate == frame.rate
                });
        self.last = Some((
            frame.generation,
            frame.sequence,
            frame.sample_offset.saturating_add(frame.len as u64),
            frame.rate,
        ));
        continuous
    }
}
struct Transcript {
    session: u64,
    segment: u64,
    revision: u32,
    language: String,
    previous: String,
    last_end_ms: u64,
}
impl Transcript {
    fn new(session: u64, language: &str) -> Self {
        Self {
            session,
            segment: 1,
            revision: 0,
            language: language.into(),
            previous: String::new(),
            last_end_ms: 0,
        }
    }
    fn reset(&mut self) {
        self.segment = self.segment.saturating_add(1);
        self.revision = 0;
        self.previous.clear();
        self.last_end_ms = 0;
    }
    fn update(
        &mut self,
        text: String,
        start_ms: u64,
        end_ms: u64,
        final_: bool,
    ) -> Option<TranscriptEvent> {
        if text.trim().is_empty() || text.len() > 16_384 || start_ms > end_ms {
            return None;
        }
        if !final_ && text == self.previous && end_ms.saturating_sub(self.last_end_ms) < 500 {
            return None;
        }
        self.revision = self.revision.saturating_add(1);
        self.previous.clone_from(&text);
        self.last_end_ms = end_ms;
        let segment = TranscriptSegment {
            key: SegmentKey {
                session_id: self.session,
                segment_id: self.segment,
                revision: self.revision,
            },
            text,
            language: Some(self.language.clone()),
            start_ms,
            end_ms,
        };
        if final_ {
            self.reset();
            Some(TranscriptEvent::Final(segment))
        } else {
            Some(TranscriptEvent::Partial(segment))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cloud_never_starts_without_explicit_consent() {
        assert!(
            Config {
                provider: Provider::Deepgram,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn japanese_revisions_replace_partial_then_stabilize() {
        let mut t = Transcript::new(7, "ja");
        let mut subtitles = subveil_core::subtitle::SubtitleState::new(7, 8, 8_000);
        for (text, final_) in [
            ("こん", false),
            ("こんにちは", false),
            ("こんにちは。", true),
        ] {
            assert!(
                t.update(text.into(), 0, 500, final_)
                    .is_some_and(|event| subtitles.apply(event, 1))
            );
        }
        assert_eq!(subtitles.history().len(), 1);
        assert!(
            subtitles
                .current()
                .is_some_and(|row| row.is_final && row.transcript.key.revision == 3)
        );
        assert!(
            t.update("次".into(), 500, 700, false)
                .is_some_and(|event| subtitles.apply(event, 2))
        );
        assert_eq!(subtitles.history().len(), 2);
    }
    #[test]
    fn continuity_detects_loss_reconnect_and_stale_audio() {
        let mut c = Continuity::default();
        let mut f = AudioFrame {
            samples: [0.; 2048],
            len: 100,
            generation: 1,
            sequence: 0,
            sample_offset: 0,
            captured: std::time::Instant::now(),
            rate: 16_000,
        };
        assert!(c.accept(&f, 16_000));
        f.sequence = 2;
        f.sample_offset = 200;
        assert!(!c.accept(&f, 16_000));
        f.generation = 2;
        f.sequence += 1;
        f.sample_offset += 100;
        assert!(!c.accept(&f, 16_000));
        f.captured -= Duration::from_secs(1);
        assert!(!c.accept(&f, 16_000));
    }
}

#[cfg(test)]
mod worker_tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn synthetic_audio_drives_mock_partial_final_and_cancels_without_device() -> anyhow::Result<()>
    {
        let (sender, frames) = FrameReceiver::channel();
        let worker = Recognition::start(Config::default(), 42, frames)?;
        for i in 0..10 {
            sender.push(AudioFrame {
                samples: [if i < 5 { 0.1 } else { 0. }; 2048],
                len: 2_048,
                generation: 1,
                sequence: i,
                sample_offset: i * 2_048,
                captured: Instant::now(),
                rate: 16_000,
            });
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut final_seen = false;
        let mut partial_seen = false;
        while Instant::now() < deadline && !final_seen {
            while let Some(event) = worker.pop() {
                match event {
                    TranscriptEvent::Partial(s) => {
                        partial_seen |= s.key.session_id == 42 && s.text.starts_with("[Mock]")
                    }
                    TranscriptEvent::Final(s) => {
                        final_seen |= s.key.session_id == 42 && s.key.revision > 1
                    }
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
        worker.join()?;
        assert!(partial_seen && final_seen);
        Ok(())
    }

    #[test]
    fn result_overload_discards_old_events_and_counts_loss() {
        let shared = Shared {
            stop: AtomicBool::new(false),
            events: ArrayQueue::new(64),
            snapshot: Mutex::new(Snapshot::default()),
            dropped: AtomicU64::new(0),
        };
        let mut transcript = Transcript::new(1, "en");
        for _ in 0..70 {
            if let Some(event) = transcript.update("test".into(), 0, 100, true) {
                shared.emit(event);
            }
        }
        assert_eq!(shared.dropped.load(Ordering::Relaxed), 6);
        assert!(
            matches!(shared.events.pop(), Some(TranscriptEvent::Final(s)) if s.key.segment_id == 7)
        );
    }

    #[test]
    fn unchanged_partial_refresh_keeps_long_segment_alive() {
        let mut transcript = Transcript::new(1, "en");
        let mut state = subveil_core::subtitle::SubtitleState::new(1, 8, 1_000);
        for time in (0..15_000).step_by(500) {
            state.expire(time);
            assert!(
                transcript
                    .update("still speaking".into(), 0, time, false)
                    .is_some_and(|event| state.apply(event, time))
            );
        }
        assert!(
            transcript
                .update("still speaking".into(), 0, 15_000, true)
                .is_some_and(|event| state.apply(event, 15_000))
        );
    }
}
