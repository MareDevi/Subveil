//! Real local WAV/capture -> ASR -> local translation -> subtitle-state diagnostic.
//! Source and translated text are never printed or uploaded.
use anyhow::{Context as _, ensure};
use std::{
    thread,
    time::{Duration, Instant},
};
use subveil_audio::{AudioFrame, FrameReceiver};
use subveil_core::subtitle::{SubtitleState, TranscriptEvent};
use subveil_translate::{Compute, Request, Translation};

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        (5..=7).contains(&args.len()),
        "usage: offline_pipeline ASR_MANIFEST TRANSLATION_MANIFEST WAV LANGUAGE cpu|gpu [--asr-cuda] [--system]"
    );
    let metadata = subveil_asr::model_metadata(std::path::Path::new(&args[0]))?;
    let compute = match args[4].as_str() {
        "gpu" => Compute::Gpu(
            subveil_translate::devices()?
                .first()
                .context("GPU unavailable")?
                .index,
        ),
        "cpu" => Compute::Cpu,
        _ => anyhow::bail!("expected cpu|gpu"),
    };
    let translation = Translation::start_local(args[1].clone().into(), compute, 1)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(snapshot) = translation.local_snapshot() {
            ensure!(!snapshot.finished, "{}", snapshot.status);
            if snapshot.ready {
                println!("{}", snapshot.status);
                break;
            }
        }
        ensure!(Instant::now() < deadline, "translation load timeout");
        thread::sleep(Duration::from_millis(10));
    }
    let system = args[5..].iter().any(|argument| argument == "--system");
    let asr_cuda = args[5..].iter().any(|argument| argument == "--asr-cuda");
    ensure!(
        args[5..]
            .iter()
            .all(|a| matches!(a.as_str(), "--system" | "--asr-cuda")),
        "unknown option"
    );
    ensure!(
        !asr_cuda || metadata.kind == subveil_asr::ModelKind::Whisper,
        "--asr-cuda requires a Whisper model"
    );
    let (sender, capture, frames) = if system {
        let (capture, frames) = subveil_audio::Capture::start_with_frames(metadata.sample_rate)?;
        (None, Some(capture), frames)
    } else {
        let (sender, frames) = FrameReceiver::channel();
        (Some(sender), None, frames)
    };
    let recognition = subveil_asr::Recognition::start(
        subveil_asr::Config {
            provider: subveil_asr::Provider::Local,
            local_model: Some(args[0].clone().into()),
            local_compute: if asr_cuda {
                Compute::Gpu(0)
            } else {
                Compute::Cpu
            },
            language: args[3].clone(),
            sample_rate: metadata.sample_rate,
            allow_cloud: false,
            ..Default::default()
        },
        1,
        frames,
    )?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        ensure!(
            !recognition.is_finished(),
            "ASR failed: {}",
            recognition.snapshot().context("ASR status")?.status
        );
        if recognition
            .snapshot()
            .is_some_and(|state| state.status.contains("已就绪"))
        {
            break;
        }
        ensure!(Instant::now() < deadline, "ASR load timeout");
        thread::sleep(Duration::from_millis(10));
    }
    let wave = sherpa_onnx::Wave::read(&args[2]).context("WAV unavailable")?;
    ensure!(
        wave.sample_rate() == metadata.sample_rate as i32
            && wave.samples().len() <= metadata.sample_rate as usize * 20,
        "expected model-rate WAV <=20s"
    );
    let first_voiced_ms = wave
        .samples()
        .chunks(metadata.sample_rate as usize / 50)
        .position(|chunk| {
            chunk.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / chunk.len() as f64 >= 0.0001
        })
        .map(|index| index as u64 * 20);
    let mut samples = wave.samples().to_vec();
    samples.resize(samples.len() + metadata.sample_rate as usize * 2, 0.);
    let playback = if system {
        Some(
            std::process::Command::new("pw-play")
                .arg(&args[2])
                .spawn()
                .context("cannot play public WAV through PipeWire")?,
        )
    } else {
        None
    };
    let block = metadata.sample_rate as usize / 50;
    let mut subtitles = SubtitleState::new(1, 64, 60_000);
    let started = Instant::now();
    let mut first_source_ms = None;
    let mut first_chinese_ms = None;
    let mut end_lags = Vec::new();
    let mut first_start_ms = None;
    let mut finals = 0;
    let mut translated = 0;
    let mut pending = 0;
    let mut drain = |subtitles: &mut SubtitleState| -> anyhow::Result<()> {
        ensure!(!recognition.is_finished(), "ASR exited");
        while let Some(event) = recognition.pop() {
            let segment = match &event {
                TranscriptEvent::Final(segment) => Some(segment.clone()),
                _ => None,
            };
            if subtitles.apply(event, started.elapsed().as_millis() as u64)
                && let Some(segment) = segment
            {
                finals += 1;
                first_start_ms.get_or_insert(segment.start_ms);
                first_source_ms.get_or_insert(started.elapsed().as_millis() as u64);
                println!(
                    "source_final: segment={} audio_start_ms={} audio_end_ms={} elapsed_ms={}",
                    segment.key.segment_id,
                    segment.start_ms,
                    segment.end_ms,
                    started.elapsed().as_millis()
                );
                if subtitles.request_translation(segment.key) {
                    let mut request = Request::new(segment, "zh".into());
                    request.deadline = Instant::now() + Duration::from_secs(25);
                    translation.submit(request);
                    pending += 1;
                }
            }
        }
        while let Some(response) = translation.pop() {
            let result = response.result.map_err(anyhow::Error::msg)?;
            ensure!(
                result
                    .chars()
                    .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                "Chinese translation missing"
            );
            ensure!(
                subtitles.finish_translation(
                    response.key,
                    Ok(result),
                    started.elapsed().as_millis() as u64
                ),
                "subtitle rejected current translation"
            );
            let now = started.elapsed().as_millis() as u64;
            first_chinese_ms.get_or_insert(now);
            if let Some(row) = subtitles
                .history()
                .iter()
                .find(|row| row.transcript.key == response.key)
            {
                end_lags.push(now.saturating_sub(row.transcript.end_ms));
            }
            translated += 1;
            pending -= 1;
        }
        Ok(())
    };
    for (sequence, chunk) in samples.chunks(block).enumerate() {
        if let Some(sender) = &sender {
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
        }
        drain(&mut subtitles)?;
        thread::sleep(
            (started + Duration::from_millis((sequence as u64 + 1) * 20))
                .saturating_duration_since(Instant::now()),
        );
    }
    let deadline = Instant::now() + Duration::from_secs(26);
    loop {
        drain(&mut subtitles)?;
        if Instant::now() >= deadline || translation.local_snapshot().is_some_and(|s| s.finished) {
            break;
        }
        // Once all emitted Final revisions finish, allow another second for the last ASR event.
        if started.elapsed()
            > Duration::from_secs_f64(samples.len() as f64 / metadata.sample_rate as f64 + 1.)
            && translation.dropped() == 0
            && subtitles.current().is_some_and(|s| {
                matches!(
                    s.translation,
                    subveil_core::subtitle::TranslationState::Complete(_)
                )
            })
        {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    drop(drain);
    if let Some(mut playback) = playback {
        let _ = playback.kill();
        let _ = playback.wait();
    }
    if let Some(capture) = capture {
        capture.stop();
    }
    let stats = recognition.snapshot().context("ASR stats")?;
    recognition.join()?;
    translation.stop();
    let stop_deadline = Instant::now() + Duration::from_secs(2);
    while !translation.local_snapshot().is_some_and(|s| s.finished) {
        ensure!(
            Instant::now() < stop_deadline,
            "translation helper did not unload"
        );
        thread::sleep(Duration::from_millis(10));
    }
    println!(
        "Offline pipeline: system_capture={system} finals={finals} translated={translated} pending={pending} elapsed_ms={}",
        started.elapsed().as_millis()
    );
    end_lags.sort_unstable();
    println!(
        "ASR device={} native_ms={} audio_ms={} resets={} dropped_audio={} dropped_events={} dropped_segments={}",
        if asr_cuda { "cuda" } else { "cpu" },
        stats.local_inference_ms,
        stats.local_audio_ms,
        stats.resets,
        stats.dropped_audio,
        stats.dropped_events,
        stats.dropped_segments
    );
    println!(
        "Software state latency: first_source_ms={first_source_ms:?} first_chinese_ms={first_chinese_ms:?} first_segment_start_ms={first_start_ms:?} end_to_chinese_ms={end_lags:?}; excludes physical presentation and hardware PTS"
    );
    println!(
        "Speech onset latency: first_voiced_ms={first_voiced_ms:?} source_ms={:?} chinese_ms={:?}",
        first_voiced_ms
            .zip(first_source_ms)
            .map(|(onset, shown)| shown.saturating_sub(onset)),
        first_voiced_ms
            .zip(first_chinese_ms)
            .map(|(onset, shown)| shown.saturating_sub(onset))
    );
    ensure!(
        finals > 0 && translated > 0 && pending == 0,
        "offline subtitle pipeline incomplete"
    );
    Ok(())
}
