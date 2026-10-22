//! Public, fixed text fixtures. No network requests or user transcript logging.
use anyhow::{Context as _, ensure};
use std::{
    thread,
    time::{Duration, Instant},
};
use subveil_core::subtitle::{SegmentKey, TranscriptSegment};
use subveil_translate::{Compute, Request, Translation};

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let devices = subveil_translate::devices()?;
    for device in &devices {
        println!(
            "GPU {}: {} ({:.0} MiB free)",
            device.index,
            device.name,
            device.free_bytes as f64 / 1_048_576.
        );
    }
    if args.first().is_some_and(|arg| arg == "--devices") {
        return Ok(());
    }
    ensure!(
        (1..=2).contains(&args.len()),
        "usage: local_probe MANIFEST [cpu|gpu]"
    );
    let compute = match args.get(1).map(String::as_str).unwrap_or("cpu") {
        "cpu" => Compute::Cpu,
        "gpu" => Compute::Gpu(devices.first().context("no GPU device")?.index),
        _ => anyhow::bail!("expected cpu or gpu"),
    };
    let worker = Translation::start_local(args[0].clone().into(), compute, 1)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(snapshot) = worker.local_snapshot() {
            ensure!(!snapshot.finished, "{}", snapshot.status);
            if snapshot.ready {
                println!("{}", snapshot.status);
                break;
            }
        }
        ensure!(Instant::now() < deadline, "model loading timeout");
        thread::sleep(Duration::from_millis(10));
    }
    let mut quality_failures = 0;
    for (index, (language, target, text, meaning)) in [
        (
            "en",
            "zh",
            "Good morning. The weather is very nice today.",
            "weather",
        ),
        (
            "ja",
            "zh",
            "こんにちは。今日はとても良い天気です。",
            "weather",
        ),
        ("ja", "zh", "うちの中学は弁当制で", "fragment"),
        ("en", "zh", "Do not pay 50 dollars.", "negation_zh"),
        ("zh", "en", "不要支付50美元。", "negation_en"),
        ("en", "ja", "Do not pay 50 dollars.", "negation_ja"),
        ("ja", "en", "50ドルを払わないでください。", "negation_en"),
        ("zh", "ja", "不要支付50美元。", "negation_ja"),
        ("en", "zh", "Subveil is ready.", "glossary"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut request = Request::new(
            TranscriptSegment {
                key: SegmentKey {
                    session_id: 1,
                    segment_id: index as u64 + 1,
                    revision: 1,
                },
                text: text.into(),
                language: Some(language.into()),
                start_ms: 0,
                end_ms: 100,
            },
            target.into(),
        );
        if meaning == "glossary" {
            request.context = "Subveil is a subtitle application.".into();
            request.glossary = "Subveil 翻译成 字幕帷幕".into();
        }
        request.deadline = Instant::now() + Duration::from_secs(25);
        let started = Instant::now();
        worker.submit(request);
        loop {
            if let Some(response) = worker.pop() {
                let text = response.result.map_err(anyhow::Error::msg)?;
                let matched = match meaning {
                    "weather" => text.contains("天气"),
                    "fragment" => {
                        !text.contains("免费")
                            && ["便当", "带饭", "自带", "午餐"]
                                .iter()
                                .any(|term| text.contains(term))
                    }
                    "negation_zh" => {
                        text.contains("50")
                            && text.contains("美元")
                            && ["不", "勿"].iter().any(|s| text.contains(s))
                    }
                    "negation_en" => {
                        let lower = text.to_lowercase();
                        lower.contains("50")
                            && (lower.contains("dollar") || lower.contains('$'))
                            && (lower.contains("not")
                                || lower.contains("n't")
                                || lower.contains("n’t"))
                    }
                    "negation_ja" => {
                        !text.contains("必要")
                            && !text.contains("なくても")
                            && text.contains("50")
                            && text.contains("ドル")
                            && ["ない", "ません", "禁止"].iter().any(|s| text.contains(s))
                    }
                    "glossary" => text.contains("字幕帷幕"),
                    _ => false,
                };
                if !matched {
                    quality_failures += 1;
                }
                println!(
                    "{language}->{target}: {} chars, {} ms; {meaning} matched={matched}",
                    text.chars().count(),
                    started.elapsed().as_millis()
                );
                break;
            }
            ensure!(
                started.elapsed() < Duration::from_secs(26),
                "translation timeout"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    worker.stop();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !worker.local_snapshot().is_some_and(|s| s.finished) {
        ensure!(Instant::now() < deadline, "unload timeout");
        thread::sleep(Duration::from_millis(10));
    }
    println!("Model unloaded; fixture_quality_failures={quality_failures}");
    ensure!(
        quality_failures == 0,
        "fixed-fixture quality checks failed; not a general translation score"
    );
    Ok(())
}
