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
        (5..=6).contains(&args.len()),
        "usage: offline_pipeline ASR_MANIFEST TRANSLATION_MANIFEST WAV LANGUAGE cpu|gpu [--system]"
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
    let system = args.get(5).is_some_and(|argument| argument == "--system");
    ensure!(args.len() == 5 || system, "unknown option");
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
    recognition.join()?;
    translation.stop();
    println!(
        "Offline pipeline: system_capture={system} finals={finals} translated={translated} pending={pending} elapsed_ms={}",
        started.elapsed().as_millis()
    );
    ensure!(
        finals > 0 && translated > 0 && pending == 0,
        "offline subtitle pipeline incomplete"
    );
    Ok(())
}
