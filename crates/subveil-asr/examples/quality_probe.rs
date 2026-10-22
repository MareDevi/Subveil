//! Explicit local public-fixture evaluation. Emits aggregate metrics, never transcript text.
use anyhow::{Context as _, ensure};
use std::{
    io::Read as _,
    path::Path,
    thread,
    time::{Duration, Instant},
};
use subveil_asr::{Config, Provider, Recognition};
use subveil_audio::{AudioFrame, FrameReceiver};
use subveil_core::subtitle::TranscriptEvent;

fn distance<T: Eq>(reference: &[T], actual: &[T]) -> usize {
    let mut row: Vec<_> = (0..=actual.len()).collect();
    for (i, expected) in reference.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, value) in actual.iter().enumerate() {
            let old = row[j + 1];
            row[j + 1] = (row[j] + 1)
                .min(old + 1)
                .min(diagonal + usize::from(expected != value));
            diagonal = old;
        }
    }
    row[actual.len()]
}
fn normalized(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        (6..=7).contains(&args.len()),
        "usage: quality_probe MANIFEST WAV REFERENCE_FILE LANGUAGE cpu|cuda clean|quiet|tonal|silence [VAD_MANIFEST]"
    );
    let mut reference = String::new();
    std::fs::File::open(&args[2])
        .context("reference unavailable")?
        .take(16_385)
        .read_to_string(&mut reference)
        .context("reference unavailable")?;
    ensure!(reference.len() <= 16_384, "reference too large");
    let metadata = subveil_asr::model_metadata(Path::new(&args[0]))?;
    let wave = sherpa_onnx::Wave::read(&args[1]).context("WAV unavailable")?;
    ensure!(
        wave.sample_rate() == metadata.sample_rate as i32
            && !wave.samples().is_empty()
            && wave.samples().len() <= metadata.sample_rate as usize * 20,
        "expected model-rate WAV <=20s"
    );
    let compute = match args[4].as_str() {
        "cpu" => subveil_core::inference::Compute::Cpu,
        "cuda" => subveil_core::inference::Compute::Gpu(0),
        _ => anyhow::bail!("invalid compute"),
    };
    ensure!(
        ["clean", "quiet", "tonal", "silence"].contains(&args[5].as_str()),
        "invalid variant"
    );
    let mut samples = wave.samples().to_vec();
    for (i, value) in samples.iter_mut().enumerate() {
        let t = i as f32 / metadata.sample_rate as f32;
        *value = match args[5].as_str() {
            "quiet" => *value * 0.08,
            // A deterministic interference stressor, not a natural music corpus.
            "tonal" => (*value
                + [220., 277.18, 329.63]
                    .iter()
                    .map(|f| (std::f32::consts::TAU * f * t).sin() * 0.025)
                    .sum::<f32>())
            .clamp(-1., 1.),
            "silence" => 0.,
            _ => *value,
        };
    }
    samples.resize(samples.len() + metadata.sample_rate as usize * 2, 0.);
    let (sender, frames) = FrameReceiver::channel();
    let loaded = Instant::now();
    let worker = Recognition::start(
        Config {
            provider: Provider::Local,
            local_model: Some(args[0].clone().into()),
            local_compute: compute,
            vad_model: args.get(6).map(Into::into),
            language: args[3].clone(),
            sample_rate: metadata.sample_rate,
            allow_cloud: false,
            ..Default::default()
        },
        1,
        frames,
    )?;
    loop {
        ensure!(!worker.is_finished(), "local ASR load failed");
        if worker
            .snapshot()
            .is_some_and(|s| s.status.contains("已就绪"))
        {
            break;
        }
        ensure!(loaded.elapsed() < Duration::from_secs(25), "load timeout");
        thread::sleep(Duration::from_millis(10));
    }
    let load_ms = loaded.elapsed().as_millis();
    let started = Instant::now();
    let mut recognized = String::new();
    let mut finals = 0;
    let mut end_lags = Vec::new();
    let mut drain = || -> anyhow::Result<()> {
        while let Some(event) = worker.pop() {
            if let TranscriptEvent::Final(segment) = event {
                finals += 1;
                recognized.push(' ');
                recognized.push_str(&segment.text);
                ensure!(
                    recognized.len() <= 32_768,
                    "result exceeds evaluation budget"
                );
                end_lags
                    .push((started.elapsed().as_millis() as u64).saturating_sub(segment.end_ms));
            }
        }
        ensure!(!worker.is_finished(), "local ASR exited");
        Ok(())
    };
    let block = metadata.sample_rate as usize / 50;
    for (sequence, chunk) in samples.chunks(block).enumerate() {
        let mut frame = AudioFrame {
            samples: [0.; 2048],
            len: chunk.len(),
            generation: 1,
            sequence: sequence as u64,
            sample_offset: (sequence * block) as u64,
            captured: Instant::now(),
            rate: metadata.sample_rate,
        };
        frame.samples[..chunk.len()].copy_from_slice(chunk);
        sender.push(frame);
        drain()?;
        thread::sleep(
            (started + Duration::from_millis((sequence as u64 + 1) * 20))
                .saturating_duration_since(Instant::now()),
        );
    }
    // A fixed bounded drain avoids claiming all slow work finished merely because a queue is empty.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        drain()?;
        thread::sleep(Duration::from_millis(10));
    }
    let snapshot = worker.snapshot().context("ASR statistics unavailable")?;
    worker.join()?;
    let actual = normalized(&recognized);
    let reference = if args[5] == "silence" {
        String::new()
    } else {
        normalized(&reference)
    };
    let actual_chars: Vec<_> = actual.chars().filter(|c| !c.is_whitespace()).collect();
    let reference_chars: Vec<_> = reference.chars().filter(|c| !c.is_whitespace()).collect();
    ensure!(
        actual_chars.len() <= 2048 && reference_chars.len() <= 2048,
        "evaluation token budget exceeded"
    );
    let char_errors = distance(&reference_chars, &actual_chars);
    let reference_words: Vec<_> = reference.split_whitespace().collect();
    let actual_words: Vec<_> = actual.split_whitespace().collect();
    let word_errors = distance(&reference_words, &actual_words);
    end_lags.sort_unstable();
    let max_lag = end_lags.last().copied();
    println!(
        "{}",
        serde_json::json!({"language":args[3],"variant":args[5],"device":args[4],"neural_vad":args.len()==7,"load_ms":load_ms,"finals":finals,"reference_chars":reference_chars.len(),"character_edits":char_errors,"reference_words":(args[3] == "en").then_some(reference_words.len()),"word_edits":(args[3] == "en").then_some(word_errors),"false_speech_chars":if reference.is_empty() {actual_chars.len()} else {0},"final_audio_end_to_state_max_ms":max_lag,"resets":snapshot.resets,"dropped_audio":snapshot.dropped_audio,"dropped_segments":snapshot.dropped_segments,"dropped_events":snapshot.dropped_events,"drain_limit_seconds":5})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metrics_count_insertions_deletions_substitutions_and_unicode() {
        assert_eq!(distance(&['日', '本', '語'], &['日', '語', '語', '言']), 2);
        assert_eq!(distance::<char>(&[], &['声']), 1);
        assert_eq!(distance(&["one", "two"], &["one"]), 1);
        assert_eq!(normalized("Hello, WORLD! 50円。"), "hello world 50円");
    }
}
