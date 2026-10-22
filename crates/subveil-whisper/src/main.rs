//! Isolated Whisper runtime. PCM and transcripts exist only in bounded pipes and memory.
use anyhow::{Context as _, ensure};
use std::{
    io::{BufRead, Read, Write},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use subveil_core::inference::{Output, SpeechInput};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

// Pinned whisper.cpp can silently initialize CPU after a CUDA failure. Observe its
// backend initialization without printing native logs (which may contain text).
// Fail closed if the pinned markers change; do not infer success from device detection.
struct BackendLog {
    selected: AtomicBool,
    failed: AtomicBool,
    weights: AtomicBool,
}
static BACKEND: BackendLog = BackendLog {
    selected: AtomicBool::new(false),
    failed: AtomicBool::new(false),
    weights: AtomicBool::new(false),
};
impl log::Log for BackendLog {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        let message = record.args().to_string();
        if message.starts_with("whisper_backend_init_gpu: using CUDA") {
            self.selected.store(true, Ordering::Relaxed);
        }
        if message.contains("CUDA") && message.contains("total size =") {
            self.weights.store(true, Ordering::Relaxed);
        }
        if record.level() == log::Level::Error {
            self.failed.store(true, Ordering::Relaxed);
        }
    }
    fn flush(&self) {}
}
fn send(output: &Output) -> anyhow::Result<()> {
    let mut out = std::io::stdout().lock();
    serde_json::to_writer(&mut out, output)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}
fn read_request(reader: &mut impl BufRead) -> anyhow::Result<Option<(SpeechInput, Vec<f32>)>> {
    let mut line = Vec::new();
    let count = reader.by_ref().take(1025).read_until(b'\n', &mut line)?;
    if count == 0 {
        return Ok(None);
    }
    ensure!(
        count <= 1024 && line.last() == Some(&b'\n'),
        "语音请求头过长或中断"
    );
    let request: SpeechInput = serde_json::from_slice(&line).context("语音请求协议无效")?;
    ensure!(
        (160..=480_000).contains(&request.samples)
            && matches!(request.language.as_str(), "en" | "ja" | "zh")
            && (1..=30_000).contains(&request.timeout_ms),
        "语音请求采样数、语言或期限无效"
    );
    let mut bytes = vec![0; request.samples * 4];
    reader.read_exact(&mut bytes).context("语音 PCM 通道中断")?;
    let samples: Vec<_> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .collect();
    ensure!(
        samples.iter().all(|s| s.is_finite() && s.abs() <= 1.),
        "语音 PCM 数值无效"
    );
    Ok(Some((request, samples)))
}
fn run() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let backends: &[&str] = if cfg!(feature = "cuda") {
        &["cpu", "cuda"]
    } else {
        &["cpu"]
    };
    send(&Output::Capabilities(
        subveil_core::inference::BuildInfo::current("whisper", backends),
    ))?;
    if args.len() == 1 && args[0] == "--capabilities" {
        return Ok(());
    }
    log::set_logger(&BACKEND).map_err(|_| anyhow::anyhow!("无法初始化推理诊断"))?;
    log::set_max_level(log::LevelFilter::Info);
    whisper_rs::install_logging_hooks();
    ensure!(args.len() == 2, "usage: subveil-whisper MANIFEST cpu|cuda");
    let gpu = match args[1].to_str() {
        Some("cpu") => false,
        Some("cuda") => true,
        _ => anyhow::bail!("语音运行设备必须是 cpu 或 cuda"),
    };
    ensure!(
        !gpu || cfg!(feature = "cuda"),
        "此构建未启用 CUDA；运行 mise run build:cuda 后重试，或显式选择 CPU"
    );
    let model = subveil_models::verified_whisper_model(std::path::Path::new(&args[0]))?;
    let mut parameters = WhisperContextParameters::default();
    parameters.use_gpu(gpu).gpu_device(0).flash_attn(true);
    let context = WhisperContext::new_with_params(
        model.to_str().context("模型路径必须是 UTF-8")?,
        parameters,
    )
    .map_err(|_| anyhow::anyhow!("Whisper 模型加载失败；检查格式、内存与所选设备"))?;
    let mut state = context
        .create_state()
        .map_err(|_| anyhow::anyhow!("Whisper 推理状态创建失败"))?;
    if gpu {
        ensure!(
            BACKEND.selected.load(Ordering::Relaxed)
                && BACKEND.weights.load(Ordering::Relaxed)
                && !BACKEND.failed.load(Ordering::Relaxed),
            "Whisper 未成功加载 CUDA 后端与权重；拒绝回退 CPU"
        );
    }
    // Warm the actual inference graph before audio starts, rather than letting the
    // first user segment pay CUDA context/kernel initialization costs.
    let warmup = SpeechInput {
        id: 0,
        language: "en".into(),
        samples: 16000,
        timeout_ms: 20_000,
    };
    infer(&mut state, &warmup, &[0.; 16000], context.token_eot())?;
    send(&Output::Ready(format!(
        "本地 Whisper 已就绪 · {} · 短块识别 · 不上传音频",
        if gpu { "CUDA0" } else { "CPU" }
    )))?;
    let mut reader = std::io::stdin().lock();
    while let Some((request, samples)) = read_request(&mut reader)? {
        let start = Instant::now();
        let (result, overlap_prefix) =
            match infer(&mut state, &request, &samples, context.token_eot()) {
                Ok((text, prefix)) => (Ok(text), prefix),
                Err(error) => (Err(error.to_string()), None),
            };
        send(&Output::Recognition {
            id: request.id,
            result,
            overlap_prefix,
            inference_ms: start.elapsed().as_millis() as u64,
        })?;
    }
    Ok(())
}
fn infer(
    state: &mut whisper_rs::WhisperState,
    request: &SpeechInput,
    samples: &[f32],
    end_of_text: i32,
) -> anyhow::Result<(String, Option<String>)> {
    let deadline = Instant::now() + Duration::from_millis(request.timeout_ms);
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_n_threads(4);
    params.set_language(Some(&request.language));
    params.set_translate(false);
    params.set_no_context(true);
    params.set_single_segment(true);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_token_timestamps(true);
    params.set_suppress_blank(true);
    params.set_suppress_nst(true);
    params.set_temperature(0.);
    params.set_temperature_inc(0.);
    params.set_max_tokens(256);
    params.set_abort_callback_safe(move || Instant::now() >= deadline);
    state
        .full(params, samples)
        .map_err(|_| anyhow::anyhow!("Whisper 推理失败或超时"))?;
    ensure!(Instant::now() < deadline, "Whisper 推理超时");
    let mut text = String::new();
    let mut tokens = Vec::new();
    for segment in state.as_iter() {
        for ix in 0..segment.n_tokens() {
            let token = segment.get_token(ix).context("Whisper token 索引无效")?;
            if token.token_id() >= end_of_text {
                continue;
            }
            let data = token.token_data();
            tokens.push((
                token.to_bytes().context("Whisper token 字节无效")?.to_vec(),
                data.t0,
                data.t1,
            ));
            ensure!(tokens.len() <= 512, "Whisper 时间对齐 token 过多");
        }
        text.push_str(segment.to_str().context("Whisper 返回无效 UTF-8")?);
        ensure!(text.len() <= 16_384, "Whisper 结果超过长度限制");
    }
    let text = text.trim().to_owned();
    let prefix = timed_context_prefix(&tokens).filter(|prefix| text.starts_with(prefix));
    Ok((text, prefix))
}

// Native whisper.cpp times use centiseconds, despite some generated docs saying milliseconds.
fn timed_context_prefix(tokens: &[(Vec<u8>, i64, i64)]) -> Option<String> {
    let mut bytes = Vec::new();
    let mut previous_end = 0;
    for (token, start, end) in tokens {
        if *start < previous_end || *end <= *start {
            return None;
        }
        if *end > 40 {
            break;
        }
        bytes.extend_from_slice(token);
        previous_end = *end;
    }
    let prefix = String::from_utf8(bytes).ok()?.trim().to_owned();
    (4..=80).contains(&prefix.chars().count()).then_some(prefix)
}
fn main() {
    if let Err(error) = run() {
        let _ = send(&Output::Error(error.to_string()));
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn timed_prefix_uses_centiseconds_and_requires_complete_valid_tokens() {
        let tokens = vec![
            (b" hello".to_vec(), 0, 20),
            (b" world".to_vec(), 20, 40),
            (b" again".to_vec(), 40, 60),
        ];
        assert_eq!(
            timed_context_prefix(&tokens).as_deref(),
            Some("hello world")
        );
        assert!(timed_context_prefix(&[(b"hello".to_vec(), 0, 41)]).is_none());
        for (start, end) in [(-1, 20), (0, 0), (20, 10)] {
            assert!(timed_context_prefix(&[(b"hello".to_vec(), start, end)]).is_none());
        }
        assert!(
            timed_context_prefix(&[(b"hello".to_vec(), 0, 20), (b"world".to_vec(), 19, 30)])
                .is_none()
        );
        let japanese = "到着します".as_bytes();
        let split = vec![
            (japanese[..4].to_vec(), 0, 20),
            (japanese[4..].to_vec(), 20, 40),
        ];
        assert_eq!(timed_context_prefix(&split).as_deref(), Some("到着します"));
        assert!(timed_context_prefix(&[(japanese[..4].to_vec(), 0, 20)]).is_none());
        assert!(timed_context_prefix(&[(b"yes".to_vec(), 0, 20)]).is_none());
    }
    fn packet(samples: usize, pcm: &[f32]) -> anyhow::Result<Vec<u8>> {
        let header = SpeechInput {
            id: 1,
            language: "ja".into(),
            samples,
            timeout_ms: 1000,
        };
        let mut bytes = serde_json::to_vec(&header)?;
        bytes.push(b'\n');
        for sample in pcm {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        Ok(bytes)
    }
    #[test]
    fn pcm_protocol_rejects_oversize_truncation_and_nonfinite_audio() -> anyhow::Result<()> {
        for bytes in [
            packet(480_001, &[])?,
            packet(160, &[0.])?,
            packet(160, &[f32::NAN; 160])?,
            vec![b'x'; 1025],
        ] {
            assert!(read_request(&mut Cursor::new(bytes)).is_err());
        }
        let bytes = packet(160, &[0.25; 160])?;
        let (request, samples) = read_request(&mut Cursor::new(bytes))?.context("request")?;
        assert_eq!(request.id, 1);
        assert_eq!(samples, vec![0.25; 160]);
        Ok(())
    }
}
