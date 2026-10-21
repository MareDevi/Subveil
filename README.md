# Subveil

Rust + GPUI 原生跨平台实时字幕翻译应用，优先支持 Linux / Wayland / PipeWire。

当前阶段 **M05：本地模型与离线字幕**。应用内安装并校验语音识别及文字翻译模型；SenseVoice / Zipformer CPU 识别与 Whisper Turbo CPU / CUDA 短块识别配合 Qwen 本地翻译；翻译可选择 CPU 或 Vulkan GPU。支持完全离线模式、模型导入、磁盘空间检查、进度与取消、停止卸载和本地设置保存。真实日英离线链路、RTX 4060 CUDA 识别与 Vulkan 翻译已验证，长时稳定性及完整 GUI 交互仍待验收。云端语音与文字服务继续保留，分别授权上传。入门见 [M05 使用与验收](docs/m05-validation.md)，新增 CUDA 使用与性能记录见 [GPU ASR 验收](docs/gpu-asr-validation.md)。

## 开发环境

统一使用 [mise](https://mise.jdx.dev/) 管理工具与任务，Rust 版本固定在 `mise.toml`。Cargo.lock 固定依赖解析结果。不要另行配置 Rust 工具链版本。

Arch Linux / CachyOS 原生依赖示例（需自行确认系统包操作）：

```sh
sudo pacman -S --needed base-devel pkgconf clang cmake fontconfig freetype2 \
  libxkbcommon libxkbcommon-x11 wayland libxcb openssl zstd vulkan-headers \
  vulkan-icd-loader shaderc pipewire
```

可选 Whisper CUDA 后端另需系统 CUDA toolkit 和兼容的主机 C++ 编译器；本机 CUDA 13.4 配合 `gcc15`，不静默安装系统包。`scripts/cuda.sh` 优先使用已安装的 `g++-15`，默认目标为本机 Ada SM89；其他 NVIDIA GPU 构建前设置 `CMAKE_CUDA_ARCHITECTURES`。默认 CPU 构建与 CI 不需要 CUDA。

安装适合 GPU 的 Vulkan 驱动和中文字体，例如 `noto-fonts-cjk`。音频采集需要运行中的 PipeWire 与提供默认输出 metadata 的会话管理器（本机验证为 WirePlumber）。

```sh
mise trust
mise install
mise run doctor
mise run check
mise run run
# 启动离线字幕演示
mise run demo
# 打开本地模型目录，不自动下载或采集
mise run models
# NVIDIA CUDA 专用构建与模型窗口
mise run doctor:cuda
mise run models:cuda
```

控制窗口使用 GPUI Kit 原生侧栏，包含「实时字幕、语音服务、模型库、字幕样式、运行诊断」五页。`Ctrl+1` 至 `Ctrl+4` 依次打开实时字幕、语音服务、字幕样式、运行诊断，`Ctrl+5` 打开模型库，`Ctrl+B` 折叠或展开侧栏。`Ctrl+Shift+O` 显示或隐藏浮窗，`Ctrl+S` 保存样式，`Ctrl+Shift+L` 切换明暗外观。恢复入口支持 `Enter` 恢复控制窗口。主题与侧栏折叠不持久化。模型安装、来源与许可证在独立「模型库」中查看；开始识别与上传授权仍在「语音服务」。设计与实测记录见 [原生界面重构](docs/ui-redesign.md)。

控制窗口可在 Wayland 或 X11 会话运行；字幕浮窗需要支持 `zwlr_layer_shell_v1` 的 Linux Wayland compositor。X11 会明确提示不支持浮窗，不创建伪置顶窗口。

采集或演示运行时，关闭主控制窗口会保留会话，并打开小型恢复入口。点击「恢复控制窗口」继续操作，关闭恢复入口或点击「退出 Subveil」结束程序。系统托盘仍留到 M06。

`RUST_LOG=subveil=debug mise run run` 可调整日志级别，日志写入 stderr，不保存识别文本或音频。

## 系统音频采集

在「实时字幕」选择音源，再在「运行诊断」点击「开始独立采集」或启动字幕。默认捕获系统默认输出的 monitor；也可显式选择特定输出、输入节点或单个应用播放流。指定流消失时不会回退到系统混音，需刷新并重新选择。默认输出模式跟随输出切换，PipeWire 服务中断后每秒重试。界面显示设备、电平、静音、PCM 样本、丢块和重连次数。`mise run audio:isolation` 在临时空输出上验证两路合成音隔离，不保存音频。

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

## 云语音服务（M03）

在「语音服务」选择服务、源语种提示与目标语种。接口要求固定采样率时自动使用 16 或 24 kHz；自动识别服务会明确显示「自动识别」。停止独立采集后点击「同意上传并开始」才向界面列出的目标发送系统音频；Mock 仅输出样本。

GUI 凭据从应用内配置读取，可选择保存到系统密钥环；CLI 保留环境变量兼容。接口自带译文可随原文修订，迟到的旧 revision 不会覆盖新字幕。OpenAI、Azure、Google 以有界连续窗口展示，不伪造逐句 Final；其他协议使用服务本身的句段标识。服务生成的音频不播放、不保存。

实时页直接选择音源、配置档案、语种、识别与翻译，并显示启动前缺项。语音服务页可命名保存最多 32 个配置；非敏感选择保存在 XDG 配置中，密钥按配置隔离在系统密钥环。切换配置会撤销音频和文本上传授权。旧版默认凭据仍可读取；不会自动读取密钥或开始采集。

密钥环需要 Secret Service 和 `secret-tool`：Arch 安装 `libsecret`，Debian/Ubuntu 安装 `libsecret-tools`，并启动、解锁桌面的密钥环服务。界面会说明工具和会话总线是否存在；是否能读写以实际操作结果为准。没有密钥环时仍可只在内存配置。

完整服务清单、环境变量、模型与部署要求、协议差异见 [云语音服务配置](docs/cloud-services.md)。本地 ASR、模型清单与独立文字翻译见 [M04 配置](docs/local-asr-and-translation.md)。

```sh
mise run asr:probe -- mock 3 ja 16000
# 先配置对应服务的环境变量；以下命令会向所选服务上传音频。
mise run asr:probe -- alibaba 10 en 16000 --allow-upload
mise run asr:probe -- openai 10 ja 24000 --allow-upload
mise run asr:check
```

诊断默认不输出识别文本，单次最多 60 秒；只有显式 `--print-transcripts` 才打印原文和译文。`asr:check` 检查所有 ASR 目标，并链接本地模型运行库。配置和协议测试通过不等于真实服务质量、权限或费用验收通过。

GUI 在「语音服务」直接填写凭据、端点和部署名，可选系统密钥环保存。OpenAI / Gemini / Deepgram 可查询兼容语音模型；独立 OpenAI-compatible 翻译可获取文字模型。环境变量仅保留给 CLI 诊断。模型发现不发送字幕，音频与文字上传仍分别授权。

本地目录新增 Whisper Small 和 Qwen2.5 0.5B。实验性「增量翻译」允许 Zipformer 的稳定 Partial 提前翻译并在 Final 纠正；Whisper / SenseVoice 仍为离线分块。见 [本轮说明与验收](docs/cloud-streaming-validation.md)。

## 字幕样式

「字幕样式」可调整字体、字号、译文颜色、背景不透明度、宽度、底部距离、行数、显示模式和鼠标穿透；内置样本与浮窗共用渲染。只在点击「保存样式」时持久化，保存操作在后台执行。

默认配置路径为 `$XDG_CONFIG_HOME/subveil/subtitle.toml`；XDG_CONFIG_HOME 未设置或不是绝对路径时，使用 `$HOME/.config/subveil/subtitle.toml`。无有效目录时明确提示，可手动指定路径：

```sh
mise exec -- cargo run --locked -p subveil -- --demo --config /tmp/subveil-subtitle.toml
```

完整选项见 [配置示例](config/subtitle.example.toml)。修改 TOML 后重启加载；无效配置显示错误并暂用默认值，不自动覆盖原文件。当前不支持热重载。

默认显示原文与译文，每种语言最多 3 行；超长文本换行后省略。浮窗最多显示两个句对：已完成译文按片段顺序获得至少 3 秒阅读位置，同时显示最新原文。显示默认 8 秒过期，但迟到且匹配的译文仍可补全历史，不重新抢占浮窗；默认最多保留 32 条历史，全部可在实时页回看。有界历史淘汰仍可能丢弃尚未阅读的条目。仅原文、仅译文模式可切换。停止取消推理与上传并保留记录；清空记录需单独操作，新会话开始时重置记录。隐藏浮窗继续保留会话。

实时页可搜索当前记录，手动保存 TOML 字幕档案，或导出 TXT、Markdown、双语 SRT。仅保存点击时仍保留的 Final 和已完成译文；不会自动保存音频、草稿或整个无限会话。打开档案不会覆盖当前实时会话。文件含私人文本，Unix 下新文件权限为 0600；每个档案最多 512 段、8 MiB。

模型安装支持固定版本文件的 Range 续传；取消与网络中断保留片段，重试时完整校验 SHA-256。错误范围拒绝续传，服务器忽略 Range 时从头覆盖，坏片段删除且不激活。模型库支持确认后卸载所选缓存版本和片段；不会删除导入目录，运行和安装期间不可卸载。

M01 使用屏幕布局中最左、其次最上方的输出，宽度与高度按该输出约束。显示器选择及热插拔适配留到 M06。当前 Hyprland 已验证普通与全屏测试视频叠加；其他 compositor 和独占全屏场景不保证相同行为。

## 常用任务

| 命令 | 用途 |
| --- | --- |
| `mise run build` | 锁定依赖构建 |
| `mise run build:cuda` | 含 Whisper CUDA 后端的锁定构建 |
| `mise run check:cuda` | CUDA 构建、严格 Clippy 与测试 |
| `mise run run:cuda` | 启动支持 Whisper CUDA 的应用 |
| `mise run run` | 启动控制窗口 |
| `mise run demo` | 启动离线 mock 双语浮窗 |
| `mise run models` | 打开本地模型目录 |
| `mise run model:install -- --list` | 列出固定版本本地模型 |
| `mise run translate:probe --devices` | 使用原生推理后端检测 GPU |
| `mise run translate:probe MANIFEST cpu` | 固定日英文本的本地翻译诊断 |
| `mise run offline:probe ASR_MANIFEST TRANSLATION_MANIFEST WAV ja gpu` | 完全离线字幕链路诊断 |
| `mise run audio:probe -- 10 48000` | 无窗口音频统计诊断 |
| `mise run asr:probe -- mock 10 en 16000` | 无窗口 ASR 诊断 |
| `mise run asr:check` | 全部 ASR 目标构建、Clippy、测试 |
| `mise run fmt` | 格式化 |
| `mise run fmt:check` | 格式检查 |
| `mise run clippy` | 全目标严格检查 |
| `mise run test` | Workspace 测试 |
| `mise run check` | 顺序运行全部质量检查 |
| `mise run doctor` | 检查工具链与原生依赖 |

## 工程与文档

Workspace 包含 `subveil` 应用、`subveil-core` 核心状态与推理协议、`subveil-audio` 音频适配器、`subveil-asr` 识别后端、`subveil-translate` 翻译调度、`subveil-models` 模型目录与下载器、`subveil-inference` 独立 GGUF 推理进程、`subveil-whisper` 独立 ASR 推理进程。安装时须将三个程序放在同一目录；`mise run build` 构建 CPU Whisper，`mise run build:cuda` 构建 CUDA Whisper。普通 `build` / `models` 会替换 helper 为 CPU 构建；使用 CUDA 时运行 `models:cuda` / `run:cuda`。核心状态和配置不依赖 GPUI，设备与付费 API 均不参与核心测试。GUI 统一依赖 GPUI Kit 0.8.0，通过 facade 使用 gpui-component 0.8.0 与 gpui-pre 0.3.9，保持一致的类型。

- [开发规范](AGENTS.md)
- [完整开发规划](docs/development-plan.md)
- [技术调研与架构限制](docs/technical-research.md)
- [Subveil / Mimi / Sokuji 源码对比与改进建议](docs/competitive-code-review-2026-10-21.md)
- [M00 验收记录](docs/m00-validation.md)
- [M01 实施计划](docs/m01-plan.md)
- [M01 验收记录](docs/m01-validation.md)
- [M02 实施计划](docs/m02-plan.md)
- [M02 验收记录](docs/m02-validation.md)
- [M03 实施计划](docs/m03-plan.md)
- [M03 验收记录](docs/m03-validation.md)
- [M04 实施计划](docs/m04-plan.md)
- [M04 配置](docs/local-asr-and-translation.md)
- [M04 验收记录](docs/m04-validation.md)
- [应用内模型库计划](docs/model-library-plan.md)
- [模型选型与运行验收](docs/model-library-validation.md)
- [M05 实施计划](docs/m05-plan.md)
- [M05 使用与验收](docs/m05-validation.md)

必须使用 Conventional Commits，按可审查的小步勤提交。项目许可证待所有者确认，尚未授予开源许可证。

字幕寿命、阅读调度和 SenseVoice 消费隔离的策略与验收见 [字幕可靠性](docs/subtitle-reliability.md)。
