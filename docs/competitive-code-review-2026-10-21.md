# Subveil、Mimi、Sokuji 源码对比与改进建议

审查日期：2026-10-21（Asia/Shanghai）。

## 结论

Subveil 已有可工作的原生离线字幕链路，并非只有窗口和模型目录。它的优势是 Rust + GPUI、直接 PipeWire 采集、明确的上传授权、带完整性校验的模型安装，以及字幕修订与过期结果隔离。当前主要问题是：**分段和上下文不足，限制字幕质量；配置与音源选择不足，限制日常使用；测试与分发仍偏开发阶段，限制可靠交付。**

Mimi 是最直接的桌面字幕对标对象。应优先学习它的音源选择、服务配置档案、预览与最终翻译分流、字幕历史、错误恢复和发布验证。Sokuji 的价值主要在本地模型适配、语音端点检测、分句和场景化配置；其双向语音、虚拟麦克风、TTS、会议接入属于产品扩展，不是 Subveil 当前字幕主任务必须补齐的缺陷。

不建议改用 Tauri 或 Electron，也不建议按模型数量竞赛。应先让既有模型在连续视频、音乐背景、日语长句和过载场景下输出更完整、可读的字幕。

## 1. 审查范围、证据与限制

实际执行了两个仓库的浅克隆，未安装或运行它们的依赖、构建脚本或应用，未调用付费 API、未采集系统音频。外部源码保留在 `/tmp/subveil-comparison/`，未加入 Subveil Git；临时目录可能被系统清理。

| 项目 | 本次审查提交 | 源码位置 |
| --- | --- | --- |
| Subveil | `9a0f5db98a9c60a243b3b90a35291aa85223f6c8` | 当前工作区，审查开始时干净 |
| Mimi | `33ada2263095d187418cf4bf21878ac2936187fe` | `/tmp/subveil-comparison/Mimi` |
| Sokuji | `9e789375ed59f45f65f1af9d5daf46bcb16687a2` | `/tmp/subveil-comparison/sokuji` |

证据分级：

- **实现已确认**：在入口、调用链、状态或协议代码中确认；不等于真机质量验收通过。
- **条件性风险**：代码存在触发路径，但依赖特定负载、系统条件或故障。
- **功能差距**：当前实现没有对应能力，不自动视为程序错误。
- **验证缺口**：不能据此断言程序必然失败，也不能声称已经可靠。

UI 比较基于页面组成、组件、状态与导航代码。没有进行三应用同机视觉、键盘、读屏、缩放或渲染性能对测，故不判定谁“更流畅”或像素布局优劣。Mimi README 的展示图明确是示意及旧版演示，不能当作当前 UI 验收证据。

所列模型和服务都是固定提交的快照，不保证账号可用、所有硬件可运行或全部经过真实推理测试。

## 2. 总体能力矩阵

| 维度 | Subveil | Mimi | Sokuji |
| --- | --- | --- | --- |
| 主任务 | 系统声音的实时双语字幕，原生离线优先 | 电脑/麦克风声音的桌面翻译字幕 | 双向会议语音翻译、字幕与音频路由 |
| 桌面架构 | Rust workspace + GPUI Kit | Tauri 2 + React/TypeScript + Rust shared runtime/core | Electron + React/TypeScript + Zustand；另有浏览器扩展 |
| 本地推理 | sherpa-onnx；Whisper helper；GGUF helper | Apple Speech、Apple Translation；未发现通用跨平台离线模型库 | 发布链路的 WASM/WebGPU workers；另有受开关控制的原生 sidecar |
| 当前平台能力 | 真实采集为 Linux PipeWire；浮窗要求 Wayland Layer Shell | macOS、Windows、Linux 原生适配；Android 共享 Rust 核心 | 桌面三平台、扩展；具体音源与路由受平台限制 |
| 音源 | 默认系统输出 monitor | 系统、麦克风、双路；平台相关应用/播放流选择 | 麦克风、系统、标签页及应用音频适配，输入/输出路由 |
| 原文和译文 | session/segment/revision；临时译文可修订 | 共享 reducer、实时对齐、草稿/Final 分流 | adapter/会话/会话内容层；句子封口、双向内容及字幕投影 |
| 本地流式 ASR | Zipformer 中英；日语目录无原生 streaming | Apple 平台服务相关能力；云 streaming 为主 | 多种 streaming 模型与 workers；并非所有 ASR 都 streaming |
| 独立文字翻译 | 本地 Qwen；OpenAI-compatible；DeepL | Qwen-MT、OpenAI-compatible、DeepL、DeepLX、Baidu、Apple 等路由 | Opus-MT、Qwen、HY-MT、TranslateGemma 等；目录也含在线 Bing |
| 字幕历史 | 默认 32 段、有界内存；停止保留 | 历史、会话档案、TXT/Markdown 导出；可选 WAV 保存 | 会话记录、字幕窗口、保存与局域网字幕分享代码 |
| 桌面便利性 | 页内快捷键、小型恢复窗口 | 托盘、全局快捷键、多窗口控制、更新 | 字幕窗口、设置向导、桌面音频设备和更新功能 |
| TTS/虚拟麦克风 | 无，当前字幕链路不需要 | 主任务不是译音输出 | 核心能力，带驱动/虚拟设备与回声处理复杂度 |
| 发布 | 单一 Linux CI；需源码构建并放置三个程序 | 桌面打包、平台 smoke、更新与签名验证脚本 | Electron/扩展发布、native/sidecar 构建工作流 |

主要源码入口：Mimi 的 [Cargo 配置](https://github.com/yuxino/Mimi/blob/33ada2263095d187418cf4bf21878ac2936187fe/src-tauri/Cargo.toml)、[共享 runtime](https://github.com/yuxino/Mimi/tree/33ada2263095d187418cf4bf21878ac2936187fe/shared/mimi-runtime/src)、[音源适配](https://github.com/yuxino/Mimi/tree/33ada2263095d187418cf4bf21878ac2936187fe/src-tauri/src/audio)；Sokuji 的 [package.json](https://github.com/kizuna-ai-lab/sokuji/blob/9e789375ed59f45f65f1af9d5daf46bcb16687a2/package.json)、[provider registry](https://github.com/kizuna-ai-lab/sokuji/blob/9e789375ed59f45f65f1af9d5daf46bcb16687a2/src/providers/registry.ts)、[本地 adapter](https://github.com/kizuna-ai-lab/sokuji/blob/9e789375ed59f45f65f1af9d5daf46bcb16687a2/src/providers/localInference/adapter.ts)。

## 3. 架构与实现方式

### 3.1 Subveil 的边界基本正确，编排层已经过重

现有八个 crate 分别承载应用、核心状态、音频、ASR、翻译、模型管理、GGUF 推理进程、Whisper 推理进程。核心字幕状态不依赖 GPUI，PCM 与结果队列有上限；停止和丢帧通过会话或 epoch 隔离迟到结果。这些设计应保留。

但 `crates/subveil/src/session.rs` 已达 1392 行，兼管模型安装/恢复、凭据、云模型发现、GPU 枚举、服务切换、音频、翻译和字幕调度。`ui.rs` 778 行，ASR/翻译能力还分散在 enum、表单、模型目录与 Session 分支中。问题不在文件长度本身，而在新增一种后端会同时触及多处状态和条件。

Mimi 的桌面 `clients`/`core` 多数已是共享 runtime 的转导出，不能把同名文件误算为两套实现。它的 `mimi-core` 提取字幕及翻译策略，`mimi-runtime` 持有网络与生命周期；Android 通过 JNI 复用。Sokuji 新 provider 定义把配置、凭据、语言、就绪检查、启动 adapter 和专属 UI 聚在一起，但仍有旧 factory、store 与原生实验路径，复杂度也高。

建议：保留 crate 划分，先从 Session 提取 `ServiceConfiguration`、`ModelOperations` 和纯数据的 session coordinator；统一能力描述，例如 source/target languages、streaming kind、device/backend、是否允许独立 MT、账号验证状态。不要为了对标增加无实际边界的新 crate。

### 3.2 推理隔离较强，隔离程度仍不一致

Whisper 与 GGUF 崩溃可由 helper 进程隔离；SenseVoice/Zipformer 仍在应用进程内，经线程/队列隔离，不能阻止原生库崩溃带走 UI。SenseVoice 已拆开消费和 decode，不应再把它写成“同步推理阻塞音频消费”的旧缺陷。

Mimi 的网络和 Apple 原生服务主要在 runtime 内；Sokuji 发布的本地路线使用 workers，原生路线有 Python sidecar + native binding，但受启用条件限制。不能将“worker”与“进程”当作完全相同的故障隔离。

建议统一 helper 生命周期、握手、超时、取消和状态协议；若真实 crash 证据表明必要，再把 sherpa 路线移入 helper。进程更多不自动等于延迟更低。

### 3.3 轮询增加可避免的延迟

Subveil [Session 定时器](../crates/subveil/src/session.rs#L80) 每 250 ms 驱动状态与结果消费。ASR Final 经 UI 消费后才提交独立翻译，译文返回也再等 UI 消费。因此存在两次 0–250 ms 的调度等待机会；这不是实测必然增加 500 ms，也不包含帧呈现延迟。

Mimi/Sokuji 通过事件、消息或 adapter 回调推进各层。建议让后台 coordinator 在收到 Final 时直接提交翻译，UI 按事件唤醒并合并刷新；资源统计可继续低频更新。必须保留现有 revision 校验，不能用降轮询间隔代替正确编排。

## 4. 音频、ASR、分段与翻译质量

### 4.1 音源选择是直接影响正确性的差距

Subveil `subveil-audio/src/linux.rs` 只跟踪 `default.audio.sink`，通过 `stream.capture.sink=true` 采集 monitor。自动重连和默认输出切换已实现，但无法只选浏览器、播放器或单个播放流，也没有麦克风输入入口。

影响：通知、游戏音效、音乐和视频会混入识别；云模式还可能上传用户没有意图翻译的其他程序声音。现有显式授权不消除“采集范围过宽”的问题。Mimi 的 `linux_streams.rs` 有播放流元数据、生命周期 token 和刷新；Sokuji 有 PipeWire 应用音频与跨平台 capture 适配。

优先增加可见的音源清单、当前目标、音源断开状态和“仅该播放流”策略。目标消失时不得静默退回全系统混音。麦克风是后续扩展，单应用音频应先做。

### 4.2 当前 VAD 是能量切分，不是语音检测

`subveil-asr/src/vad.rs` 明确采用 RMS 阈值 `0.01`、最短有声 150 ms、前滚 200 ms、尾静音 600 ms、默认最长 15 秒。它不能区分人声、音乐和爆炸，也可能漏掉低音量讲话。SenseVoice 等到段结束才解码，连续讲话可接近最长段才有结果。

Sokuji 有 Silero 输入、VAD worker、turn detector、标点与 SentenceStream，能够把“有声音”“停顿”“句子完成”分开处理。Mimi 多数云路线借服务自身端点和草稿提交策略，不能据此推断它所有离线路线都更好。

建议先给现有离线后端增加可选神经 VAD，并保留能量路径作低资源对照；分开配置 hangover、最长等待、标点封口和读取位置。验收需覆盖背景音乐、低声、短促词、长句与静音误字幕。

### 4.3 Whisper 短块已加速，但没有跨块语义连续性

当前 2/4/8 秒切块，`subveil-whisper/src/main.rs:149` 设置 `set_no_context(true)`，各块独立。分段器提交后清空已提交样本；没有跨块重叠、对齐去重或滚动原文上下文。

影响：2 秒边界可能切断单词、名字、助词和否定；后续翻译无法可靠恢复缺失内容。切换 8 秒可能改善上下文，但增加收集等待，不能同时保证完整句和 1–3 秒首字延迟。

建议评估有限重叠 + 时间对齐去重，或 VAD 触发后有界滚动解码；原文 draft 与稳定提交分开。不要把 Whisper 反复短块推理命名为原生 streaming。日语应单独选择可验证的 streaming 模型；现有可安装 Zipformer 只覆盖英/中，增量开关不能解决日语没有 Partial 的上游限制。

### 4.4 本地翻译缺少上下文，云/本地行为不一致

HTTP worker 已保留最多三句、每句最多 1024 字符的源文上下文，并发送给 DeepL 或 Chat Completions；因此“Subveil 完全没有翻译上下文”是错误结论。

但本地 `subveil-core/src/inference.rs` 的 `Input` 没有 context；`subveil-translate/src/local.rs` 只传当前 text/source/target；`generate.rs` 每次创建 2048 token context，仅放当前一段。输入上限 1536 token、输出 256 token，超限显式报错，不显示截断译文。

影响：本地路线更容易丢代词指向、人物名一致性和跨块句意；短块 ASR 会放大问题。每段重新分配 context 也有开销，但没有性能剖析前不能量化损失。

建议先增加有界“之前已确定原文 + 可选术语表”，明确源文是数据；再测缓存/上下文复用。上下文必须按 session/音源/语言重置，不能把旧会话带入新会话。不必预先引入无限聊天记忆。

### 4.5 稳定 Partial 已实现，Final 和预览仍竞争同一通道

增量调度要求三个假设的共同前缀、至少 12 字符、稳定 600 ms、提交间隔至少 1500 ms。英文取词界，中日取标点边界；短句或无标点时可能没有提前译文。这是保守策略，不是实现了所有语种的流式翻译。

HTTP 和本地共享容量 8 的输入队列，但执行路径不同：HTTP 每 10 ms 检查过时请求并可丢弃 future；本地 helper 等当前结果返回后才检查 superseded。慢临时推理会占住模型，Final 只能排队。不会覆盖新状态，不代表不会浪费算力或增加 Final 延迟。

Mimi 的 [high_quality_client](https://github.com/yuxino/Mimi/blob/33ada2263095d187418cf4bf21878ac2936187fe/shared/mimi-runtime/src/clients/high_quality_client.rs) 明确分 latest-only preview lane 与 serial final queue，并把预算提取到 core。建议 Subveil 也区分预览和 Final：Final 优先；预览合并；必要时取消旧预览；仍保留不可变前缀关联。验收要看 Final p95 和预览被修正比例，不能只看首次临时译文提前多少。

### 4.6 三秒阅读队列有吞吐上限

`SubtitleState::advance_presentation` 给一个阅读位置分配至少三秒，另一个位置显示最新原文；译文产生速度持续超过每三秒一条时，阅读队列会积压。默认 32 段历史最终可能淘汰未读条目。两秒 Whisper 产出不等于每两秒一定一个可读句对，但持续密集段确实可能超过阅读吞吐。

这比立即覆盖译文更好，仍缺字幕长度/阅读速度预算、合并短段、用户追赶历史等策略。Mimi 有最近 1–10 条/全部历史和浮窗滚动；Sokuji 有句子分段及字幕流。建议保留双位置，增加字符阅读预算与相邻短段合并，明确统计“识别/翻译/显示/淘汰”各阶段，避免把零队列溢出等同于用户读完。

## 5. 模型、服务与加速支持

### 5.1 Subveil 的可安装模型是七项

| 类型 | 模型 | 本项目实际边界 |
| --- | --- | --- |
| ASR | SenseVoice Small INT8 | CPU，离线能量分段，Final-only；应用只开放中/英/日 |
| ASR | Zipformer English 20M INT8 | CPU，英语 streaming，保留识别状态 |
| ASR | Zipformer Chinese/English INT8 | CPU，中英 streaming |
| ASR | Whisper large-v3-turbo Q5_0 | CPU / 专用 CUDA 构建；2/4/8 秒块；CUDA 当前仅设备 0 |
| ASR | Whisper Small Q5_1 | 已进入安装目录；新增权重的速度/质量未在最近验收实测 |
| MT | Qwen2.5 1.5B Instruct Q4_K_M | CPU / Vulkan；中英日；已有短样本离线实测 |
| MT | Qwen2.5 0.5B Instruct Q4_K_M | 已进入安装目录；新增权重的速度/质量未在最近验收实测 |

七项不等于七种架构，也不等于各模型所有原生语种均已开放。自定义清单导入不能把任意 ONNX/GGUF 变成受支持运行时；现有分支和 manifest kind 才是边界。

云 ASR 目录为 13 项，其中包括 Mock、本地与 11 个云端选项；后者包含 Deepgram、阿里 LiveTranslate、Gemini、火山、腾讯、百度、OpenAI、Azure、xAI、自定义 DashScope、自定义 OpenAI。不能把“11 个选项”理解成 11 家独立厂商。独立 MT 为本地、OpenAI-compatible、DeepL，以及关闭状态；自带译文的 ASR 当前禁止再选独立 MT。

### 5.2 Mimi 在云服务组合与能力约束上更完整

固定提交的 `ProviderKind` 包含 Alibaba、OpenAI、Gemini、Azure、火山、腾讯、百度、xAI、DeepLX 路线、自定义 DashScope/OpenAI、Apple Speech；独立翻译另有 Qwen-MT Lite/Flash/Plus、OpenAI-compatible、DeepL/DeepLX、Baidu、Apple 等配置。

最值得借鉴的是 [ProviderCapabilities](https://github.com/yuxino/Mimi/blob/33ada2263095d187418cf4bf21878ac2936187fe/shared/mimi-runtime/src/core/provider.rs#L474) 和服务 profile，不是机械补更多名称。Subveil 云端源语种按钮只在英/日间轮换，目标只在中/英/日轮换，服务本身支持更广语种也无法通过 UI 选用。语种列表还未统一按 ASR、MT、设备的实际交集生成。

Mimi 的 Apple 本地路线依赖 Apple 平台和系统条件，不能当成 Linux 通用离线优势。当前没有发现它有与 Subveil 同等的跨平台 GGUF/Whisper 下载管理能力。

### 5.3 Sokuji 模型生态更宽，但 README 数字不能照抄

README 写 44 ASR、75 MT、137 TTS。固定提交 `MODEL_MANIFEST` 范围内的字面声明统计为：35 个 `asr`、11 个 `asr-stream`、81 个 `translation`、139 个 `tts`、3 个 `punctuation`、1 个 `turn`。这是静态声明统计，不是逐模型运行验收；也不能与原生目录相加当作去重总数。

ASR 类型覆盖 SenseVoice、Moonshine、Whisper、Transducer/Zipformer、Paraformer、NeMo、Canary、Cohere、Voxtral、Granite、Qwen3-ASR 等；MT 有 Opus-MT 语言对、Qwen 2.5/3/3.5、HY-MT、TranslateGemma；TTS 有 Piper、Matcha、VITS、Supertonic 等。WebGPU 型号、dtype 和 shader-f16 等要求在目录与选择器中筛选。大型模型不会因为出现在清单里就在普通 CPU 上满足实时目标。

特别注意两点：

1. **原生路线存在，但不是默认发布能力的同义词。** `sidecar/` 有 Python/native ASR/MT/TTS，MT 包含 Qwen/Hunyuan/Gemma 策略；`isLocalNativeEnabled()` 仍检查开发环境、构建开关或 Electron 测试者开关。主 registry 与旧 factory 共存，不能把代码存在写成所有用户已可直接使用。
2. **“Local Inference”目录也有在线项。** `bing-translator` 和 `edge-tts` 显式标为 `isCloudModel: true`。这不证明离线模型会上传音频，但说明不能仅凭 provider 名称承诺整应用零网络；应检查最终选中的每个 stage。

证据：[模型清单](https://github.com/kizuna-ai-lab/sokuji/blob/9e789375ed59f45f65f1af9d5daf46bcb16687a2/src/lib/local-inference/modelManifest.ts)、[原生启用门槛](https://github.com/kizuna-ai-lab/sokuji/blob/9e789375ed59f45f65f1af9d5daf46bcb16687a2/src/utils/environment.ts#L182)、[原生 MT](https://github.com/kizuna-ai-lab/sokuji/blob/9e789375ed59f45f65f1af9d5daf46bcb16687a2/sidecar/sokuji_sidecar/translate_backend.py)。

Subveil 应优先补：可验证的日语 streaming、一个翻译专用小模型、统一硬件/语种兼容检查。TTS 大目录对当前字幕目标收益低。

### 5.4 GPU 边界与性能证据

Subveil 已明确区分 Whisper CUDA、Qwen Vulkan、sherpa CPU，并验证 CUDA 不静默回退 CPU。这比只展示 GPU 名称更可信。但当前 CUDA 仅设备 0，普通构建会替换 CPU helper，用户必须记住专用任务；也没有多模型联合显存预算、不同 GPU 资源预留或游戏并行帧时间验收。

已有记录中的日语/英语首条中文距有声起点约 2.20/2.16 秒，来自特定硬件、短公开 WAV 和字幕状态接收，不是本次新测，也不是屏幕呈现时间。此前 CUDA 日语 ASR release RTF 中位数约 0.028，CPU 英语约 2.60；不同语种和样本量不能作为严格同条件加速比。

未运行另两项目的同音频、同硬件推理，故不能给出 Subveil 比 Mimi/Sokuji 快多少、占用少多少的排名。GPUI 没有浏览器运行时是架构事实，低功耗或“不影响游戏”仍需实测。

## 6. UI、配置与可用性

| 用户任务 | Subveil 当前问题 | 对标与建议 |
| --- | --- | --- |
| 第一次出字幕 | 实时页主要入口是“配置语音服务…”和演示；真实启动在另一页 | Mimi QuickStartGuide、Sokuji 场景向导；增加“音源→语言→本地/云→模型/凭据→开始”的完整路径 |
| 选语言/设备/后端 | 多个按钮循环选项，不能先看到全部选择；语种集合过窄 | 改成可搜索 Select/Radio，展示不支持原因和实际能力交集 |
| 配本地链路 | 服务页选择 ASR，模型库选模型/设备，回来开始 | 从主页面显示当前链路与缺失项；模型详情为二级操作 |
| 切换服务 | 单一全局字段集合，没有命名 profile；选择与界面偏好不完整持久化 | 学 Mimi ServiceProfiles；账号与端点绑定，保存选择但不保存上传同意 |
| 保存凭据 | 要系统 `secret-tool`，用户需手动读取；doctor 不检查该运行依赖 | 显示密钥环可用性、保存/读取状态，补运行依赖检查；保持拒绝明文回退 |
| 看历史 | 32 段内存、无搜索/导出，停止保留但新会话清空 | 增加显式导出 TXT/SRT/VTT、可选会话保存；未选保存时继续内存模式 |
| 调浮窗 | 固定底部、不可拖动/调整；仅按坐标选最左上显示器 | 输出选择、热插拔、拖动或锚点设置、恢复不可见位置 |
| 关闭主窗后控制 | 小型恢复窗口占桌面；快捷键有上下文限制 | 依平台实现托盘与全局快捷键；穿透打开后仍需可发现恢复入口 |
| 发现故障 | 状态/计数不少，但多数是技术文字，缺按对象恢复动作 | 显示“音源/识别/翻译/浮窗”四阶段状态和下一步，不把错误全部压成重试 |
| 多语言/无障碍 | 可见文字主要硬编码中文；部分控件有名称，尚无完整交互验收 | 先测 Tab、焦点、下拉、错误提示、最小窗口和大字体，再扩展本地化 |

Subveil 已采用标准 GPUI 组件、语义主题、侧栏、页面快捷键、独立滚动和样式预览，不应说它完全没有 UI 结构。字幕内容颜色属于用户设置，不应把 `rgb(settings.translation_color)` 当成普通界面硬编码配色缺陷。

Sokuji 的场景、Simple/Advanced、双向 participant 路由很强，但对“看视频中文字幕”用户也可能过重。建议学习它的任务引导，不照搬所有会议控制。UI 审查尚不能证明三者的读屏、对比度或不同 DPI 已全面合格。

## 7. 已定位的可靠性风险

### R1：本地翻译写管道不受取消/期限保护（P1，条件性）

位置：`crates/subveil-translate/src/local.rs:262` 的 `write_all`/`flush` 在 `receive(deadline, stopped)` 之前同步执行。helper 若握手后停止读取，且请求大于可用管道容量，worker 会阻塞在写入，无法检查停止或进入超时清理；UI 不会因此同步卡住，但推理资源和停止保证可能失效。

本次机制复现：默认管道 65536 字节，16456 字节请求能写入，**未触发**；用 Linux `F_SETPIPE_SZ` 注入 4096 字节容量，子进程不读 stdin，同大小写入超过 200 ms 仍阻塞，外部 kill 后才释放。这证明低容量管道条件下机制成立，不是已在正常 Subveil 推理中复现挂死。

临时复现脚本：`/tmp/subveil-comparison/pipe-write-probe.py`；在 Subveil 目录运行 `mise exec -- python /tmp/subveil-comparison/pipe-write-probe.py`。每种条件不到一秒，无模型、设备或网络依赖；该文件未提交，临时目录清理后不保留。

Whisper helper 已有独立写线程和 `stalled_pcm_reader_does_not_block_cancellation` 测试；GGUF 翻译应采用同等保护，并测试写阻塞而不只测试等待响应超时。现有本地翻译测试未覆盖写入这个阶段。

### R2：云错误无类别地无限重试（P1，代码已确认）

位置：`subveil-asr/src/cloud.rs` 与 `protocols/mod.rs` 的 reconnect。错误均进入循环，延迟封顶 8 秒，未设总次数；成功运行后计数也没有按健康时长恢复。握手错误被泛化，认证失败、配置错误与临时网络错误无法可靠分流。

影响：错误密钥或无权限部署会一直重试；瞬时错误后也可能长期保持较慢重试。Mimi `core/recovery.rs` 有错误归类和最多四次恢复策略。建议保留脱敏，只抽取安全的 HTTP 状态/厂商错误码；401/403/配置错误停下并指向修改配置，429 尊重限流，网络故障有界退避。不要把任意服务响应原文直接放日志。

### R3：预览抢占不足与最终译文排队（P1，条件性）

位置与影响见 4.5。本地路径只在推理前后检查过时，已经失效的长预览仍可消耗到返回或期限。建议先以假 helper 注入慢预览，验证 Final 的最大等待，再决定取消或进程重建策略。不能为抢占每条预览都重新加载大模型。

### R4：历史、显示和模型状态仍缺端到端验收（P1，验证缺口）

现有状态测试覆盖晚到译文补历史、过期拒抢浮窗、三秒阅读、停止保留等；这些问题已经修过，不应重新列为未修复 bug。但文档仍明确双句对调度、历史滚动、全部凭据操作、焦点恢复等完整 GUI 交互未验收。需要把真实生产 View 纳入测试，而不只是测试对应状态函数。

## 8. 隐私、模型安装与发布

### 8.1 有些方面 Subveil 更严格，不应盲目照搬

Subveil 的音频与文本分别授权；离线模式拒绝云识别、文本上传和下载；无静默云回退；GUI 凭据不回退读取环境变量；模型固定 revision、准确大小和 SHA-256；验证成功最后发布 manifest。以上均有实现与回归覆盖。

Mimi 当前 [FileCredentialStore](https://github.com/yuxino/Mimi/blob/33ada2263095d187418cf4bf21878ac2936187fe/src-tauri/src/settings_store/file_credentials.rs) 使用 `credentials/credentials.json`，含私有目录/文件权限和迁移控制；原生密钥存储用于旧配置迁移。不能因为 Cargo 依赖 keyring 就称当前生产凭据都存系统密钥环。

Sokuji 的 `providerStore.setCredential` 经 SettingsService 保存，桌面为 localStorage，扩展为 `chrome.storage.sync`。这与 OS keyring 不同，sync 路径也不适合承诺凭据绝不参与浏览器同步。配置了 PostHog key 的构建还可发送分析事件；不能把“推理本地”扩张成“整应用任何时候无网络”。本次未检查服务端留存或发布二进制的实际配置。

Sokuji WASM 文件验证主要检查 HTML 错页、大小容差 20%、WASM/ONNX 文件头和 JSON 可解析；`hfRevision` 可选，部分使用默认 main。它有较广模型生态，但这部分并不比 Subveil 固定 hash 更强。原生 sidecar 下载机制需单独评价，不能用 WASM 校验结论概括全部后端。

### 8.2 Subveil 下载器的真实缺口是可恢复性

有缓存复用、取消清理、空间检查和安装锁，但单文件使用新 `.part`，无 Range 续传；失败后同一大文件需重下。尚缺模型卡搜索/筛选、明确推荐、卸载/空间回收、镜像与用户可控下载源等完整管理体验。

优先做断点续传和失败恢复，同时保留固定 revision/hash，不能为了下载方便放弃完整性验证。磁盘占用应按安装与临时文件共同预算，最终仍需重新校验。

### 8.3 发布闭环落后于另两者

Subveil `.github/workflows/ci.yml` 只在 Ubuntu 跑 doctor/check；没有发布安装包、三个二进制一致性检查、干净机器 smoke、GPU capability 握手、跨平台采集或浮窗测试矩阵。`build` 与 `build:cuda` 复用输出名称，普通构建覆盖 CUDA helper 是已文档化但容易误操作的限制。

Mimi 有 Linux 包、Windows/macOS 包、签名/更新检查、平台 smoke 与多种 UI 验证脚本；Sokuji 有桌面/扩展构建、native build、sidecar bundles 及更新逻辑。这说明存在更完整的交付机制，不说明当前每条工作流或发布包都经过本次验证。

建议先交付 Linux CPU/CUDA 能力可识别的包；启动握手显示 helper build/backend 版本；CI 做缺失 helper、错配版本和干净环境启动测试。Windows/macOS 目前应写成规划中的平台边界，不应把“Rust 可跨平台编译”表达成音频/浮窗已跨平台可用。

## 9. 优先级与验收建议

这里 P1 表示直接影响字幕可靠性或主要使用路径，P2 表示重要产品/工程差距；没有足够证据声称存在需要紧急修复的 P0。

| 顺序 | 工作 | 优先级 | 可审查验收 |
| --- | --- | --- | --- |
| 1 | 统一 helper 写入、等待、取消/超时保证 | P1 | 4 KiB 管道、不读 stdin、半写退出、停止期间无残留 helper；假进程测试不依赖模型 |
| 2 | 云恢复错误分类 | P1 | 401/403 不重试；429/网络错误有界退避；恢复后状态正确；日志仍脱敏 |
| 3 | 神经 VAD + 句子边界 + Whisper 跨块对齐 | P1 | 固定中英日含音乐/静音语料；漏字、重字、误字幕、首译/Final 延迟对照 |
| 4 | 本地有界上下文、Final/预览分流 | P1 | 慢预览不饿死 Final；改写拒旧结果；术语/代词、否定/数字无跨段增译 |
| 5 | 单应用/播放流选择 | P1 | 两应用并行仅选中流进入 ASR；流消失不切全系统；音源切换重置上下文 |
| 6 | 首次配置与当前链路主入口 | P1 | 新配置从零走到本地字幕；云路径分别授权；缺模型/凭据能在原处恢复 |
| 7 | GUI 集成测试与分阶段质量指标 | P1 | 实际 View 的导航、停止/保留、清空、凭据、错误、焦点；记录呈现与状态时间差 |
| 8 | 统一能力描述和服务 profile | P2 | 源/目标/模型/设备交集一致；重启恢复选择但不恢复上传授权 |
| 9 | 日语 streaming、翻译专用模型 | P2 | 只有来源/许可/hash/运行时俱全才可安装；同机真实推理与质量对照 |
| 10 | 多屏、托盘、全局快捷键 | P2 | 输出移除恢复、穿透后可恢复、窗口关闭不误退出、不抢焦点 |
| 11 | 可选会话保存/字幕导出、下载续传 | P2 | 默认不写文本；显式导出；取消/中断/损坏恢复不激活坏模型 |
| 12 | Linux 发布包与 helper 版本握手 | P2 | 干净机器安装启动；CPU/CUDA 能力明确；错配 helper 提示可恢复 |

质量评估建议使用相同音频、语言对、硬件与模型版本，分别记录：ASR WER/CER、漏段/重段、译文人工错误分类、首次可读译文/最终译文 p50/p95、翻译失败率、显示覆盖率、队列丢弃、CPU/GPU/RAM/VRAM、与视频/游戏并行帧时间。最终呈现时间需单独量测，不能拿文件推理 RTF 代替字幕延迟。

单次自动运行维持最多 90 秒；用多段短样本扩覆盖，不实际等待 30 分钟。长时稳定性在获得证据前继续标为未验证。无需先做 TTS、声音克隆、虚拟设备或 Android；这些会显著扩大验证面。

## 10. 本次验证与交付边界

- 两个外部仓库已实际克隆，提交固定如第 1 节；没有只看 README 下结论。
- `mise run doctor` 通过：固定 Rust 1.99.1、rustfmt、Clippy 与本机原生依赖可用。
- `mise run check` 的格式、构建、严格 Clippy 通过；首次测试有三个模型下载器测试因沙箱不允许绑定回环端口而失败。允许本机监听后重跑完整 `mise run check`，退出码 0，全量通过；没有跳过测试或修改断言。
- 管道机制实验分别检查默认 64 KiB 与注入 4 KiB，明确区分正常情况未触发和故障条件成立。
- 未改应用逻辑，未安装外部仓库依赖，未复制其源码或改变许可证。Mimi 为 MIT、Sokuji 为 AGPL-3.0；本报告仅比较实现，Subveil 的许可证仍由所有者决定。
- 未重测真实 GPU/模型、云端服务、三应用 UI 或性能。已有 Subveil 短测数据引用 [GPU 验收](gpu-asr-validation.md)、[增量翻译验收](cloud-streaming-validation.md) 与 [字幕可靠性](subtitle-reliability.md)，未包装成本次新结果。

## 11. 后续复查入口

Subveil：`crates/subveil/src/session.rs`、`ui/{live,services,model_library}.rs`、`overlay.rs`、`storage.rs`、`credentials.rs`；`subveil-audio/src/linux.rs`；`subveil-asr/src/{vad,local,cloud}.rs`、`local/{offline,whisper}.rs`、`protocols/`；`subveil-translate/src/{lib,local,http,scheduler}.rs`；`subveil-core/src/{subtitle,inference}.rs`；`subveil-inference/src/generate.rs`；`subveil-whisper/src/main.rs`；`subveil-models/src/{catalog.toml,lib.rs,tests.rs}`；CI 和 mise 任务。

Mimi：`shared/mimi-core/src/{translation_policy,subtitle_reducer,live_pair_aligner}.rs`；`shared/mimi-runtime/src/core/{provider,recovery,preview_pacing,session_archive}.rs`、`clients/high_quality_client.rs`；`src-tauri/src/audio/`、`settings_store/file_credentials.rs`、`session_{history,export}.rs`、`desktop_shortcuts.rs`；`src/windows/settings/{QuickStartGuide,ServiceProfiles,SessionExport,SupportDiagnostics,SoftwareUpdate}.tsx`；`src/windows/overlay/` 和平台验证脚本。

Sokuji：`src/providers/{registry,localInference,soniox}/`；`src/lib/local-inference/{modelManifest,ModelManager,modelFileValidation}.ts`、`engine/`、`selection/`、`workers/`、`native/`；`src/lib/{session,segmentation,audio}/`；`src/components/{SetupWizard,Subtitle,Settings}/`；`src/stores/providerStore.ts`、`src/services/SettingsService.ts`；`electron/{pipewire-app-audio,subtitle-window,caption-share,transcript-save,update-manager,native-host-manager}.js`；`sidecar/sokuji_sidecar/`；构建工作流。
