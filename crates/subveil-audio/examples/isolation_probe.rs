//! Explicit synthetic PipeWire isolation diagnostic. Never records user audio.
#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use anyhow::{Context as _, ensure};
    use pipewire::{self as pw, properties::properties, spa};
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };
    let stop = Arc::new(AtomicBool::new(false));
    let remove = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let removed = remove.clone();
    let name = format!("Subveil isolation {}", std::process::id());
    let stream_name = name.clone();
    let worker = std::thread::spawn(move || -> anyhow::Result<()> {
        pw::init();
        let loop_ = pw::main_loop::MainLoopRc::new(None)?;
        let context = pw::context::ContextRc::new(&loop_, None)?;
        let core = context.connect_rc(None)?;
        let sink_name = format!("subveil-validation-{}", std::process::id());
        let _sink = core.create_object::<pw::node::Node>("adapter", &properties! {
            "factory.name" => "support.null-audio-sink", "node.name" => sink_name.clone(),
            "node.description" => "Subveil temporary isolation sink", "media.class" => "Audio/Sink",
            "audio.position" => "[ FL FR ]", "object.linger" => "false", "node.virtual" => "true",
            "priority.session" => "0", "priority.driver" => "0"
        })?;
        fn tone<'a>(
            core: &'a pw::core::CoreRc,
            target: &str,
            label: &str,
            frequency: f64,
        ) -> anyhow::Result<(pw::stream::StreamListener<f64>, pw::stream::StreamBox<'a>)> {
            let stream = pw::stream::StreamBox::new(
                core,
                label,
                properties! {
                    "application.name" => label, "media.name" => label, "media.type" => "Audio",
                    "media.category" => "Playback", "target.object" => target, "node.dont-fallback" => "true"
                },
            )?;
            let listener = stream
                .add_local_listener_with_user_data(0_f64)
                .process(move |stream, phase| {
                    let Some(mut buffer) = stream.dequeue_buffer() else {
                        return;
                    };
                    let Some(data) = buffer.datas_mut().first_mut() else {
                        return;
                    };
                    let mut size = 0;
                    if let Some(bytes) = data.data() {
                        for frame in bytes.as_chunks_mut::<8>().0 {
                            *phase = (*phase + std::f64::consts::TAU * frequency / 48000.)
                                % std::f64::consts::TAU;
                            let sample = (phase.sin() * 0.05) as f32;
                            frame[..4].copy_from_slice(&sample.to_le_bytes());
                            frame[4..].copy_from_slice(&sample.to_le_bytes());
                            size += 8;
                        }
                    }
                    *data.chunk_mut().offset_mut() = 0;
                    *data.chunk_mut().size_mut() = size;
                    *data.chunk_mut().stride_mut() = 8;
                })
                .register()?;
            let mut format = spa::param::audio::AudioInfoRaw::new();
            format.set_format(spa::param::audio::AudioFormat::F32LE);
            format.set_rate(48000);
            format.set_channels(2);
            let mut positions = [0; spa::param::audio::MAX_CHANNELS];
            positions[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
            positions[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
            format.set_position(positions);
            let bytes = spa::pod::serialize::PodSerializer::serialize(
                std::io::Cursor::new(Vec::new()),
                &spa::pod::Value::Object(spa::pod::Object {
                    type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
                    id: spa::param::ParamType::EnumFormat.as_raw(),
                    properties: format.into(),
                }),
            )
            .map_err(|_| anyhow::anyhow!("format serialization failed"))?
            .0
            .into_inner();
            stream.connect(
                spa::utils::Direction::Output,
                None,
                pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
                &mut [spa::pod::Pod::from_bytes(&bytes).context("format invalid")?],
            )?;
            Ok((listener, stream))
        }
        let mut first = Some(tone(&core, &sink_name, &format!("{stream_name} A"), 440.)?);
        let _second = tone(&core, &sink_name, &format!("{stream_name} B"), 1000.)?;
        let deadline = Instant::now() + Duration::from_secs(15);
        while !stopped.load(Ordering::Relaxed) && Instant::now() < deadline {
            if removed.load(Ordering::Relaxed) {
                first.take();
            }
            loop_
                .loop_()
                .iterate(pw::loop_::Timeout::Finite(Duration::from_millis(10)));
        }
        Ok(())
    });
    let result = (|| -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let selected = loop {
            if let Some(source) = subveil_audio::sources()?
                .into_iter()
                .find(|s| s.label.contains(&format!("{name} A")))
            {
                break source;
            }
            ensure!(Instant::now() < deadline, "synthetic source unavailable");
            std::thread::sleep(Duration::from_millis(50));
        };
        let (capture, frames) = subveil_audio::Capture::start_target(
            48000,
            subveil_audio::CaptureTarget::Node(selected),
        )?;
        let mut count = 0_u64;
        let mut sums = [(0_f64, 0_f64); 2];
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            while let Some(frame) = frames.pop() {
                for &sample in &frame.samples[..frame.len] {
                    for (index, freq) in [440., 1000.].into_iter().enumerate() {
                        let phase = std::f64::consts::TAU * freq * count as f64 / 48000.;
                        sums[index].0 += f64::from(sample) * phase.sin();
                        sums[index].1 += f64::from(sample) * phase.cos();
                    }
                    count += 1;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let power = |index: usize| sums[index].0.powi(2) + sums[index].1.powi(2);
        println!(
            "selected_samples={count} selected_to_other_power_ratio={:.1}",
            power(0) / power(1).max(1e-12)
        );
        ensure!(
            count > 8000 && power(0) > power(1) * 100.,
            "selected-stream isolation failed: {:?}",
            capture.snapshot().map(|s| s.status)
        );
        remove.store(true, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(500));
        while frames.pop().is_some() {}
        std::thread::sleep(Duration::from_millis(500));
        let mut unexpected = 0;
        while let Some(frame) = frames.pop() {
            unexpected += frame.samples[..frame.len]
                .iter()
                .filter(|v| v.abs() > 0.001)
                .count();
        }
        ensure!(
            unexpected == 0,
            "source disappearance captured another stream"
        );
        println!("disconnect_nonzero_samples={unexpected}");
        capture.join()?;
        Ok(())
    })();
    stop.store(true, Ordering::Relaxed);
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("synthetic source worker failed"))??;
    result
}
#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("requires Linux PipeWire")
}
