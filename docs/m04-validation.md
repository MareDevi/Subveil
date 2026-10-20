# M04 开发与验收记录

日期：2026-10-20，Asia/Shanghai。

状态：本地 streaming ASR 与独立文字翻译工程实现已落地，英文公开 WAV 真实推理通过。日语 streaming 模型、真实云端翻译账号、系统音频至双语字幕的完整验收尚未完成；不宣称首个 MVP 全部验收通过。范围止于 M04。

## 已实现

- 官方 sherpa-onnx 1.13.8 CPU streaming transducer，统一 Session / Segment / revision、Partial / Final。模型通过 TOML 清单导入；后台校验 SHA-256、路径、元数据与 ONNX 加载，不静默回退云端。输入采样率来自模型清单。
- 本地识别采用 sherpa endpoint rules，默认有文字时尾静音 600 ms、无文字时至少 2.4 秒静音、15 秒最大分段。加载积压、音频过期或不连续直接丢弃/重置，不累积无限延迟。取消不等待 GUI；不能强制中断单次原生加载或推理调用。
- 新增 subveil-translate：Translator trait、OpenAI-compatible Chat Completions、DeepL、可配置 Prompt、三句有限源文上下文、Final 驱动、有界队列、总期限、HTTP 超时、429/5xx 有限重试、取消及错误隔离。
- 文字翻译单独授权，默认关闭；GUI 展示服务、目标主机、授权状态、错误与队列丢弃数。停止会话/切换翻译服务撤销授权。既有语音服务自带译文时不再独立请求。
- 翻译配置失败时仍启动 ASR，运行时翻译失败也不停止 ASR。响应继续经过 SubtitleState 的 Pending、Session / Segment / revision、过期保护；旧结果不会覆盖新字幕。
- 本地模型导入使用原生文件选择器，清单元数据后台读取。运行时禁用设置切换；保留既有恢复窗口与浮窗生命周期。
- 新增模型清单示例、配置文档与 WAV 诊断任务。应用不提供模型下载或 M05 模型管理。

## 自动检查

最终执行 `mise run check`：格式检查、锁定依赖构建、全目标严格 Clippy、Workspace 测试全部通过。

测试 71 项：应用 3、CLI 2、ASR 36、音频 11、字幕/配置 12、翻译 7；没有忽略测试。单次测试命令限制 90 秒。测试不依赖真实模型、设备、密钥或付费接口。

覆盖模型缺失/校验错误/取消、无授权不启动云端、英文/日文修订、旧会话和过期译文保护、翻译队列过载、取消释放在途 future、请求总期限、错误后继续处理后续句子，以及两种协议的真实本机 HTTP 请求/响应与 429 重试。既有协议、PCM 与字幕回归测试继续通过。

首次在沙箱内运行 HTTP 回环测试被 socket 权限拒绝（Operation not permitted）；在获准的沙箱外环境重跑全部检查通过，没有跳过测试或调用外部服务。

`mise run doctor` 与 `git diff --check` 通过。新增依赖已写入 Cargo.lock；统一 reqwest 与既有 WebSocket 使用 ring，避免 TLS 提供者冲突。

## 真实运行

1. 官方英文模型 `csukuangfj/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17`，模型卡标注 Apache-2.0。临时下载至 `/tmp/subveil-m04-model`，ONNX 逐文件通过发布方 LFS SHA-256，其他文件通过 Git blob ID 校验。模型和音频未加入仓库。
2. `mise run asr:local-wav -- /tmp/subveil-m04-model/model.toml /tmp/subveil-m04-model/test_wavs/0.wav`：最终端点配置下收到 16 个 Partial、1 个 Final，0 次重置，0 音频丢弃，0 事件丢弃。以 20 ms 帧模拟实时输入；只输出计数，不上传或打印识别文本。这证明真实模型加载和事件链路，不证明准确率或所有语音场景质量。
3. `mise run asr:probe -- mock 3 ja 16000`：真实 PipeWire 捕获，7 个样本事件，0 丢块、0 重置，取消退出成功。Mock 不是日语语音识别验收。
4. Wayland 下实际启动控制窗口与离线 demo，10 秒后由测试时限结束（退出码 124）。进程未提前退出；仍出现既存第三方 Vulkan 隐式层错误。未核实最终界面完整渲染、键盘/鼠标交互、文件选择器、主题/窗口尺寸和恢复流程，因此不将启动日志视为 GUI 完整验收。

## 已知限制与下一步

- 尚未选定并验证日语 streaming transducer 模型。官方 ReazonSpeech 日语 Zipformer 为 offline，不能导入当前后端。清单接受 ja 不等于具备已验证的本地日语识别。
- 未配置真实翻译账号，未上传音频/文本，未验证 OpenAI-compatible 或 DeepL 的日/英至中文质量、账号权限、费用和线上延迟。
- 真实系统音频到本地 ASR 再到云翻译及浮窗的完整流程、完整 GUI 交互、长期稳定性和资源占用未验收。未等待 30 分钟。
- 模型选择、ASR/翻译设置仍未持久化，密钥仍来自环境变量；系统凭据存储未接入。原生运行库构建来源与限制见配置文档。
- 单线程顺序翻译可能因慢服务丢弃排队任务；错误只影响译文。音频源语种取决于实际模型，不能通过修改清单增加模型语言。

下一步仍在 M04 内，用实际日语 streaming 模型与用户翻译账号完成日语/英语中文字幕验收，并完成模型导入、授权、启停、恢复与错误场景的 GUI 交互验收。M05 尚未开始。

来源：
- https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17/blob/main/README.md
- https://k2-fsa.github.io/sherpa/onnx/pretrained_models/offline-transducer/zipformer-transducer-models.html
