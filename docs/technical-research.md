# M00 技术调研

调研日期：2026-10-20。先查询 Context7，再核对已发布 crate 与官方源码；主线文档可能领先或落后于发布包。

## 结论与版本

| 项目 | 选择 / 状态 | 依据 |
| --- | --- | --- |
| Rust | Stable 1.99.1，edition 2024，mise 固定 | 本机 mise 工具链实际执行结果 |
| GPUI | `gpui-pre =0.3.9`，依赖名 `gpui` | gpui-component 0.8.0 发布元数据要求精确快照 |
| 平台实现 | `gpui-pre-platform =0.3.9`，依赖名 `gpui_platform` | 平台与 GPUI 使用同一快照；启用 font-kit、wayland、x11、runtime_shaders |
| UI 组件 | `gpui-component =0.8.0` | crates.io 已发布版本，初始化 `init`，窗口根使用 `Root::new` |
| 日志 | tracing 0.1 + tracing-subscriber 0.3 | Cargo.lock 固定实际解析版本，stderr 输出，可用 RUST_LOG 过滤 |
| PipeWire | 调研 0.10.1；M00 不引入依赖 | 发布源码的 audio-capture 示例，Rust 最低版本 1.80 |
| ASR / 翻译 / Tokio | 后续里程碑按需引入 | M00 无推理与网络任务，避免增加未使用依赖 |

`gpui-pre 0.3.9` 的发布元数据标注 Zed revision `93d745668805029476e6d7fbb19652116fed51de`，Zed crate version 0.2.2。这是上游代码的发布快照，并非本项目 fork；不能因此宣称 GPUI API 已稳定。

原需求中的 `gpui-ce/gpui-component` 与当前 `longbridge/gpui-component` 路线已分化。前者主线使用 `gpui-ce`，后者发布包 0.8.0 使用 `gpui-pre`。本项目选择后者，避免不同 GPUI 来源产生不兼容类型。不要将未经核对的 Context7 示例直接复制进代码。

## Wayland Layer Shell

已核对 **所选发布版** 的 `src/platform/layer_shell.rs`、`src/platform.rs` 与 `src/window.rs`：

- `WindowKind::LayerShell(LayerShellOptions)` 存在，需要 Linux + wayland feature。
- `Layer::Overlay`、`Anchor::BOTTOM`、边距与 `KeyboardInteractivity::None` 可表达字幕窗口基本要求。
- `WindowBackgroundAppearance::Transparent` 控制透明背景。
- `Window::set_input_region(Some(&[]))` 将输入区域置空，文档说明仅 Linux Wayland 支持指针和触摸穿透；`None` 恢复默认输入区域。
- 缺少协议时存在 `LayerShellNotSupportedError`，不能用普通顶层窗口静默伪装相同能力。

M00 仅创建普通控制窗口。上述为源码能力确认，**不是浮窗运行验收**。M01 必须独立验证透明、底部定位、无键盘焦点、鼠标穿透、多屏与全屏播放。GNOME 等环境的协议支持及独占全屏行为不得一概承诺。X11 后端仅用于普通控制窗口兼容，不代表 Layer Shell 在 X11 可用。

## PipeWire API 与风险

0.10.1 发布包示例使用 `main_loop::MainLoopRc::new`、`context::ContextRc::new`、`connect_rc`、`stream::StreamBox::new`，再通过 listener 接收格式变化和处理缓冲。示例明确展示 `STREAM_CAPTURE_SINK = "true"` 的 monitor 捕获选项，以及 `TARGET_OBJECT` 的目标选择。

示例默认 Capture 不代表自动选择系统输出 monitor。M02 仍需实现默认 sink metadata 发现、目标跟踪、重连、格式协商、时间戳和有界队列。上游示例中的终端打印、expect 和处理方式不能原样放入实时回调。M00 没有连接 PipeWire 或录音。

## 环境与架构

本机：Arch Linux，Wayland 会话，Rust 1.99.1；wayland-client 1.26.0、xkbcommon 1.13.2、PipeWire 1.6.9、Vulkan 1.4.363、fontconfig 2.18.3、FreeType 26.7.20、OpenSSL 3.6.5、zstd 1.5.7。

初始 Workspace 只有 `crates/subveil`：应用入口、日志、窗口生命周期与控制视图。没有业务模型时不建立 core 空库。后续出现真实需求再将数据模型、管线和平台适配拆出。GPUI 负责事件循环；未来 Tokio 运行时必须通过明确通道与 UI 交互，不能阻塞 UI。

风险：GPUI 及组件库仍快速变化；完整组件库会引入较大传递依赖；首次编译需要网络、磁盘和内存；Linux 运行需要 GPU/Vulkan 驱动、字体及图形会话。精确固定 GPUI 快照与 Cargo.lock，降低 API 漂移。`runtime_shaders` 是平台 crate 提供的 macOS shader 编译选项，与组件库推荐配置一致，不改变 Linux GPU 驱动要求；macOS 未经验证。

开源许可证尚未由项目所有者指定，M00 不擅自选择。发布前须确认项目许可证、依赖许可证及资源授权。

## 官方来源

- [GPUI 上游](https://github.com/zed-industries/zed/tree/main/crates/gpui)
- [上游 Layer Shell 示例](https://github.com/zed-industries/zed/blob/main/crates/gpui/examples/layer_shell.rs)
- [gpui-pre 0.3.9 源码](https://docs.rs/crate/gpui-pre/0.3.9/source/)
- [gpui-component 0.8.0 源码](https://docs.rs/crate/gpui-component/0.8.0/source/)
- [组件库主线 Workspace](https://github.com/longbridge/gpui-component/blob/main/Cargo.toml)
- [需求引用的 CE 分支](https://github.com/gpui-ce/gpui-component/blob/main/Cargo.toml)
- [PipeWire 0.10.1 捕获示例](https://docs.rs/crate/pipewire/0.10.1/source/examples/audio-capture.rs)
- [PipeWire 官方音频捕获示例](https://docs.pipewire.org/audio-capture_8c-example.html)
- [mise Rust 工具链](https://mise.jdx.dev/lang/rust.html)
- [mise 任务配置](https://mise.jdx.dev/tasks/task-configuration.html)

验收结果见 `docs/m00-validation.md`；不以文档调研替代运行验证。

## M01 实施补充（2026-10-20）

- 保持所有 GPUI 与组件库版本不变；新增真实使用的 `subveil-core`，隔离配置、字幕状态与窗口实现。
- 同步开窗可能立即调用 Render，窗口创建必须在持有 Session Entity update 之外执行。使用 `App::defer` 后，控制窗口、浮窗、恢复入口均已运行验证。
- 所选 Wayland 后端 `primary_display()` 明确返回 None；不能用它判断无显示器。通过 `displays()` 枚举，按输出布局排序选择首个。
- Layer Shell 输入区域穿透、不抢焦点、透明、底部锚定与全屏视频叠加均在当前 Hyprland 实测；这不扩大为所有 compositor 的支持承诺。
- 核心状态拒绝旧 session / revision 以及已过期或淘汰的段，mock 计时器使用单调时钟。窗口重建不重置字幕。
- 配置使用 serde + TOML；原生文件读写在应用 storage 模块。用户显式保存时后台原子替换，加载错误持续展示，不自动覆盖坏配置。
- M01 计划和验收详见 `docs/m01-plan.md` 与 `docs/m01-validation.md`。

## M02 实施补充（2026-10-20）

Context7 查询 `/git_gitlab_freedesktop_org/pipewire_pipewire-rs` 后核对 pipewire 0.10.1 发布源码的 audio-capture、registry、metadata 和 loop API。使用安全的 Loop::iterate 有限超时驱动专用线程，无需应用 unsafe。

采用 PipeWire 内置 sinc 重采样：请求的流采样率与图采样率不同时自动转换，resample.quality=4；请求单声道交由原生混音。采样率是调用参数，不绑定未来模型为 16 kHz。不设置 node.force-rate 或修改系统输出音量。

默认输出由 default metadata 的 default.audio.sink JSON 和 Audio/Sink registry 共同确定；stream.capture.sink=true、target.object=node.name、node.dont-fallback=true 防止误采麦克风。服务中断后重建 context/core，输出切换重建流。

来源：[PipeWire 属性与重采样](https://docs.pipewire.org/page_man_pipewire-props_7.html)、[pipewire 0.10.1 发布源码](https://docs.rs/crate/pipewire/0.10.1/source/)。具体实现与验收见 M02 文档。

### M02 回调与缓冲约束

应用不设置 RT_PROCESS：process listener 在独立 PipeWire main loop 线程调度，避免将 Rc/RefCell 用户数据交给实时 DSP 线程。回调仅校验并复制 PCM；同一后台线程在 iterate 返回后消费固定容量队列和计算电平。GPUI 以 try_lock 读取聚合快照，不等待音频线程。

SPA chunk 按 offset / size / stride 校验，拒绝损坏标记与超过 256 KiB 的缓冲；EMPTY 标记使用等时长零样本，避免读取遗留数据。F32LE 用字节转换，无 unsafe cast。NaN/Inf 置零，幅值约束到 [-1, 1]。PCM 块携带连接代次、递增序号、累计样本偏移、采样率与单调接收时间；此时间不是设备硬件 PTS，M03 对齐时必须保留区别。

队列使用 crossbeam-queue ArrayQueue 预分配 16×2048 个样本，force_push 淘汰最旧数据；250 ms 过期块和流替换待消费块亦计入丢弃。metadata listener 使用 Weak 引用，移除对象时释放 listener/proxy，避免服务重连时形成引用环。stream listener 先于 stream 释放，registry 使用持有 CoreRc 的 RegistryRc。

依据：[Crossbeam ArrayQueue 源码](https://github.com/crossbeam-rs/crossbeam/blob/master/crossbeam-queue/src/array_queue.rs)、本机 SPA buffer.h 与 libspa 0.10.1 buffer API。Context7 已先查询 `/crossbeam-rs/crossbeam`，实际依赖固定在 Cargo.lock。

## M03 调研（2026-10-20）

Context7 先 resolve 再 query：`/websites/developers_deepgram`、`/websites/rs_sherpa-onnx_sherpa_onnx`、`/snapview/tokio-tungstenite`。核对官方发布源码，保留已有 GPUI 类型来源。

Deepgram Listen v1 的 nova-3 支持 en/ja；WebSocket 采用 Authorization: Token、linear16 单声道、显式 sample_rate、interim_results=true。is_final 稳定当前识别范围，speech_final 表示端点，二者不能混同。超时/断开重连后丢弃旧音频，不回放积压。来源：https://developers.deepgram.com/docs/models-languages-overview 、https://developers.deepgram.com/docs/understand-endpointing-interim-results 、https://developers.deepgram.com/docs/audio-keep-alive 。

官方 sherpa-onnx 1.13.8 发布包提供 OnlineRecognizer::create/create_stream、accept_waveform、is_ready/decode、get_result/is_endpoint/reset。默认静态绑定的 sys build script 可下载原生运行库；可用 SHERPA_ONNX_LIB_DIR 提供预安装库。本项目将其放入可选特性，不自动下载模型；模型来源、许可证和哈希由用户在部署时确认。来源：https://docs.rs/crate/sherpa-onnx/1.13.8/source/ 、https://docs.rs/crate/sherpa-onnx-sys/1.13.8/source/build.rs 。

Tokio WebSocket 由后台专用 runtime 驱动；握手、读写与取消均需有时限，限制服务响应大小。上游文档：https://github.com/snapview/tokio-tungstenite 。

### M03 实现补充

实际固定 `tokio-tungstenite 0.28.0`，核对该发布包的 IntoClientRequest、connect_async_with_config、WebSocketConfig builder 和 Message API；没有升级 GPUI。选择官方 `sherpa-onnx =1.13.8` 安全绑定，默认构建不链接模型运行库；`sherpa` 特性实际完成 Workspace 静态链接与测试。

M02 的固定容量 PCM 块在后台电平消费后转入独立 16 块 handoff；捕获回调不执行 ASR、网络或推理。ASR 单独线程持有本地 recognizer 或 Tokio current-thread runtime。输出 64 事件，满时淘汰最旧并计数；输入序号、样本偏移、代次、采样率和 250 ms 接收时间约束检测连续性。

本地模型使用官方 OnlineRecognizer streaming transducer API；只接收用户显式配置的模型文件，不提供自动下载或伪本地实现。Mock 的能量分段提供最小语音、起始时间前移、尾静音与最长段；真实本地后端持续喂流、使用原生 endpoint 规则；云端使用服务端 is_final。三者共享 TranscriptEvent，但不声称相同 VAD 算法或相同识别能力。

Deepgram 的 is_final 结束当前识别范围，不等待 speech_final。空 Final 关闭当前段编号，不把先前 Partial 猜成已确定文本。重连丢弃旧上下文并保留单调段编号；音频来源时间是累计样本时间，非硬件 PTS。停止取消上下文和未处理结果，不强制输出可能错误的 Final。
