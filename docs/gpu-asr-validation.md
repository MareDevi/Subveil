# Whisper CUDA 使用与短时验收

日期：2026-10-21，Asia/Shanghai。硬件：Core Ultra 9 185H、约 32 GB RAM、RTX 4060 Laptop 8188 MiB；CachyOS / Hyprland 0.56.2、NVIDIA 615.78.08、CUDA 13.4、GCC 15 CUDA host compiler。

## 已实现与使用

应用内模型库新增 Whisper large-v3-turbo Q5_0，574041195 字节，MIT，固定 Hugging Face revision `5359861c739e955e79d9a303bcbc70fb988958b1`；SHA-256 `394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2`。安装前显示来源、许可与大小，使用现有显式下载、校验、取消和离线缓存机制。本机已安装。自有 GGML 导入示例见 `config/whisper-model.example.toml`，不能使用 ONNX 或 GGUF 冒充 Whisper GGML。

```sh
mise run doctor:cuda
mise run models:cuda
```

1. 在「语音服务」选择「本地语音识别」，模型选 Whisper Turbo，核对来源与 MIT 许可后「安装并选用」。安装不开始采集。
2. 「语音识别设备」选择 NVIDIA CUDA0。当前仅支持 CUDA 设备 0；不会自动改用 CPU、Intel Arc 或云端。默认 CPU 构建未包含 CUDA 时会明确提示重新构建。
3. 「Whisper 分块」在 2 / 4 / 8 秒切换。默认 2 秒，持续讲话也会输出短块 Final；这不是 Whisper 原生 streaming，可能切断词句。较长块提供更多上下文，同时增加等待时间。
4. 「翻译服务」选择本地 Qwen；「翻译运行设备」独立选择 CPU 或实际枚举的 Vulkan GPU。CUDA ASR 与 Vulkan 文字翻译是不同后端，不代表 sherpa CUDA 已实现。
5. 已安装模型可开启离线模式。点击「开始本地识别」，等待翻译和 ASR 完成加载与预热，再播放需要翻译的音频。「实时字幕」和浮窗显示原文与中文。
6. 停止取消会话、终止 helper 并释放模型。「保存本地设置」保留设备、分块、模型和语种；恢复不会自动采集或恢复上传授权。

标准 Button 支持 Tab / Shift+Tab、Enter。真实窗口已验证 CUDA→CPU 及 2→4 秒选择变化；临时设置恢复已验证。通过键盘启动真实识别、播放公开日语 WAV，GPUI 实时字幕及浮窗可渲染原文 / 中文，停止测试进程后无残留模型进程。截图仅保留在 `/tmp`，不进入 Git。

安装时将 `subveil`、`subveil-inference`、`subveil-whisper` 放在同一目录。`SUBVEIL_WHISPER_BIN` 可指定绝对路径 helper 进行诊断。`mise run build` / `models` 会构建 CPU Whisper；使用 CUDA 时运行 `build:cuda` / `models:cuda` / `run:cuda`。默认 CI 不依赖 CUDA。CUDA 任务默认编译 Ada SM89，其他 GPU 设置 `CMAKE_CUDA_ARCHITECTURES`；本机脚本使用已有 `g++-15`，不安装或更改系统包。

## 实测结果

Whisper 固定 whisper-rs 0.16.0 / whisper-rs-sys 0.15.0，内含 whisper.cpp 1.8.3。Qwen 与此前 M05 相同，llama-cpp-2 0.1.160 / Vulkan；中文提示额外约束片段忠实翻译、数字、货币和否定，不允许补全未完句。两个模型在报告 Ready 前执行固定公开输入预热。

公开 WAV 与 M05 相同：日语 7.2 秒、英语 7.152 秒，16 kHz 单声道。固定 WAV SHA-256 见 [M05 基线](m05-validation.md)。没有上传用户音频，没有把文本、音频、模型、临时截图或日志提交 Git。

### Release ASR 文件吞吐

显式运行 `whisper:bench:release`；模型常驻并预热后单次处理整段音频，无批量推理。每语种 3 次，不把文件吞吐当作字幕延迟。

| 语种 / 设备 | 加载与预热 | 推理合计时间，包含 IPC | RTF | 样本数 |
| --- | --- | --- | --- | --- |
| 日语 / CUDA | 1153 ms | 191.7–200.7 ms，中位数 199.8 ms | 0.0266–0.0279 | 3 |
| 英语 / CUDA | 1025 ms | 162.7–169.9 ms，中位数 168.5 ms | 0.0227–0.0238 | 3 |
| 英语 / CPU | 18326 ms | 18560.6 ms | 2.5952 | 1 |

另用同一日语 WAV 在常驻、预热后的两个 helper 之间串联转写与中文翻译，单路非批量运行 3 次，实测合计 387 / 370 / 371 ms，RTF 0.0537 / 0.0513 / 0.0515。该短诊断使用 release Whisper 与开发构建 Qwen helper（原生 GGUF C++ 为 RelWithDebInfo），包含串行调度与两次 IPC；不是把两个独立最佳值相加，也不包含等待收集音频的时间。

CUDA ASR 与上述串联短诊断均超过 2 倍实时目标。CPU Whisper 不适合本机低延迟后台字幕；CPU 资源受限时优先保留现有 SenseVoice / Zipformer 路径。CPU 原先 10 秒期限不足，最终独立采用 30 秒，CUDA 保持 10 秒；期限增加不代表 CPU 达到实时目标。

### 热态字幕与资源

下表为最终版本开发构建的实时 WAV 回放诊断，最长 2 秒分块，CUDA ASR + RTX Vulkan Qwen。Native C++ 为 RelWithDebInfo；不是完整 release 应用基准。加载与预热发生在回放之前。

| 指标 | 日语→中文 | 英语→中文 |
| --- | --- | --- |
| 首个 RMS 有声帧在 WAV 中的偏移 | 520 ms | 1100 ms |
| 首条原文进入字幕状态，距有声起点 | 2100 ms | 2040 ms |
| 首条中文进入字幕状态，距有声起点 | 2200 ms | 2160 ms |
| 首条中文距 WAV 开始 | 2720 ms | 3260 ms |
| 各块结束至中文进入字幕状态 | 260 / 400 / 1220 ms | 360 / 880 / 960 ms |
| Final / 中文完成 / Pending | 3 / 3 / 0 | 3 / 3 / 0 |
| 音频丢块 / 事件丢弃 / 分块丢弃 / 重置 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| 双模型显存合计观测峰值 | 1960 MiB | 1949 MiB |

首条中文短测达到 1–3 秒目标。显存按约 150 ms 间隔采样该诊断进程的子进程；Whisper 峰值约 908 MiB，Qwen 峰值约 1052 MiB。合计观测峰值约 1.91 GiB，低于 4 GiB 目标；采样不能保证捕获更短的瞬时峰值，也不包括桌面或其他程序。单进程峰值不一定发生在同一时刻。

有声起点使用与能量分段相同的 RMS 0.01 阈值，不是人工语音标注。状态接收时间不等于显示器呈现时间；当前 GPUI 会话轮询为 250 ms，还可能有渲染和 compositor 延迟。真实 PipeWire 窗口短测已显示中文字幕，但未测硬件 PTS / 光学呈现延迟。Vulkan 首次遇到新计算形状仍可能有额外开销，尾延迟不作保证。样本量不足以证明稳定的 p95。

## 边界、可靠性与质量

- 音频消费不等待推理。等待分块容量 1，过载淘汰最旧块；等待超过 1 秒的块丢弃。结果容量 2。界面单独统计丢弃分块，不伪装为成功字幕。
- 2 秒无 PCM、音频不连续或设备换代会重置 epoch；旧推理结果不能进入新上下文。Session / Segment / revision 的翻译保护保留。
- PCM 仅经内存管道传输，头部最多 1024 字节，单请求最多 30 秒；响应最多 64 KiB，文本最多 16 KiB。取消 / 超时杀死 helper，关闭管道；原生崩溃不带崩 GPUI。
- CUDA 初始化可能由上游回退 CPU。固定版本原生日志只在内存中检查后端、权重分配和错误标记，缺少标记即失败。实际隐藏 CUDA 设备后测试返回「拒绝回退 CPU」；不是仅检查设备枚举。
- Qwen 1.5B 及 2 秒 ASR 分块存在误识、术语误译和上下文损失。窗口验收发现未完日语片段曾被增译为“免费”；最终中文提示和固定片段回归检查已拒绝该增译。该检查仅证明特定样本，不等于翻译质量全面达标。可按内容选择 4 / 8 秒块，不能同时保证所有内容的 1–3 秒延迟和完整句子质量。
- 尚未完成游戏 / 视频并行负载帧时间、功耗限制、多种内容 WER / 翻译评分、30 分钟稳定性、其他 GPU / compositor 和独占全屏验收。没有声称“不影响游戏”。Seamless / Canary 仍是研究候选，没有加入伪可用后端。

## 重复验证

```sh
mise run check
mise run check:cuda
mise run model:install -- whisper-large-v3-turbo-q5
# 以下变量指向用户显式选择的本机清单和公开 WAV；不上传内容。
asr_manifest=/absolute/path/whisper/model.toml
translation_manifest=/absolute/path/translation/model.toml
public_wav=/absolute/path/ja.wav
mise run whisper:bench:release -- "$asr_manifest" "$public_wav" ja cuda
mise run offline:probe:cuda -- "$asr_manifest" "$translation_manifest" "$public_wav" ja gpu --asr-cuda
# 显式系统回放仅在没有其他音频时运行。
mise run offline:probe:cuda -- "$asr_manifest" "$translation_manifest" "$public_wav" ja gpu --asr-cuda --system
```

`--system` 的分段时间基准从采集开始，包含预热期间流逝的采样偏移；不能直接把它和 WAV 内有声起点相减，进行精确字幕延迟判断。延迟表使用可对齐的非系统实时回放；真实系统回放用于链路功能验收。单次测试 / 诊断最多 90 秒；release 构建在诊断计时之外进行。

默认门禁 93 项测试，CUDA 门禁 92 项测试（CPU 构建拒绝 CUDA 的专属测试在 CUDA 构建中不运行）。覆盖完整性、协议截断 / 超长 / 非有限 PCM、原生崩溃、取消、超时、阻塞写、错误请求编号、连续分块边界、过载仍消费 PCM、设置持久化与既有字幕修订保护。Core 测试不依赖 GPU、真实采集设备或付费服务。

来源与架构决策见 [选型规范](gpu-asr-evaluation.md)、[技术调研](technical-research.md)。项目许可证归属未改动。
