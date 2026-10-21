# GPU ASR 选型与性能验收

日期：2026-10-21，Asia/Shanghai。本文保留选型调研与目标口径；已完成实现和实测结果见 [GPU ASR 使用与验收](gpu-asr-validation.md)，以下“当前基线”指调研开始时的 M05 版本。

## 当前基线与选型

现有实现为 CPU SenseVoice / Zipformer 加 Qwen2.5 1.5B Q4_K_M CPU / Vulkan 翻译。RTX 4060 Laptop 上翻译进程曾测得约 938 MiB 显存；这不包含桌面、ASR 与未来 CUDA 上下文，也不是完整链路峰值。既有约 10–15 秒音频链路诊断包括实时播放、尾静音及排空，不能当作字幕延迟。详见 [M05 验收](m05-validation.md)。

优先评估 Whisper large-v3-turbo 加现有 Qwen 翻译。Rust 原生集成候选为 whisper-rs / whisper.cpp CUDA，正式开发前仍需核对绑定与底层版本、编译工具链、取消边界和 C++ 库共存。当前进程隔离经验应纳入设计，不能假设新增原生库可直接链接 GPUI 进程。faster-whisper 的 CTranslate2 跑分仅作参考，不作为 Rust 后端的性能依据，也不因此引入 Python 运行时。

Whisper 使用原语种转写，再由文本模型生成中文；不能将 Whisper 的语音翻译任务理解为任意目标语种翻译。上游 whisper.cpp 支持 CUDA，并提供 large-v3-turbo；stream 示例每隔一段时间对音频窗口重新解码，不是保持原生流式解码状态的 streaming ASR。VAD 控制分段，不能消除等待句末造成的延迟。

候选顺序：

1. Whisper large-v3-turbo CUDA，与现有 CPU 模型在固定日 / 英音频上比较质量、热态延迟和资源。若显存或游戏影响不达标，再比较 small 与量化变体；不能仅凭权重大小决定默认模型。
2. 保留 CPU SenseVoice（日语）与 CPU streaming Zipformer（支持语种内）的资源受限路径。Intel Arc 只完成设备枚举，须独立推理验证后才能推荐承载 ASR 或翻译。
3. SeamlessStreaming 为流式语音翻译研究候选，SeamlessM4T v2 为质量比较候选。本次未核实其完整部署条件，也未完成本机测试，不按用户材料中的估算显存或速度排名。
4. Canary 必须按具体模型版本、语言对、部署产品和权重许可证分别核对；不能把开源 Canary 与 Riva 产品能力合并成一个可选项。

## 指标与目标

单路音频、非批量处理；日→中与英→中分别报告。以下均为待验证目标。

| 指标 | 优化目标与测量口径 |
| --- | --- |
| 常驻显存 | ASR 与翻译全部推理进程合计尽量不超过 4 GiB；报告热态稳定值和加载 / 推理峰值，同时记录桌面总显存变化。GiB 与 MB 保留原单位，不混用。 |
| 首条中文字幕 | 热态从首个有声采样到首次中文显示的目标为 1–3 秒；标明预览或已确认译文。另报从句末到已确认译文的延迟，长句不得从样本中删除。 |
| 持续处理速度 | 处理时间 / 音频时长（RTF）≤ 0.5，即至少 2 倍实时；固定 10 秒音频目标为处理时间 ≤ 5 秒。采用非实时回放测处理能力；实时回放另验队列是否积压。 |
| CPU 与并行负载 | 记录整机及应用 CPU、内存、GPU 利用率；同一游戏场景比较无字幕 / 有字幕的帧时间 p50 / p95、帧率与显存。视频记录掉帧；未定量前不声称“不影响游戏”。 |
| 冷启动与运行方式 | 独立报告完整性校验、模型加载、CUDA / Vulkan 预热和冷态首条字幕。本地离线；NVIDIA CUDA 为待实现 ASR 优先评估后端，现有 Vulkan 翻译独立保留。 |

处理速度需分别报告 ASR、文字翻译及二者合计；合计包含实际调度、IPC 和等待，不能把两个单独最佳值相加当作链路实测。字幕延迟包含缓冲、分段、排队、推理与 UI 更新，不能用 token/s 或整段文件吞吐替代。

当前独立翻译仅处理 Final。SenseVoice 等待 VAD 分段结束；长句下首条中文可能超过 3 秒。Whisper 更快也不能单独解决这一点。若需要句中中文，必须另行设计稳定前缀 / 分段提交、有限频率翻译和 Session / Segment / revision 修订保护，验收重复、抖动、乱序、取消及过载，不能把反复重译 Partial 作为无成本方案。

## 短时测试方案

- 所有命令通过 mise，构建使用锁定依赖。每次测试最多 90 秒；通过多个短测试收集样本，不以短测宣称 30 分钟稳定性。
- 固定公开日 / 英音频及 SHA-256，覆盖短句、连续长句、停顿和静音；统一采样率转换、量化、线程数、beam、窗口与步长，记录模型 revision、驱动、功耗模式和实际 GPU 后端。不得上传用户音频。
- 冷启动与热态分开。标注有效样本数，报告各阶段及端到端 p50 / p95、错误和丢弃数；样本不足时不声称稳定的尾延迟。实时测试持续检查队列深度、丢弃及延迟增长，防止文件处理很快但在线持续落后。
- 音频接收时间是软件单调时钟，不是硬件 PTS；报告该测量边界。UI 状态接受时间与实际呈现时间也分开，若仅测前者需明确标注。
- 设备检测、模型加载、GPU 进程出现均不足以单独证明 ASR GPU 推理成功。需完成实际识别并记录执行后端与资源；不静默回退 CPU 或云端。
- 游戏与视频测试仅使用用户显式选定的公开场景；本轮未进行并行负载测试。

## 资料核查

本次先用 Context7 resolve / query 检索 whisper.cpp 与 faster-whisper，再核对上游页面。来源访问日期均为 2026-10-21。

- [whisper.cpp 上游](https://github.com/ggml-org/whisper.cpp)：CUDA 后端、模型列表；[stream 示例](https://github.com/ggml-org/whisper.cpp/tree/master/examples/stream)说明滑窗推理方式。
- [Whisper large-v3-turbo 模型卡](https://huggingface.co/openai/whisper-large-v3-turbo)：转写模型及能力边界。模型支持不等于绑定版本已兼容或本机已测试。
- [faster-whisper 官方基准](https://github.com/SYSTRAN/faster-whisper#benchmark)：13 分钟音频、RTX 3070 Ti 8GB、large-v2、beam 5；非批量 FP16 为 63 秒 / 4525 MB，INT8 为 59 秒 / 2926 MB。批量结果须单独列出。用户材料中的 large-v3 52 秒、turbo 19 秒与 2.48 GiB 缺少可复核的同条件原始来源，本轮不写入验收数据。

本次只核对优先候选及性能口径，未全面验证参考材料中 Seamless / Canary 的参数量、显存和部署陈述。

## 实施决定（2026-10-21）

用户要求完成实现与验证。新增独立 `subveil-whisper` Rust 推理程序，固定 whisper-rs 0.16.0 / whisper-rs-sys 0.15.0；不与 sherpa 或 llama 原生库混合链接。默认构建保留 CPU，CUDA 通过显式 mise 任务构建。CPU / CUDA 分别选择，CUDA 不可用时报错，不自动降级。模型库增加固定版本 large-v3-turbo Q5_0（574041195 字节），复用显式安装和校验。

字幕采用最长 2 秒的有界能量分段（只提交 Final），连续讲话也会切块；这允许句中字幕，但可能切断词句，质量须单独比较。保留原模型分段语义。运行设备和短块长度属于 Session 设置，由原生 Button 切换，禁用会话中更改；状态和错误在现有模型区域显示，键盘使用标准组件 Tab / Enter 路径。
