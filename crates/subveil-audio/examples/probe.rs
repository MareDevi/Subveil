//! Headless local capture validation. Prints aggregates, never audio samples.
use std::time::{Duration, Instant};
use subveil_audio::Capture;

fn main() -> anyhow::Result<()> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.first().is_some_and(|s| s == "--list") {
        for source in subveil_audio::sources()? {
            println!("{} {:?} {}", source.serial, source.kind, source.label);
        }
        return Ok(());
    }
    let mut args = arguments.into_iter();
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
    let selected = args.next().map(|s| s.parse::<u64>()).transpose()?;
    anyhow::ensure!(
        args.next().is_none(),
        "usage: probe [--list | seconds sample_rate [SOURCE_SERIAL]]"
    );
    anyhow::ensure!(seconds <= 60, "诊断最多 60 秒");
    let capture = if let Some(serial) = selected {
        let source = subveil_audio::sources()?
            .into_iter()
            .find(|s| s.serial == serial)
            .ok_or_else(|| anyhow::anyhow!("所选音源已断开"))?;
        Capture::start_target(rate, subveil_audio::CaptureTarget::Node(source))?.0
    } else {
        Capture::start(rate)?
    };
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
