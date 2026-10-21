//! Explicit model benchmark; public WAV content and transcripts are never printed.
use anyhow::{Context as _, ensure};
use std::{path::Path, time::Instant};
use subveil_asr::WhisperEngine;
use subveil_core::inference::Compute;
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 4,
        "usage: whisper_bench MANIFEST WAV LANGUAGE cpu|cuda"
    );
    let compute = match args[3].as_str() {
        "cpu" => Compute::Cpu,
        "cuda" => Compute::Gpu(0),
        _ => anyhow::bail!("expected cpu|cuda"),
    };
    let wave = sherpa_onnx::Wave::read(&args[1]).context("WAV unavailable")?;
    ensure!(
        wave.sample_rate() == 16000 && (160..=480_000).contains(&wave.samples().len()),
        "expected 16 kHz mono WAV <=30s"
    );
    let load = Instant::now();
    let mut engine = WhisperEngine::load(Path::new(&args[0]), compute, || false)?;
    println!(
        "Whisper loaded and warmed: device={} load_ms={}",
        args[3],
        load.elapsed().as_millis()
    );
    let audio_ms = wave.samples().len() as f64 / 16.;
    let mut times = Vec::new();
    let trials = if compute == Compute::Cpu { 1 } else { 3 };
    for trial in 1..=trials {
        let start = Instant::now();
        let result = engine.transcribe(wave.samples(), &args[2], || false)?;
        ensure!(!result.text.is_empty(), "empty transcription");
        let elapsed = start.elapsed().as_secs_f64() * 1000.;
        times.push(elapsed);
        println!(
            "trial={trial} audio_ms={audio_ms:.1} native_ms={} total_ms={elapsed:.1} rtf={:.4} chars={}",
            result.inference_ms,
            elapsed / audio_ms,
            result.text.chars().count()
        );
    }
    times.sort_by(f64::total_cmp);
    println!(
        "samples={} median_ms={:.1} max_ms={:.1}; throughput only, not subtitle latency",
        times.len(),
        times[times.len() / 2],
        times[times.len() - 1]
    );
    drop(engine);
    Ok(())
}
