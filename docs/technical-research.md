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

## M03 云语音协议调研（2026-10-20）

先通过 Context7 resolve/query 检索各家官方资料，无法完整返回的文档再核对官方页面。OpenAI 同时使用 OpenAI Docs 工作流，Azure 使用微软官方协议。服务协议来源、模型、凭据和能力差异统一记录于 `docs/cloud-services.md`。

- 保留 Deepgram Listen v1；新增阿里 LiveTranslate、Google Live、火山 AST v2、腾讯实时语音翻译、百度实时语音翻译、OpenAI Realtime Translation、Azure OpenAI 和 xAI Voice。提供自定义 DashScope 与 OpenAI GA 转写端点。
- 阿里源文 delta 按 item_id 累加，译文通过 previous_item_id 关联。腾讯依据 sentence_id / sentence_end，百度分开处理 MID 的 asr 和 FIN 的 sentence，避免下一句预览混入已确定原文。
- OpenAI 的 elapsed_ms 是对齐元数据，不是唯一事件 ID；相同时间可有多个 delta，不能按时间去重。OpenAI / Azure / Google 使用单独、有界的连续原文和译文窗口，不虚构逐句配对或 Final。
- xAI 的 input_audio_transcription.updated 是累计修订，不是 append delta。response_id 绑定已提交的回合；后来的讲话不能改变既有译文关联。
- 火山 AST v2 使用新版 X-Api-Key 与资源头，WebSocket 二进制载荷为 Protobuf，不套用旧音频协议头。通过 prost 0.14.4 实现所需字段。官方页面可在浏览器读取；附件下载未完成，当前字段实现仍须实际服务样本复核。
- 腾讯参数排序后做 HMAC-SHA1、Base64 和查询参数编码；使用独立已知向量测试签名。每次重连刷新 nonce / voice_id，不打印完整 URL 或鉴权材料。
- 保持 tokio-tungstenite 0.28.0 与现有 GPUI 版本。新增 protobuf、安全 URL、HMAC、Base64 直接依赖；移除 sherpa-onnx 及其原生构建依赖。本地模型推迟至 M04，前期原生绑定探索不再作为当前实现或验收结论。

音频回调继续只处理 PCM；后台识别线程驱动网络。初始化确认后才发送音频，各服务以规定包长节奏发送；不把积压瞬间灌入接口。原文与译文分别有界，译文带完整 SegmentKey；UI 只接受匹配 revision 的返回。返回语音始终丢弃，不播放形成采集反馈。


## GPUI Kit 界面重构补充（2026-10-20）

按用户指定的 GPUI Kit Design Guides / Coding Guides 与技能重构控制、样式预览、浮窗展示和恢复入口。Context7 返回的示例为 0.7；网站首页展示 0.7.3。通过 crates.io 与实际发布源码核对，采用兼容现有工程的 `gpui-kit =0.8.0`，保留 `gpui-pre =0.3.9` 和 `gpui-component 0.8.0`。应用只声明 Kit，一个 facade 提供框架、组件、平台与图标资源。依赖树确认只有一套 GPUI 类型。

使用标准 TabBar、Select、RadioGroup、Switch、Button 和滚动条；主题来自 ActiveTheme，界面间距与字体使用 rem 辅助方法。用户配置的字幕颜色、字号以及窗口/Layer Shell 几何是明确的内容数据与平台边界，不跟随界面主题换色。字幕样式预览与浮窗共用 subtitle_panel。会话与识别生命周期仍由 Session 持有，UI 不创建服务或发起上传。详细设计与验收见 [ui-redesign.md](ui-redesign.md)。
