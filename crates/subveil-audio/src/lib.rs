//! Bounded local system-output capture. No recording or network access.

#![deny(missing_docs)]

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
mod linux;
mod pcm;

pub use pcm::{AudioFrame, FrameReceiver, FrameSender};

/// PipeWire node role, distinct from a reusable numeric object ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// One application's playback stream, copied without changing its routing.
    Playback,
    /// A particular output device's monitor.
    Output,
    /// A microphone or other explicit source node.
    Input,
}
/// One source lifetime. Server cookie and object serial prevent ID recycling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    /// Server lifetime identifier.
    pub server: u32,
    /// Object serial, never a recycled global ID.
    pub serial: u64,
    /// Underlying node role.
    pub kind: SourceKind,
    /// Human-readable source and stream description.
    pub label: String,
}
impl Source {
    /// Unique picker label, even when two tabs have the same application name.
    pub fn picker_label(&self) -> String {
        format!("{} · {}", self.label, self.serial)
    }
}
/// Explicit capture scope. A missing selected node never falls back to system mix.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CaptureTarget {
    /// Follow the default output monitor, as explicitly selected by the user.
    #[default]
    DefaultOutput,
    /// Capture this exact source lifetime.
    Node(Source),
}
/// Discover available source metadata without starting PCM capture.
pub fn sources() -> anyhow::Result<Vec<Source>> {
    #[cfg(target_os = "linux")]
    {
        linux::sources()
    }
    #[cfg(not(target_os = "linux"))]
    anyhow::bail!("音源选择当前需要 Linux PipeWire")
}

/// Local audio status, independent of any GUI window.
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// Human-readable state or recovery error.
    pub status: String,
    /// Exact default output node name, when available.
    pub device: Option<String>,
    /// Requested mono PCM rate after PipeWire resampling.
    pub sample_rate: u32,
    /// Root mean square amplitude, 0..=1.
    pub rms: f32,
    /// Peak amplitude, 0..=1.
    pub peak: f32,
    /// True below -60 dBFS or after 500 ms without PCM.
    pub silent: bool,
    /// Received PCM blocks, including subsequently dropped blocks.
    pub blocks: u64,
    /// Received mono samples, including subsequently dropped samples.
    pub samples: u64,
    /// Blocks discarded on overload, expiry, or stream replacement.
    pub dropped_blocks: u64,
    /// Mono samples discarded on overload, expiry, or stream replacement.
    pub dropped_samples: u64,
    /// Invalid PCM chunks rejected.
    pub invalid_chunks: u64,
    /// Number of successfully created monitor streams.
    pub connections: u64,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            status: "未采集".into(),
            device: None,
            sample_rate: 48_000,
            rms: 0.,
            peak: 0.,
            silent: true,
            blocks: 0,
            samples: 0,
            dropped_blocks: 0,
            dropped_samples: 0,
            invalid_chunks: 0,
            connections: 0,
        }
    }
}

/// Owned capture session. Dropping requests cancellation without blocking the UI.
pub struct Capture {
    stop: Arc<AtomicBool>,
    snapshot: Arc<Mutex<Snapshot>>,
    worker: Option<JoinHandle<()>>,
}

impl Capture {
    /// Starts local capture at a caller-selected rate (8–192 kHz).
    /// Errors if unsupported or if the worker cannot be created.
    pub fn start(sample_rate: u32) -> anyhow::Result<Self> {
        Self::start_inner(sample_rate, None, CaptureTarget::DefaultOutput)
    }

    /// Starts capture with a bounded, independent PCM consumer for recognition.
    pub fn start_with_frames(sample_rate: u32) -> anyhow::Result<(Self, FrameReceiver)> {
        let frames = FrameReceiver::new();
        let capture = Self::start_inner(
            sample_rate,
            Some(frames.clone()),
            CaptureTarget::DefaultOutput,
        )?;
        Ok((capture, frames))
    }
    /// Start an explicitly selected source with the same bounded PCM handoff.
    pub fn start_target(
        sample_rate: u32,
        target: CaptureTarget,
    ) -> anyhow::Result<(Self, FrameReceiver)> {
        let frames = FrameReceiver::new();
        let capture = Self::start_inner(sample_rate, Some(frames.clone()), target)?;
        Ok((capture, frames))
    }

    fn start_inner(
        sample_rate: u32,
        frames: Option<FrameReceiver>,
        target: CaptureTarget,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (8_000..=192_000).contains(&sample_rate),
            "sample rate must be 8000..=192000 Hz"
        );
        #[cfg(not(target_os = "linux"))]
        anyhow::bail!("system output capture currently requires Linux PipeWire");
        #[cfg(target_os = "linux")]
        {
            let stop = Arc::new(AtomicBool::new(false));
            let snapshot = Arc::new(Mutex::new(Snapshot {
                sample_rate,
                status: "连接 PipeWire…".into(),
                ..Snapshot::default()
            }));
            let worker_stop = stop.clone();
            let worker_snapshot = snapshot.clone();
            let worker = thread::Builder::new()
                .name("subveil-audio".into())
                .spawn(move || {
                    let mut stats = Snapshot {
                        sample_rate,
                        ..Snapshot::default()
                    };
                    while !worker_stop.load(Ordering::Relaxed) {
                        if let Err(error) = linux::run(
                            &worker_stop,
                            &worker_snapshot,
                            &mut stats,
                            frames.as_ref(),
                            &target,
                        ) {
                            stats.status = format!("连接失败，1 秒后重试：{error:#}");
                            stats.device = None;
                            stats.rms = 0.;
                            stats.peak = 0.;
                            stats.silent = true;
                            publish(&worker_snapshot, &stats);
                        }
                        for _ in 0..20 {
                            if worker_stop.load(Ordering::Relaxed) {
                                break;
                            }
                            thread::sleep(Duration::from_millis(50));
                        }
                    }
                    stats.status = "采集已停止".into();
                    stats.rms = 0.;
                    stats.peak = 0.;
                    stats.silent = true;
                    publish(&worker_snapshot, &stats);
                })?;
            Ok(Self {
                stop,
                snapshot,
                worker: Some(worker),
            })
        }
    }

    /// Reads the latest aggregate without waiting for the capture thread.
    pub fn snapshot(&self) -> Option<Snapshot> {
        self.snapshot.try_lock().ok().map(|value| value.clone())
    }

    /// Requests cancellation; use `is_finished` before replacing a session.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Whether cancellation or worker termination has completed.
    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Waits for shutdown. Intended for headless validation, never the GUI thread.
    pub fn join(mut self) -> anyhow::Result<()> {
        self.stop();
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| anyhow::anyhow!("audio worker panicked"))?;
        }
        Ok(())
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop();
    }
}

fn publish(shared: &Mutex<Snapshot>, snapshot: &Snapshot) {
    if let Ok(mut current) = shared.lock() {
        *current = snapshot.clone();
    }
}

fn expire_level(stats: &mut Snapshot, last: Instant) {
    if last.elapsed() >= Duration::from_millis(500) {
        stats.rms = 0.;
        stats.peak = 0.;
        stats.silent = true;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn invalid_rates_fail_before_spawning_a_worker() {
        assert!(super::Capture::start(0).is_err());
        assert!(super::Capture::start(192_001).is_err());
    }
}
