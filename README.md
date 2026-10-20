# Subveil

Rust + GPUI 原生跨平台实时字幕翻译应用，优先支持 Linux / Wayland / PipeWire。

当前开发 **M03：ASR Provider**：已接入音频驱动 Mock、Deepgram 流式接口和可选本地 sherpa-onnx streaming transducer，并保留 M01 离线双语演示与 M02 独立采集。默认不上传、不保存音频。云端须点击明确标注上传的启动按钮；真实云端和本地模型的英语、日语识别验收仍待凭据与模型。真实翻译留到 M04。

## 开发环境

统一使用 [mise](https://mise.jdx.dev/) 管理工具与任务，Rust 版本固定在 `mise.toml`。Cargo.lock 固定依赖解析结果。不要另行配置 Rust 工具链版本。

Arch Linux / CachyOS 原生依赖示例（需自行确认系统包操作）：

```sh
sudo pacman -S --needed base-devel pkgconf clang cmake fontconfig freetype2 \
  libxkbcommon libxkbcommon-x11 wayland libxcb openssl zstd vulkan-headers \
  vulkan-icd-loader pipewire
```

安装适合 GPU 的 Vulkan 驱动和中文字体，例如 `noto-fonts-cjk`。音频采集需要运行中的 PipeWire 与提供默认输出 metadata 的会话管理器（本机验证为 WirePlumber）。

```sh
mise trust
mise install
mise run doctor
mise run check
mise run run
# 启动离线字幕演示
mise run demo
```

控制窗口可在 Wayland 或 X11 会话运行；字幕浮窗需要支持 `zwlr_layer_shell_v1` 的 Linux Wayland compositor。X11 会明确提示不支持浮窗，不创建伪置顶窗口。

采集或演示运行时，关闭主控制窗口会保留会话，并打开小型恢复入口。点击「恢复控制窗口」继续操作，关闭恢复入口或点击「退出 Subveil」结束程序。系统托盘仍留到 M06。

`RUST_LOG=subveil=debug mise run run` 可调整日志级别，日志写入 stderr，不保存识别文本或音频。

## 系统音频采集

控制窗口内点击「开始系统音频采集」。默认捕获系统默认输出的 monitor，不采麦克风；没有默认输出时等待设备。切换默认输出或设备断开后自动重新发现，PipeWire 服务中断后每秒重试。界面区分等待、捕获与连接错误，并显示设备、电平、静音、PCM 样本、丢块和重连次数。

默认输出单声道 F32LE、48 kHz PCM。停止采集后可切换为 16 / 24 / 48 kHz，采样率是管线参数，后续由 ASR Provider 能力决定。PipeWire 完成混音和 sinc 重采样，不修改输出音量或强制图采样率。静音阈值为 RMS < -60 dBFS；连续 500 ms 无 PCM 时也清除电平，但连接状态会独立显示。

PCM 队列固定 16 块，每块最多 2048 个单声道样本；满时淘汰最旧块，超过 250 ms 的旧块不再消费。丢块与丢样本累计统计包含溢出、过期和流切换丢弃；无效缓冲单独计数。音频回调不等待 UI、不执行推理或网络操作。独立采集只计算电平；启动 ASR 时另用固定 16 块队列将 PCM 送入识别线程。输入溢出、过期、设备换代会重置识别上下文，避免拼接不连续的音频。

无需图形环境的本机诊断：

```sh
# 默认运行 10 秒，48 kHz；只打印统计，不保存音频
mise run audio:probe
# 运行 60 秒，输出 24 kHz PCM
mise run audio:probe -- 60 24000
```

诊断运行至时限后取消并等待线程退出。正常 GUI 启动不会自动采集；`--demo` 也只启动模拟字幕。音频采样率暂不持久化。Linux 以外平台会明确拒绝采集。

## ASR（M03）

在控制窗口选择 Mock / Deepgram / Sherpa、en / ja，停止采集后选择 PCM 采样率，再点击 ASR 启动按钮。ASR 拥有采集会话；请先停止独立采集。停止 ASR 同时停止采集、取消未完成结果、清空字幕；线程退出前不能再次启动。离线双语演示和 ASR 互斥，隐藏或恢复窗口不会重置识别会话。

- Mock 按能量输出带 `[Mock]` 标记的英语或日语预设样本，不识别实际语音。
- Deepgram 使用 `nova-3`、Listen v1、mono linear16 和选择的采样率。通过环境变量 `DEEPGRAM_API_KEY` 提供凭据；不写入配置或日志。选择云端后点击「同意上传并开始 Deepgram ASR」才连接并上传默认输出音频，可能产生服务费用；点击停止撤销本次上传。没有凭据会拒绝启动，不回退其他后端。
- Sherpa 需要可选构建特性及自行提供的 streaming transducer 模型。普通构建会明确提示缺少特性。它调用 OnlineRecognizer，属于真实 streaming API；不包含离线 Whisper 等模型的分块包装。

本地构建与模型配置（路径为示例，请替换）：

```sh
# 官方 sherpa-onnx 1.13.8 原生运行库可由 SHERPA_ONNX_LIB_DIR 指定。
# 未指定时，上游依赖 build script 会下载对应版本原生库；不会下载模型。
export SHERPA_ENCODER=/path/to/streaming-model/encoder.onnx
export SHERPA_DECODER=/path/to/streaming-model/decoder.onnx
export SHERPA_JOINER=/path/to/streaming-model/joiner.onnx
export SHERPA_TOKENS=/path/to/streaming-model/tokens.txt
mise exec -- cargo run --locked -p subveil --features sherpa
```

模型须来自可信来源，部署前核对其许可证与发布方完整性校验值。应用没有模型下载器，不承诺任意模型支持日语或英语；本地语言选择只标记结果，实际语言由模型决定。设置 PCM 速率须匹配模型特征采样率。ONNX Runtime CPU 推理在后台线程执行，模型加载和单次原生推理调用本身无法强制中断；取消在调用之间生效。

基础 VAD 是能量分段，非 Silero：默认 RMS 0.01、最小有声 150 ms、起始时间前移最多 200 ms、尾部静音 600 ms、最长 15 s。Mock 使用全部参数；本地流持续接收 PCM（含前置静音），使用 sherpa 原生端点与尾静音/最大段长规则；云端使用服务端 endpointing 和真实 is_final。`VadConfig` 可由管线调用方配置，GUI 当前使用默认值，不持久化 ASR 设置。

结果队列固定 64 个事件，满时淘汰最旧并计数；UI 每 250 ms 消费。Partial 使用同一段递增 revision，Final 后新段递增。音频 250 ms 过期、序号跳跃、设备换代会清除当前识别上下文；字幕历史中的旧 Partial 按原 TTL 过期。云端握手 10 s、写入 3 s、无 PCM 2 s、无服务消息 30 s 超时；断开后按 2 / 4 / 8 s 重试，丢弃积压，不上传断线期间缓存。取消在约 20 ms 轮询检查点生效，已发送到服务端的数据不能撤回。

无窗口诊断，默认不输出识别文本：

```sh
mise run asr:probe -- mock 10 ja 16000
# 必须显式允许上传，凭据通过环境变量提供
mise run asr:probe -- deepgram 10 en 16000 --allow-upload
# 仅显式指定此选项时将识别文本输出到终端
mise run asr:probe -- mock 10 en 16000 --print-transcripts
# 本地诊断
mise exec -- timeout 90s cargo run --locked -p subveil-asr --features sherpa --example asr_probe -- sherpa 10 en 16000
mise run asr:check
```

单次诊断 1–60 秒；`asr:check` 检查可选后端构建、严格 Clippy 和测试，不要求模型或付费凭据。合成音频与内存 WebSocket 测试证明协议和状态行为，不代表真实语音准确率或延迟。

## 字幕样式

控制窗口可调整字体、字号、译文颜色、背景不透明度、宽度、底部距离、行数、显示模式和鼠标穿透。只在点击「保存样式」时持久化，保存操作在后台执行。

默认配置路径为 `$XDG_CONFIG_HOME/subveil/subtitle.toml`；XDG_CONFIG_HOME 未设置或不是绝对路径时，使用 `$HOME/.config/subveil/subtitle.toml`。无有效目录时明确提示，可手动指定路径：

```sh
mise exec -- cargo run --locked -p subveil -- --demo --config /tmp/subveil-subtitle.toml
```

完整选项见 [配置示例](config/subtitle.example.toml)。修改 TOML 后重启加载；无效配置显示错误并暂用默认值，不自动覆盖原文件。当前不支持热重载。

默认显示原文与译文，每种语言最多 3 行；超长文本换行后省略。字幕静默 8 秒后清除，最多保留 32 条历史。仅原文、仅译文模式可切换。停止演示会取消当前模拟会话并清空字幕；隐藏浮窗继续保留会话。

M01 使用屏幕布局中最左、其次最上方的输出，宽度与高度按该输出约束。显示器选择及热插拔适配留到 M06。当前 Hyprland 已验证普通与全屏测试视频叠加；其他 compositor 和独占全屏场景不保证相同行为。

## 常用任务

| 命令 | 用途 |
| --- | --- |
| `mise run build` | 锁定依赖构建 |
| `mise run run` | 启动控制窗口 |
| `mise run demo` | 启动离线 mock 双语浮窗 |
| `mise run audio:probe -- 10 48000` | 无窗口音频统计诊断 |
| `mise run asr:probe -- mock 10 en 16000` | 无窗口 ASR 诊断 |
| `mise run asr:check` | 可选 sherpa 后端构建、Clippy、测试 |
| `mise run fmt` | 格式化 |
| `mise run fmt:check` | 格式检查 |
| `mise run clippy` | 全目标严格检查 |
| `mise run test` | Workspace 测试 |
| `mise run check` | 顺序运行全部质量检查 |
| `mise run doctor` | 检查工具链与原生依赖 |

## 工程与文档

Workspace 包含 `subveil` 应用、`subveil-core` 核心状态、`subveil-audio` 音频适配器与 `subveil-asr` 识别后端。核心状态和配置不依赖 GPUI，设备与付费 API 均不参与核心测试。GUI 使用 GPUI 与 gpui-component，同一版本的 gpui-pre 快照提供一致的类型。

- [开发规范](AGENTS.md)
- [完整开发规划](docs/development-plan.md)
- [技术调研与架构限制](docs/technical-research.md)
- [M00 验收记录](docs/m00-validation.md)
- [M01 实施计划](docs/m01-plan.md)
- [M01 验收记录](docs/m01-validation.md)
- [M02 实施计划](docs/m02-plan.md)
- [M02 验收记录](docs/m02-validation.md)
- [M03 实施计划](docs/m03-plan.md)
- [M03 验收记录](docs/m03-validation.md)

必须使用 Conventional Commits，按可审查的小步勤提交。项目许可证待所有者确认，尚未授予开源许可证。
