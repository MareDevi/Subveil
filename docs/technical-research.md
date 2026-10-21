# M00 技术调研

## 字幕可靠性修复（2026-10-21）

本次复核用户提供的审查结论：SubtitleState 的显示 TTL 同时限制 finish_translation / matching，确实会丢弃翻译 worker 截止时间内的迟到结果；浮窗只使用 current；stop 调用 reset；SenseVoice 在 PCM 消费循环中 decode。修复设计及边界见 [subtitle-reliability.md](subtitle-reliability.md)。

Context7 先 resolve sherpa-onnx，再查询 `/websites/rs_sherpa-onnx_sherpa_onnx`。查询确认 create_stream / accept_waveform / decode / get_result 调用顺序；线程约束另核对本机锁定版本 sherpa-onnx 1.13.8 `src/offline_asr.rs`，OfflineRecognizer 由上游实现 Send / Sync。本次把唯一 recognizer 移入专用线程，不新增 unsafe。原生 decode 没有取消接口，停止仍可能等待当前 decode 返回；等待只发生于后台 ASR worker。没有因此宣称进程崩溃隔离或可抢占取消。

来源：[OfflineRecognizer](https://docs.rs/sherpa-onnx/1.13.8/sherpa_onnx/struct.OfflineRecognizer.html)。GPUI 继续沿用现有 div、Button、主题与 Layer Shell API；已阅读 Design Guides / Coding Guides，不升级依赖。

## 全应用界面重构调研（2026-10-21）

本轮先检查 Git（main 干净）、mise Rust 1.99.1 与 doctor。现有工程已使用 `gpui-kit =0.8.0`，无需迁移依赖。Context7 `/longbridge/gpui-kit` 与本机发布源码共同核对 Sidebar、SidebarMenuItem、StatusBar、Progress、Icon API；保留单一 facade 和 Root。

完整阅读 GPUI Kit Design Guides / Coding Guides。采用 Zed 式稳定侧栏、平面分区与紧凑状态栏；参考 Mimi 的字幕、声音与语言、模型与服务、显示样式任务划分，不复制其 Web/Tauri 实现。主任务仍是实时字幕，服务与模型分离，原有业务状态、明确上传授权及停止行为保留。布局使用主题 token 与 rem，原生窗口及用户字幕内容几何仍是已说明的像素边界。

来源（2026-10-21）：[GPUI Kit](https://gpui-kit.com/)、[Design Guides](https://gpui-kit.com/docs/design-guides/)、[Coding Guides](https://gpui-kit.com/docs/coding-guides/)、[Mimi](https://github.com/yuxino/Mimi)、[Zed appearance](https://zed.dev/docs/appearance)。本轮计划及验收记录见 [ui-redesign.md](ui-redesign.md)。

## M05 追加：原生翻译与 GPU（2026-10-21）

- Context7 resolve/query 后核对 llama-cpp-2 0.1.160 与实际 Cargo 源码：`LlamaBackend::init`、`list_llama_ggml_backend_devices`、`LlamaModelParams::with_devices/with_n_gpu_layers`、`LlamaModel::chat_template/apply_chat_template`、`LlamaVocab::tokenize/is_eog/token_to_piece`。主线旧示例中的 `model.str_to_token`、`model.is_eog_token` 不适用于当前发布版；使用实际 vocab API。
- Vulkan 原生构建需要 CMake、Clang、Vulkan headers/loader、glslc；Arch 的 shaderc 包提供 glslc，CI Ubuntu 显式安装 glslc。不改系统驱动或用户 Vulkan 配置。
- GPU 枚举和模型加载都来自 llama 原生设备 API，不将 nvidia-smi 结果当作 GPU 推理证明。使用显式设备索引和 1000 层卸载上限，小模型全部可卸载层交由所选 GPU；CPU 明确设置 0 层及空设备列表。加载前检查模型大小加 512 MiB 的可用显存估计，实际分配失败仍返回错误。算子可能由运行库在 CPU/GPU 间分配，不宣称每个算子均在 GPU。
- 同进程链接 sherpa 预编译静态库与 GCC 16 编译的 llama 时，在进入 main 前发生 `std::bad_alloc`；GDB 栈落在 `INVALID_RULE_CHARS_RE` 的 C++ regex 初始化。最终隔离为独立 Rust 推理程序，未修改 sherpa、GPUI 或系统工具链。主程序启动后原生探测、CPU/GPU 翻译均恢复正常。
- IPC 使用 stdin/stdout JSON 行，单条最多 64 KiB，结果带请求编号，父进程再关联 Session/Segment/revision。专用读取线程加容量 1 的通道；推理等待每 10 ms 检查取消及截止时间，故障终止子进程并明确失败。原文不作为命令行参数、临时文件或日志传递，子进程 stderr 不转存。
- 模型权重与上下文归子进程，停止后终止并回收。模型加载最长 30 秒，GUI 等待翻译模型准备后再打开采集，避免加载期间产生待翻译音频积压。GPU 失败不自动改用 CPU；本地翻译失败时原文识别可以继续，并显示错误。
- 本地翻译每句单独使用模型内置 chat template，最多 1536 输入 token、256 输出 token、2048 上下文；不沿用云端三句上下文。超限明确失败，不显示截断译文。文本模型目录的采样率标记为 0，表示不接收音频，不能用于 ASR。
- sherpa 继续采用 1.13.8 CPU 静态库；其 CUDA provider 源码在编译能力不足时可能回退 CPU，本轮未包装成虚假的 CUDA 选项。GPU 支持范围为 Qwen 本地文字翻译。

来源：<https://docs.rs/llama-cpp-2/0.1.160/llama_cpp_2/>，<https://github.com/utilityai/llama-cpp-rs>，<https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.8/sherpa-onnx/csrc/session.cc>。验收见 `docs/m05-validation.md`。

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

## M04 调研（2026-10-20）

Context7 resolve-library-id / query-docs 后核对 crates.io 与 1.13.8 源码：官方 sherpa-onnx 提供 OnlineRecognizer::create、create_stream、is_ready、decode、get_result、is_endpoint、reset。这是持有流状态的真实 streaming API。选择 CPU transducer，模型输入速率来自用户清单。默认 static 通过 sherpa-onnx-sys 构建脚本链接官方预编译库；SHERPA_ONNX_LIB_DIR 可指定自建库。没有在应用内下载模型。

DeepL POST /v2/translate 使用 DeepL-Auth-Key、text 数组、target_lang、可选 source_lang 与 context。OpenAI-compatible POST /chat/completions 使用 Bearer、model 和 messages，读取 choices[0].message.content。远程端点仅 HTTPS，允许本机 HTTP；禁止 URL 凭据、查询参数、片段及自动重定向。应用默认不传文本，独立显式授权后才启用。

来源：
- https://docs.rs/sherpa-onnx/1.13.8/sherpa_onnx/
- https://github.com/k2-fsa/sherpa-onnx
- https://developers.deepl.com/api-reference/translate
- https://developers.deepl.com/docs/learning-how-tos/examples-and-guides/how-to-use-context-parameter
- https://github.com/openai/openai-openapi

M04 运行验证补充：reqwest 锁定 0.13.5，使用 rustls-no-provider 与现有 ring，显式初始化进程级提供者，避免 aws-lc / ring 混用。公开英文测试模型为 csukuangfj/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17，模型卡声明 Apache-2.0。下载至 /tmp 的 ONNX 通过发布方 LFS SHA-256，其他文件通过发布方 Git blob ID 校验；不提交模型或音频。日语官方 ReazonSpeech Zipformer 属于 offline 模型，不能用于当前 streaming transducer 后端；当前未选定并验证日语 streaming 模型。

- https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17/blob/main/README.md
- https://k2-fsa.github.io/sherpa/onnx/pretrained_models/offline-transducer/zipformer-transducer-models.html

## 应用内模型目录与 SenseVoice（2026-10-21）

用户明确要求应用内选择、下载和运行，提前实施 M05 模型下载子集。选择固定 revision 的英文 20M INT8、中英 Zipformer INT8、SenseVoice Small INT8。目录文件从发布方元数据获得长度与 LFS SHA-256；tokens / README 经 Git blob ID 校验后固定 SHA-256。下载模型与 ASR、翻译分开，安装不开始音频采集。

官方 Rust OfflineRecognizer 支持 SenseVoice ONNX、tokens、language、use_itn；当前用 CPU 4 线程和有界能量 VAD，仅生成 Final。这解决可运行日语模型缺口，且不混淆 streaming 与 offline。源语种按模型声明选择，未启用自动语种检测。既有 streaming 清单保持向后兼容。

下载器使用 std::fs::File::try_lock（本机 Rust 1.99.1）管理同包安装互斥，锁随进程/文件句柄关闭释放。逐文件临时发布与最后提交清单，避免损坏内容标为已安装。完整缓存文件重校验后复用；未实现 HTTP Range 续传。

硬件、模型固定版本、完整条款、实测和未验证项见 [模型选型与运行验收](model-library-validation.md)。当前 GPU 虽可检测，但运行库仍为 CPU，不表示 CUDA 执行已实现。


## Whisper CUDA 与短块字幕（2026-10-21）

Context7 先 resolve / query `/tazz4843/whisper-rs`，再核对 crates.io 发布包与实际源码。固定 whisper-rs 0.16.0、whisper-rs-sys 0.15.0，内含 whisper.cpp 1.8.3。采用安全 API `WhisperContextParameters`、`FullParams`、`set_abort_callback_safe`、`WhisperState::as_iter`，应用继续 forbid unsafe。CUDA 13.4 / GCC 15 / SM89 在本机编译通过；CUDA 不参与默认 CPU CI。

上游 GPU 初始化失败可保留 CPU 后端。安全 Rust API 未暴露运行设备查询，因此独立进程安装不输出文本的 `log_backend` 日志接收器，检查固定版本的 CUDA 后端选择、CUDA 权重分配及原生错误标记。缺少标记即失败，升级依赖需复核标记并重测，不能根据 CUDA 编译开关或 NVIDIA 设备枚举宣称推理成功。实际推理还须通过公开 WAV 解码与进程显存观察。

Whisper 与 sherpa、llama 分属独立原生链接单元。PCM 使用最多 1024 字节 JSON 请求头加最多 30 秒单声道 16 kHz 小端 f32 数据；响应限制 64 KiB，文本限制 16 KiB。独立写线程防止大 PCM 被阻塞管道卡住取消；CUDA 10 秒 / CPU 30 秒推理期限、30 秒加载与预热期限由父进程执行，错误毒化引擎并终止进程。模型完整性由 helper 在原生加载前校验。

音频消费线程执行能量 VAD 与最长 2 / 4 / 8 秒分块，解码线程只接收容量 1 的等待队列；过载替换最旧等待块，超过 1 秒仍未开始的等待块丢弃。结果队列容量 2。音频换代 / 中断递增 epoch，旧结果不得变成新会话字幕；Session / Segment / revision 的字幕与翻译保护继续生效。分块是 Final，不伪造 Whisper streaming Partial。前置缓冲按实际保留范围约束，连续分块不复用已提交音频。

Whisper 与 Qwen 均在报告 Ready 前执行固定公开输入预热；预热不含用户内容，也不将结果发送 UI。冷启动与热态延迟分开记录。使用说明、硬件实测与未验证项见 [GPU ASR 验收](gpu-asr-validation.md)。

来源：[whisper-rs 发布 API](https://docs.rs/whisper-rs/0.16.0/whisper_rs/)、[sys 发布源码](https://docs.rs/crate/whisper-rs-sys/0.15.0/source/)、[固定 Whisper 模型](https://huggingface.co/ggerganov/whisper.cpp/tree/5359861c739e955e79d9a303bcbc70fb988958b1)。
