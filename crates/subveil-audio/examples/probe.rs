//! Headless local capture validation. Prints aggregates, never audio samples.
use std::time::{Duration, Instant};
use subveil_audio::Capture;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let seconds = args
        .next()
        .map(|s| s.parse::<u64>())
        .transpose()?
        .unwrap_or(10);
    let rate = args
        .next()
        .map(|s| s.parse::<u32>())
        .transpose()?
        .unwrap_or(48_000);
    anyhow::ensure!(
        args.next().is_none(),
        "usage: probe [seconds] [sample_rate]"
    );
    let capture = Capture::start(rate)?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(seconds) {
        std::thread::sleep(Duration::from_secs(1));
        if let Some(s) = capture.snapshot() {
            println!(
                "elapsed={} status={} device={} rate={} rms={:.6} peak={:.6} silent={} blocks={} samples={} dropped_blocks={} dropped_samples={} invalid={} connections={}",
                started.elapsed().as_secs(),
                s.status,
                s.device.as_deref().unwrap_or("none"),
                s.sample_rate,
                s.rms,
                s.peak,
                s.silent,
                s.blocks,
                s.samples,
                s.dropped_blocks,
                s.dropped_samples,
                s.invalid_chunks,
                s.connections
            );
        }
        anyhow::ensure!(!capture.is_finished(), "capture worker exited unexpectedly");
    }
    capture.join()
}
