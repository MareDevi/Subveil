# Subveil — 项目开发规划与实施规范

**项目名称：** Subveil  
**项目类型：** 开源、原生、跨平台实时字幕翻译应用  
**核心技术：** Rust + GPUI  
**初始目标平台：** Linux / Wayland / PipeWire  
**目标架构：** Linux-first, Cross-platform Ready  
**开发方式：** 里程碑驱动、渐进式实现、测试优先

---

# 1. 项目定位

Subveil 是一款基于 Rust 和 GPUI 构建的轻量级桌面实时字幕翻译应用。

它可以捕获计算机正在播放的系统音频，对语音进行实时识别，并将原文及翻译以透明悬浮字幕的形式显示在屏幕上。

主要使用场景：

- 观看 YouTube、直播、动漫、电影。
- 观看 F1 等国际体育赛事的外语解说。
- 参与跨语言语音会议。
- 玩没有中文字幕的游戏。
- 日语、英语等外语的听力学习。

## 1.1 核心设计原则

1. **Native First：** 只使用 Rust + GPUI 构建桌面 GUI，不使用 Electron、Tauri 或 WebView。
2. **Low Latency：** 优先减少音频采集、识别、翻译和字幕显示的总延迟。
3. **Modular：** 音频采集、ASR、翻译和字幕渲染相互解耦。
4. **Local & Cloud：** 支持本地模型与云端服务自由组合。
5. **Privacy First：** 本地处理优先可用；云端音频传输必须明确告知用户。
6. **Cross-platform Ready：** 初期支持 Linux，架构层保留 Windows/macOS 扩展能力。
7. **Non-intrusive：** 字幕悬浮窗不能影响正常操作、视频播放和游戏输入。
8. **Maintainability：** 避免过度设计，保持清晰的模块边界与可测试性。

# 2. 平台支持规划

## 第一阶段：Linux

重点环境：

- Arch Linux / CachyOS
- Hyprland / Wayland
- PipeWire + WirePlumber
- x86_64
- Intel / AMD / NVIDIA GPU

Linux 为首个正式支持平台。

必须兼容常见 Wayland 桌面，不得将整个程序与 Hyprland 专有 IPC 绑定。

## 第二阶段：Windows

- WASAPI Loopback 音频捕获
- 原生透明悬浮窗口
- 系统托盘与全局快捷键
- 单独进程音频捕获（能力允许时）

## 第三阶段：macOS

- CoreAudio / ScreenCaptureKit
- 原生透明字幕悬浮窗
- 音频权限管理
- Apple Silicon 优化

Windows/macOS 后端在初期只定义接口，不创建空壳实现或声称可用。

# 3. 技术栈

| 模块 | 推荐技术 |
|---|---|
| 编程语言 | Rust Stable |
| GUI | GPUI |
| UI 组件 | gpui-component（按需） |
| 异步执行 | Tokio |
| Linux 音频 | pipewire-rs |
| 音频重采样 | rubato 或经验证的替代方案 |
| VAD | Silero VAD |
| 本地 ASR | sherpa-onnx |
| 云端 ASR | 可插拔流式 ASR Provider |
| 云端翻译 | DeepL / OpenAI-compatible API |
| 本地翻译 | 可插拔本地模型后端 |
| 网络 | reqwest / WebSocket |
| 序列化 | serde |
| 配置 | TOML |
| 日志 | tracing |
| 错误处理 | thiserror / anyhow |
| 单元测试 | cargo test |
| 代码质量 | cargo fmt / clippy |

所有库、API、功能参数及版本必须通过官方文档确认，不允许臆造依赖版本或 GPUI API。

对 sherpa-onnx 必须区分真正的 streaming recognizer 与离线模型的分块调用，不能把分块离线识别宣传为真正的流式识别。

# 4. 系统架构

整体数据流：

```text
                    Subveil
                       |
                 App Controller
                       |
             +---------+---------+
             |                   |
         GPUI GUI           Core Pipeline
             |                   |
       Control Window       AudioCapture
       Subtitle Overlay          |
       Settings                  |
                              Audio Frames
                                 |
                              Resampler
                                 |
                            VAD / Segmenter
                                 |
                             ASR Engine
                                 |
                         Partial/Final Text
                                 |
                          Translation Queue
                                 |
                          Translation Engine
                                 |
                           Subtitle State
                                 |
                         GPUI Overlay Window
```

## 4.1 建议的 Rust Workspace

```text
subveil/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── LICENSE
├── AGENTS.md
├── crates/
│   ├── subveil/            # 可执行程序、应用生命周期
│   ├── subveil-ui/         # GPUI 控制界面
│   ├── subveil-overlay/    # 悬浮字幕窗口
│   ├── subveil-core/       # 公共模型、配置、接口
│   ├── subveil-audio/      # 采集、重采样、VAD
│   ├── subveil-asr/        # ASR Provider
│   ├── subveil-translate/  # Translation Provider
│   └── subveil-pipeline/   # 实时任务调度
├── assets/
├── docs/
└── tests/
```

上述为目标结构，不要求在 M00 一次性创建所有 crate。

应优先建立最小可编译 Workspace，根据真实需求逐步拆分，避免无意义的空模块。

# 5. 核心接口设计

## 5.1 AudioCapture

要求：

- 枚举系统音频设备。
- 捕获默认输出设备的播放音频。
- 支持音频源切换。
- 支持采集开始、停止、恢复。
- 传递 PCM 数据及时间戳。
- 处理设备断开、默认设备变化。

后续支持：

- 单独进程音频采集。
- 多音频源。
- 麦克风输入。
- 同时监听麦克风与系统音频。

音频采集回调不得执行 ASR 推理、网络请求或阻塞等待。

## 5.2 SpeechRecognizer

每个 ASR Provider 应实现统一接口，具体形式根据 Rust 所选依赖的线程安全与生命周期约束设计。

必须能表示：

```rust
enum TranscriptEvent {
    Partial(TranscriptSegment),
    Final(TranscriptSegment),
}
```

每个 Segment 至少具有：

- Session ID
- Segment ID
- 文本内容
- 语种
- 音频开始时间
- 音频结束时间
- 结果修订版本
- Final 状态

支持：

- 指定语言
- 自动识别语言（仅在后端支持时）
- 重置会话
- 取消识别
- 后端错误处理
- 分段与端点检测

不能将同一个语音段的每次 Partial 更新视作独立字幕。

## 5.3 Translator

支持：

- 指定源语言和目标语言
- 自动语言检测（后端支持时）
- 上下文翻译
- 自定义翻译 Prompt
- 可取消的翻译任务
- 限流与重试
- 超时处理
- 不同 Provider 切换

初期 Provider：

1. OpenAI-compatible API
2. DeepL
3. Mock Translator（用于开发测试）

后续：

- 本地 LLM
- 专用神经机器翻译模型
- 更多翻译 API

每个翻译请求必须关联 Session ID、Segment ID 和修订版本，避免过期结果覆盖最新字幕。

## 5.4 SubtitleState

字幕状态必须区分：

- 正在识别的原文
- 已确定的原文
- 正在翻译
- 已完成翻译
- 翻译失败
- 已过期的字幕

支持：

- 原文 + 翻译
- 仅原文
- 仅译文
- 自动清除过期字幕
- 多行排版
- 长句换行
- 字幕历史

# 6. GUI 设计

## 6.1 控制窗口

使用 GPUI 和 gpui-component。

整体风格：

- 现代、简洁、轻量
- 深色/浅色主题
- 清晰的信息层级
- 保持一致的间距与字体
- 不复制其他软件的 UI

主要页面：

### Home

- 开始/停止字幕
- 音频源选择
- 源语言选择
- 目标语言选择
- 当前 ASR Provider
- 当前翻译 Provider
- 音频输入状态
- 实时字幕预览

### ASR Settings

- 本地/云端模式
- Provider 选择
- 模型选择
- 识别语言
- VAD 参数
- 模型管理

### Translation Settings

- Provider 选择
- API 配置
- 目标语言
- 翻译 Prompt
- 上下文长度

### Subtitle Settings

- 字体与字号
- 文字颜色
- 字幕背景颜色
- 透明度
- 屏幕位置
- 双语布局
- 最大显示行数
- 原文与译文字号比例
- 点击穿透开关

### General Settings

- 自启动（后期）
- 系统托盘（后期）
- 快捷键
- 语言
- 主题
- 日志
- 隐私选项

## 6.2 字幕悬浮窗

Linux Wayland 优先研究 GPUI 原生 Layer Shell 能力。

目标：

- 透明背景
- 支持 Overlay Layer
- 不抢夺键盘焦点
- 鼠标点击穿透
- 多显示器定位
- 支持调整宽度与位置
- 支持全屏视频场景
- 可以隐藏与恢复
- 窗口与主控制界面独立

优先参考 GPUI 官方：

https://github.com/zed-industries/zed/blob/main/crates/gpui/examples/layer_shell.rs

如果上游 GPUI 的特定能力不足，应先研究现有 API、限制与替代方案，不要立即 fork 整个 GPUI。

明确区分 Wayland Layer Shell、普通顶层窗口以及 X11 always-on-top 行为。

不要假设 Overlay Layer 在所有 compositor 和所有独占全屏场景下一定可见。

# 7. 音频处理

## 7.1 Linux PipeWire

MVP 必须支持默认输出设备音频的 Loopback/Monitor 捕获。

处理过程：

1. 发现 PipeWire 节点。
2. 获取默认音频输出。
3. 建立监视或 loopback 捕获流。
4. 读取音频帧。
5. 必要时转换声道与采样率。
6. 将 PCM 帧送入有界队列。
7. 交给 VAD 或 ASR。

优先适配常见的 48 kHz 系统输出，并根据模型需求重采样。

不能硬编码所有模型均要求 16 kHz。

## 7.2 并发要求

音频实时线程不得：

- 执行网络请求
- 等待翻译结果
- 执行耗时模型推理
- 阻塞等待 UI
- 无限增长缓存

使用有界队列与明确的过载处理策略。

ASR/翻译消费者落后时应记录事件，并采取合理的丢帧、重置或背压策略，不允许无限累积延迟。

## 7.3 VAD

支持：

- 静音检测
- 语音起始检测
- 语音结束检测
- 最小语音长度
- 最大分段长度
- 前置缓冲
- 尾部静音缓冲

所有 VAD 参数均应可配置，并有合理默认值。

# 8. 实时字幕与翻译策略

这是本项目的核心。

必须采用增量事件机制，而不是固定间隔翻译全部历史内容。

建议流程：

1. ASR 输出 Partial，更新屏幕原文。
2. ASR 修订 Partial，仅更新对应 Segment。
3. 检测到稳定句子或 Final。
4. 将稳定文本提交翻译。
5. 翻译完成，更新对应字幕。
6. 若翻译结果已过期，则丢弃。
7. 保留有限数量的历史字幕。

优化方向：

- Partial 去抖
- Stable Prefix 检测
- 句子边界检测
- 翻译请求合并
- 上下文窗口
- 结果乱序处理
- 超时取消
- 延迟统计

不要在每次 Partial 文本变化时立即无条件调用翻译 API。

## 延迟测量

记录：

- 音频帧时间戳
- ASR Partial 到达时间
- ASR Final 到达时间
- 翻译开始与完成时间
- 字幕提交显示时间

提供可选开发者统计：

- ASR 延迟
- 翻译延迟
- 端到端延迟
- 音频队列深度
- 丢帧数

初期延迟目标：

- 云端流式 ASR 原文尽可能在语音开始后 1 秒左右出现
- 译文尽可能在稳定分段后 1–2 秒内出现
- 字幕 UI 更新流畅、不闪烁
- 内存占用在持续运行时保持稳定

以上为优化目标，不是承诺值，必须通过真实网络、语言和模型进行基准测试。

# 9. 安全、配置与隐私

配置目录遵循 XDG Base Directory Specification。

推荐：

```text
~/.config/subveil/
~/.local/share/subveil/
~/.cache/subveil/
```

要求：

- 设置持久化。
- API Key 不写入日志。
- 优先使用系统安全凭据存储。
- 无法安全存储凭据时给出明确提示。
- 日志默认不保存音频和完整语音内容。
- 音频云端传输应向用户清晰展示。
- 支持完全关闭云端服务。
- 不内置第三方付费 API Key。
- 不静默上传用户音频。
- 模型下载提供来源、许可信息和完整性校验。

# 10. 开发里程碑

## M00 — 技术验证与工程初始化

目标：建立可靠、可编译的工程基础。

任务：

- 检查开发环境。
- 检查 GPUI 当前稳定可用版本及上游示例。
- 检查 GPUI Layer Shell API。
- 检查 pipewire-rs 当前 API。
- 验证 gpui-component 与 GPUI 的版本兼容性。
- 建立最小 Cargo Workspace。
- 实现最小 GPUI 控制窗口。
- 配置 tracing。
- 建立 README、AGENTS.md。
- 建立 fmt/clippy/test 检查流程。
- 编写架构说明与已发现的技术限制。

验收：

- `cargo build` 成功。
- `cargo fmt --all -- --check` 成功。
- `cargo clippy --workspace --all-targets` 无警告，或记录有依据的例外。
- 可以启动 GPUI 窗口。
- 无未解释的占位实现。

## M01 — 字幕浮窗

目标：实现可用的 Wayland 字幕显示。

任务：

- GPUI Layer Shell 窗口。
- 透明背景。
- 底部锚定。
- 原文与译文双行显示。
- 鼠标穿透。
- 不抢焦点。
- 可配置字体、透明度与位置。
- Mock 字幕驱动。
- 字幕状态管理。

验收：

- Hyprland 下能显示。
- 普通视频播放时正常置顶。
- 鼠标能操作字幕下方窗口。
- 主控制窗口关闭/打开不会破坏字幕状态。
- 长文本可正常换行。

## M02 — PipeWire 音频采集

目标：稳定捕获系统播放音频。

任务：

- 默认输出设备检测。
- PipeWire monitor 捕获。
- PCM 帧处理。
- 重采样。
- 音量电平显示。
- 设备断开恢复。
- 有界队列及异常处理。

验收：

- 播放视频时能采集到音频。
- 停止播放后检测到静音。
- 不产生明显播放干扰。
- 连续运行 30 分钟无明显资源泄漏。
- 记录队列溢出与丢帧。

## M03 — ASR Provider

目标：系统音频转实时文字。

实施顺序：

1. Mock ASR。
2. 首个可用云端 ASR。
3. 真实 Partial/Final 处理。
4. 基础 VAD/分段。
5. 扩展阿里云、Google Gemini、火山引擎、腾讯云、百度、OpenAI、Azure OpenAI、xAI，以及自定义 DashScope / OpenAI Realtime 协议；显示接口自带译文。
6. 本地 ASR 与模型加载移到 M04，本里程碑不引入模型运行库。

要求重点测试日语与英语。

验收：

- 可以从系统播放内容生成原文。
- Partial 文本可被正确修订。
- Final 文本稳定。
- 断网、超时和服务异常可恢复。
- 本地和云端 Provider 使用统一数据模型。

## M04 — 本地 ASR 与实时翻译

目标：完整打通系统音频到双语字幕。

任务：

- 本地 sherpa-onnx streaming ASR、用户模型导入和加载验证。
- 本地与云端识别使用统一字幕数据模型；不静默回退云端。
- Translator Trait。
- OpenAI-compatible Provider。
- DeepL Provider。
- Prompt 模板。
- 翻译队列。
- 翻译上下文。
- 过期结果处理。
- 错误显示。

验收：

- 日语音频能显示中文字幕。
- 英语音频能显示中文字幕。
- 翻译错误不阻断 ASR。
- 旧请求不会覆盖新字幕。
- 可切换翻译服务。

至此完成首个 MVP。

## M05 — 本地模型与模型管理

2026-10-21：按用户明确要求提前实施应用内模型目录、显式下载/校验/选用，以及 SenseVoice 日语分段识别。详见 `docs/model-library-validation.md`。这不代表 GPU、本地翻译或整个 M05 已验收。

任务：

- 完善 sherpa-onnx 集成。
- 模型下载与导入。
- 模型信息与磁盘空间显示。
- 模型加载/卸载。
- CPU/GPU Provider 检测。
- 本地翻译后端。
- 离线模式。

验收：

- 至少一种语言组合实现完全离线字幕翻译。
- 模型不可用时明确说明。
- 不存在静默回退云端的问题。

## M06 — 产品化

任务：

- 字幕历史与 SRT 导出。
- 系统托盘。
- 全局快捷键。
- 自动恢复上次设置。
- 多屏支持。
- 中英文 UI。
- AppImage。
- Arch Linux PKGBUILD。
- AUR 发布准备。
- 性能优化。
- 错误诊断。
- 完善用户文档。

## M07 — 跨平台

依次推进：

1. Windows WASAPI。
2. Windows 原生浮窗。
3. macOS CoreAudio/ScreenCaptureKit。
4. macOS 原生浮窗。

平台能力通过 Adapter 抽象实现，不允许散落大量无组织的 `cfg` 分支。

# 11. 测试要求

至少包括：

- 配置序列化/反序列化测试。
- 音频缓冲边界测试。
- 采样率转换测试。
- Transcript 修订测试。
- 翻译乱序返回测试。
- Session 取消测试。
- Provider 超时测试。
- 设备切换测试。
- 长时间运行测试。

建立一个可重复使用的测试音频集，以 WAV 文件驱动 ASR/Pipeline 测试。

不能完全依赖真实音频设备或真实付费 API 才能运行核心测试。

# 12. Codex 工作规范

以下规则必须严格执行：

1. 开始每个里程碑前先检查项目当前状态。
2. 优先查阅官方文档和实际源码，不凭记忆编造 API。
3. 实施前列出该里程碑的简短任务清单。
4. 完成代码后运行构建、格式化检查、Clippy 和测试。
5. 编译失败时优先修复，不继续堆积功能。
6. 不为尚未实现的能力创建假成功返回值。
7. 不使用 `unwrap()` 随意处理运行时外部错误。
8. 保持音频处理、网络、推理与 GPUI 渲染线程之间的明确边界。
9. 避免不必要的依赖、unsafe 代码和全局可变状态。
10. 实现中的架构变更同步更新文档。
11. 每个里程碑结束时输出：已完成、测试结果、已知问题、下一步。
12. 必须优先完成当前里程碑，不得擅自提前实施全部功能。
13. 有条件时实际运行程序验证；不能实测的功能必须明确标记未经验证。
14. 对暂时无法支持的平台或功能，应提供真实的错误提示而非静默忽略。

# 13. 重要参考资料

- GPUI: https://github.com/zed-industries/zed
- GPUI Layer Shell: https://github.com/zed-industries/zed/blob/main/crates/gpui/examples/layer_shell.rs
- GPUI Component: https://github.com/gpui-ce/gpui-component
- PipeWire: https://pipewire.pages.freedesktop.org/pipewire/
- pipewire-rs: https://crates.io/crates/pipewire
- sherpa-onnx: https://github.com/k2-fsa/sherpa-onnx
- Sokuji: https://github.com/kizuna-ai-lab/sokuji

参考项目仅用于学习实现方式，不直接复制其代码。复用时须遵守对应开源许可证。

# 14. 第一次执行指令

现在开始实现 Subveil 项目。

**只执行 M00：技术验证与工程初始化。**

执行顺序：

1. 检查当前工作目录和已有文件，避免覆盖现有工作。
2. 检查 Rust 工具链以及 Linux 所需系统依赖。
3. 调研当前 GPUI、gpui-component 和 Wayland Layer Shell 支持情况。
4. 给出初步技术结论与重要风险。
5. 选择能够实际编译的依赖版本。
6. 创建最小 Rust Workspace。
7. 实现并运行最小 GPUI 控制窗口。
8. 设置项目的代码质量检查和基础日志。
9. 创建 README.md、AGENTS.md 与基础技术文档。
10. 执行编译、Clippy 和测试。
11. 总结 M00 验收结果。

**不要立即开发音频采集、语音识别和翻译。**

M00 的目标是获得一个干净、可编译、可维护、技术路线经过验证的项目基础，而不是生成一大堆尚未集成的代码。

如遇到 API 不兼容或技术限制，先核对官方源码，明确记录替代方案，优先解决当前问题。

完成 M00 后汇报结果，后续按 M01 → M02 → M03 → M04 顺序继续实施。
