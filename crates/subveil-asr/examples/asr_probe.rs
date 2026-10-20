//! Explicit system-output ASR diagnostic. No transcript is printed by default.
use anyhow::{Context as _, ensure};
use std::{
    thread,
    time::{Duration, Instant},
};
use subveil_asr::{Config, Provider, Recognition};

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        (1..=6).contains(&args.len()),
        "usage: asr_probe PROVIDER [SECONDS] [en|ja] [RATE] [--allow-upload] [--print-transcripts]"
    );
    let provider = Provider::ALL.into_iter().find(|p| p.id() == args[0]).context("未知 Provider；可用 mock/local/deepgram/alibaba/gemini/volcano/tencent/baidu/openai/azure-openai/xai/custom-dashscope/custom-openai")?;
    let positional: Vec<_> = args[1..].iter().filter(|a| !a.starts_with("--")).collect();
    ensure!(
        positional.len() <= 3
            && args[1..]
                .iter()
                .filter(|a| a.starts_with("--"))
                .all(|a| matches!(a.as_str(), "--allow-upload" | "--print-transcripts")),
        "invalid probe arguments"
    );
    let seconds = positional
        .first()
        .map(|s| s.parse::<u64>())
        .transpose()?
        .unwrap_or(10);
    ensure!(
        (1..=60).contains(&seconds),
        "probe duration must be 1..=60 seconds"
    );
    let config = Config {
        provider,
        language: positional.get(1).map(|s| s.as_str()).unwrap_or("en").into(),
        sample_rate: positional
            .get(2)
            .map(|s| s.parse::<u32>())
            .transpose()?
            .unwrap_or(provider.required_rate().unwrap_or(16_000)),
        local_model: std::env::var_os("SUBVEIL_LOCAL_MODEL").map(Into::into),
        allow_cloud: args.iter().any(|s| s == "--allow-upload"),
        ..Default::default()
    };
    config.validate()?;
    println!(
        "ASR {:?}, {} Hz, {}; cloud upload: {}",
        provider,
        config.sample_rate,
        config.language,
        config.allow_cloud && provider.is_cloud()
    );
    let (capture, frames) = subveil_audio::Capture::start_with_frames(config.sample_rate)?;
    let worker = Recognition::start(config, 1, frames)?;
    let print_text = args.iter().any(|s| s == "--print-transcripts");
    let start = Instant::now();
    let mut last = Instant::now() - Duration::from_secs(1);
    let mut events = 0;
    while start.elapsed() < Duration::from_secs(seconds) {
        while let Some(event) = worker.pop() {
            events += 1;
            if print_text {
                println!("{event:?}");
            }
        }
        while let Some(translation) = worker.pop_translation() {
            if print_text {
                println!("{translation:?}");
            }
        }
        if last.elapsed() >= Duration::from_secs(1) {
            if let Some(snapshot) = worker.snapshot() {
                println!(
                    "{}; resets={} lost_audio={} lost_events={} events={events}",
                    snapshot.status,
                    snapshot.resets,
                    snapshot.dropped_audio,
                    snapshot.dropped_events
                );
            }
            if let Some(audio) = capture.snapshot() {
                println!(
                    "audio: {}; blocks={} dropped={}",
                    audio.status, audio.blocks, audio.dropped_blocks
                );
            }
            last = Instant::now();
        }
        if worker.is_finished() {
            anyhow::bail!(
                "ASR worker terminated: {}",
                worker.snapshot().context("missing worker status")?.status
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
    worker.stop();
    capture.stop();
    worker.join()?;
    capture.join()?;
    println!("Stopped; transcript events: {events}");
    Ok(())
}
