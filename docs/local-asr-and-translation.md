# 本地 ASR 与文字翻译（M04）

## 本地模型导入

使用官方 sherpa-onnx 1.13.8 CPU streaming transducer。只接受对应 streaming encoder / decoder / joiner / tokens；不把离线分块识别标为流式。当前支持清单声明 en 或 ja。仅英文 streaming 模型完成真实推理验证；尚未选定并验证可用的日语 streaming 模型。官方日语 ReazonSpeech offline Zipformer 不能导入本后端。语言能否识别取决于实际模型，修改清单语言不会增加模型能力。

自行取得模型并核对其来源、许可证与发布者提供的完整性信息。应用不下载模型。复制 `config/local-model.example.toml` 到解包模型目录，填写文件名、实际输入采样率、来源、许可证和各文件 SHA-256。相对路径以清单目录为基准，也支持绝对路径。

在「语音服务」选择「本地 sherpa-onnx」，点击「导入模型清单…」。界面显示用户声明的来源、许可证、语种、采样率；这不是对许可证真实性的认证。启动后后台先校验全部 SHA-256，再通过原生 API 加载 ONNX。缺文件、错误校验值或不兼容模型明确报错，不启用云端后备。

推理与校验均不占 GPUI 主线程。音频不连续或 2 秒无 PCM 时重置流；加载时积压音频直接丢弃，避免追赶历史。模型使用 sherpa endpoint rules；有文字时默认尾静音 600 ms 分段，无文字时至少 2.4 秒静音重置，最长 15 秒强制分段。尾静音和最大段长来自 Config.vad，能量阈值、前置缓冲和最小语音时长不用于本地模型端点。采样率来自清单；PipeWire 按模型速率输出单声道 PCM。CPU 推理落后于实时速度时可能频繁重置，应使用更轻模型或更快设备。

停止请求能在文件校验块之间和解码步骤之间取消。单次原生模型创建/推理调用无法强制中断，GUI 不等待线程退出。正常停止清空会话并丢弃迟到结果，不保证输出尚未完成的最后半句。当前不持久化模型选择，不提供下载、GPU 或模型管理。

无窗口系统音频诊断（不会上传音频或调用文字翻译）：

```sh
SUBVEIL_LOCAL_MODEL=/absolute/path/model.toml mise run asr:probe -- local 10 en 16000
```

语种与速率应匹配模型。只有显式 `--print-transcripts` 才打印识别文本。

可重放本地 WAV 验证模型加载与真实 Partial / Final，不采集设备，不输出文本：

```sh
mise run asr:local-wav -- /absolute/path/model.toml /absolute/path/test.wav
```

WAV 必须匹配模型采样率，时长不超过 30 秒。诊断以 20 ms 块模拟实时输入，补两秒静音，验证至少一条 Partial 与 Final，并报告重置与丢弃统计。

首次构建 `sherpa-onnx-sys` 会从官方 GitHub release 下载 CPU 原生运行库，不包含语音模型。可使用 `SHERPA_ONNX_LIB_DIR` 指定自行构建的同版本静态库目录，或用 `SHERPA_ONNX_ARCHIVE_DIR` 提供官方归档。Cargo.lock 固定 Rust 包，但上游构建脚本没有验证原生归档的发布者校验值；生产供应链应自行验证/构建并指定库目录。应用保持 forbid unsafe；FFI 由官方依赖封装。

## 独立文字翻译

默认关闭。在停止识别时点击「翻译服务」轮换关闭 / OpenAI-compatible / DeepL，核对「文本上传目标」，点击「同意本次会话上传文本」，再开始识别。该授权只允许文字及最多三句源文上下文；本地 ASR 不因此上传音频。停止或切换翻译服务会撤销授权。语音服务自带译文时禁用独立翻译，避免重复请求。

OpenAI-compatible 配置：

- `SUBVEIL_TRANSLATE_URL`：完整 Chat Completions 地址，例如 `https://api.openai.com/v1/chat/completions`。
- `SUBVEIL_TRANSLATE_KEY`：该端点的 API Key，不复用 ASR 凭据。
- `SUBVEIL_TRANSLATE_MODEL`：账号实际可用的模型名称，不内置默认付费模型。
- `SUBVEIL_TRANSLATE_PROMPT`：可选 system Prompt，必须含 `{target}`，支持 `{source}`。最多 4096 字节。原文与上下文作为独立 JSON 数据传入 user message；模型仍可能不遵循提示。

DeepL 配置：

- `DEEPL_API_KEY`：DeepL API 密钥。
- `SUBVEIL_DEEPL_URL`：可选完整端点，默认 `https://api-free.deepl.com/v2/translate`；Pro 账号使用 `https://api.deepl.com/v2/translate`。
- 使用 `text`、`source_lang`（已知时）、`target_lang` 和 `context`。DeepL 不应用 OpenAI system Prompt。

远程端点必须 HTTPS；仅本机可 HTTP。拒绝 URL 中的凭据、查询参数和片段；不跟随重定向。密钥仅驻留内存，仍未接入系统凭据存储。错误不显示服务正文、URL、密钥或原文。

仅 SubtitleState 接受的 Final revision 提交翻译，不逐 Partial 调用。等待队列 8 条，结果队列 64 条，满时淘汰最旧并计数；被淘汰的等待任务返回失败。单线程顺序请求，间隔至少 100 ms。上下文最多三句，每句最多 1024 字符；源文本/译文最多 16 KiB，HTTP 响应最多 64 KiB。

每个任务从提交起总期限 6 秒，包含排队、网络、响应读取和重试；HTTP 请求另有 5 秒超时。429 / 5xx 最多重试两次，退避 250 / 500 ms，仍受总期限约束。错误只影响对应译文，ASR 继续处理。结果必须匹配 Session / Segment / revision，且仍为 Pending、未过期，才能更新字幕。取消会丢弃网络 future。

真实日语/英语识别质量、字幕延迟、收费与供应商兼容性需要使用用户模型及账号另行验收。离线协议测试不代表这些项目通过。
