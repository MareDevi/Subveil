//! Explicit local WAV diagnostic; never uploads or prints transcript text.
use anyhow::{Context as _, ensure};
use std::{
    thread,
    time::{Duration, Instant},
};
use subveil_asr::{Config, Provider, Recognition};
use subveil_audio::{AudioFrame, FrameReceiver};
use subveil_core::subtitle::TranscriptEvent;

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        (2..=3).contains(&args.len()),
        "usage: local_wav_probe MODEL_MANIFEST WAV [LANGUAGE]"
    );
    let metadata = subveil_asr::model_metadata(std::path::Path::new(&args[0]))?;
    let language = args.get(2).unwrap_or(&metadata.language).clone();
    ensure!(metadata.languages.contains(&language), "模型不支持该语种");
    let kind = metadata.kind;
    let wave = sherpa_onnx::Wave::read(&args[1]).context("无法读取 WAV")?;
    ensure!(
        wave.sample_rate() == metadata.sample_rate as i32,
        "WAV 采样率与模型不一致"
    );
    ensure!(
        !wave.samples().is_empty() && wave.samples().len() <= metadata.sample_rate as usize * 30,
        "WAV 必须为 1..30 秒"
    );
    let (sender, frames) = FrameReceiver::channel();
    let worker = Recognition::start(
        Config {
            provider: Provider::Local,
            local_model: Some(args[0].clone().into()),
            language,
            sample_rate: metadata.sample_rate,
            ..Default::default()
        },
        1,
        frames,
    )?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        ensure!(
            !worker.is_finished(),
            "模型加载失败：{}",
            worker.snapshot().context("缺少状态")?.status
        );
        if worker
            .snapshot()
            .is_some_and(|s| s.status.contains("已就绪"))
        {
            break;
        }
        ensure!(Instant::now() < deadline, "模型加载超时");
        thread::sleep(Duration::from_millis(10));
    }
    let block = metadata.sample_rate as usize / 50;
    let mut samples = wave.samples().to_vec();
    samples.resize(samples.len() + metadata.sample_rate as usize * 2, 0.);
    let mut partials = 0;
    let mut finals = 0;
    let started = Instant::now();
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
        while let Some(event) = worker.pop() {
            match event {
                TranscriptEvent::Partial(_) => partials += 1,
                TranscriptEvent::Final(_) => finals += 1,
            }
        }
        ensure!(!worker.is_finished(), "本地识别线程退出");
        let next = started + Duration::from_millis((sequence as u64 + 1) * 20);
        thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    thread::sleep(Duration::from_millis(250));
    while let Some(event) = worker.pop() {
        match event {
            TranscriptEvent::Partial(_) => partials += 1,
            TranscriptEvent::Final(_) => finals += 1,
        }
    }
    let snapshot = worker.snapshot().context("缺少本地识别统计")?;
    worker.join()?;
    println!(
        "Local WAV: mode={} partials={partials} finals={finals} resets={} dropped_audio={} dropped_events={} inference_ms={} audio_ms={}",
        kind.name(),
        snapshot.resets,
        snapshot.dropped_audio,
        snapshot.dropped_events,
        snapshot.local_inference_ms,
        snapshot.local_audio_ms
    );
    ensure!(finals > 0, "未收到真实 Final");
    if kind == subveil_asr::ModelKind::StreamingTransducer {
        ensure!(partials > 0, "Streaming 模型未收到真实 Partial");
    } else {
        ensure!(partials == 0, "离线模型不能伪造 Partial");
    }
    Ok(())
}
