//! Isolated native GGUF runtime. No audio, networking or persistent transcript storage.
mod generate;
use anyhow::{Context as _, ensure};
use llama_cpp_2::{
    LlamaBackendDeviceType, LogOptions,
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{LlamaChatMessage, LlamaModel, params::LlamaModelParams},
    sampling::LlamaSampler,
};
use std::{
    io::{BufRead, Read, Write},
    num::NonZeroU32,
    time::{Duration, Instant},
};
use subveil_core::inference::{Device, Input, Output};

fn devices() -> Vec<Device> {
    llama_cpp_2::list_llama_ggml_backend_devices()
        .into_iter()
        .filter(|d| {
            matches!(
                d.device_type,
                LlamaBackendDeviceType::Gpu | LlamaBackendDeviceType::IntegratedGpu
            )
        })
        .map(|d| Device {
            index: d.index,
            name: format!("{} · {}", d.backend, d.description),
            free_bytes: d.memory_free,
        })
        .collect()
}
fn send(output: &Output) -> anyhow::Result<()> {
    let mut out = std::io::stdout().lock();
    serde_json::to_writer(&mut out, output)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}
fn run() -> anyhow::Result<()> {
    llama_cpp_2::send_logs_to_tracing(LogOptions::default().with_logs_enabled(false));
    let backend = LlamaBackend::init().map_err(|_| anyhow::anyhow!("无法初始化本地推理运行库"))?;
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--devices" {
        return send(&Output::Devices(devices()));
    }
    ensure!(
        args.len() == 2,
        "usage: subveil-inference MANIFEST cpu|GPU_INDEX"
    );
    let path = subveil_models::verified_translation_model(std::path::Path::new(&args[0]))?;
    let (params, label) = if args[1] == "cpu" {
        (
            LlamaModelParams::default()
                .with_n_gpu_layers(0)
                .with_devices(&[])?,
            "CPU".to_owned(),
        )
    } else {
        let index: usize = args[1].to_str().context("设备索引无效")?.parse()?;
        let device = devices()
            .into_iter()
            .find(|d| d.index == index)
            .context("GPU 不可用；检查驱动或显式选择 CPU")?;
        ensure!(backend.supports_gpu_offload(), "运行库不支持 GPU 卸载");
        ensure!(
            device.free_bytes as u64
                >= std::fs::metadata(&path)?
                    .len()
                    .saturating_add(512 * 1024 * 1024),
            "GPU 可用显存不足；释放显存或显式选择 CPU"
        );
        (
            LlamaModelParams::default()
                .with_devices(&[index])?
                .with_n_gpu_layers(1000),
            device.name,
        )
    };
    let model = LlamaModel::load_from_file(&backend, &path, &params)
        .map_err(|_| anyhow::anyhow!("无法加载 GGUF；检查格式、内存和所选设备"))?;
    // Compile the selected backend's kernels before capture starts. This fixed,
    // public phrase never includes a user transcript and is never emitted.
    let warmup = Input {
        id: 0,
        text: "Hello.".into(),
        source: "en".into(),
        target: "zh".into(),
        timeout_ms: 20_000,
    };
    generate::infer(
        &model,
        &backend,
        &warmup,
        Instant::now() + Duration::from_secs(20),
    )?;
    send(&Output::Ready(format!(
        "本地翻译已就绪 · {label} · 不上传文本"
    )))?;
    let mut input = std::io::stdin().lock();
    loop {
        let mut line = Vec::new();
        let len = input.by_ref().take(65_537).read_until(b'\n', &mut line)?;
        if len == 0 {
            return Ok(());
        }
        ensure!(
            len <= 65_536 && line.last() == Some(&b'\n'),
            "推理请求超过长度限制或协议中断"
        );
        let request: Input = serde_json::from_slice(&line).context("推理请求协议无效")?;
        let deadline = Instant::now() + Duration::from_millis(request.timeout_ms.min(30_000));
        let result = generate::infer(&model, &backend, &request, deadline)
            .map_err(|error| error.to_string());
        send(&Output::Translation {
            id: request.id,
            result,
        })?;
    }
}
fn main() {
    if let Err(error) = run() {
        // Sanitized errors only; parse errors may include positions but never input text.
        let _ = send(&Output::Error(error.to_string()));
        std::process::exit(1);
    }
}
