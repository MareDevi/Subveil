# 本地 ASR 与文字翻译（M04 / M05）

M05 增加 Qwen GGUF 本地文字翻译、Vulkan GPU 选择和完全离线模式。当前使用步骤及验收见 [M05 使用与验收](m05-validation.md)。语音识别保持 CPU；GPU 加速文字翻译，不代表 sherpa CUDA 已支持。

## 应用内模型选择与下载

运行 `mise run models`，或在控制窗口「语音服务」选择「本地 sherpa-onnx」。从「下载模型」选择候选，核对来源、许可证、下载量和运行模式，点击「安装并选用」。安装完成后再点击「开始本地识别」。安装本身不会开始音频采集；本地识别不会上传音频。

| 模型 | 语言 | 下载量 | 运行方式 | 权重许可证 |
| --- | --- | --- | --- | --- |
| SenseVoice Small INT8 | 日 / 英 / 中 | 239.5 MB | CPU，VAD 分段后识别，仅 Final | FunASR Model License 1.1，自定义条款 |
| Zipformer English 20M INT8 | 英 | 43.6 MB | CPU，真正 streaming，Partial / Final | Apache-2.0 |
| Zipformer Chinese / English INT8 | 英 / 中 | 198.3 MB | CPU，真正 streaming，Partial / Final | Apache-2.0 |

日语优先选择 SenseVoice；英文低延迟先试 Zipformer 20M；中英文混合场景再比较中英 Zipformer。语言能力取决于权重，不能通过修改清单增加语言。SenseVoice 上游模型卡引用自定义 [FunASR Model License](https://github.com/modelscope/FunASR/blob/main/MODEL_LICENSE)，不能套用运行库 Apache-2.0 或代码许可证。

模型保存在 `$XDG_DATA_HOME/subveil/models`；未设置或不是绝对路径时，使用 `$HOME/.local/share/subveil/models`。每个模型按 ID 与固定 revision 分目录。应用先流式下载到临时文件，核对精确长度和 SHA-256，再原子发布文件；全部文件通过后才生成运行清单。目录内含发布者 README 和来源、许可信息。

界面显示下载进度，支持取消及重试。已验证的完整文件保留并复用；未完成的单个文件不提供 HTTP Range 续传，需要重新下载。安装锁阻止多个进程同时写入同一包；崩溃后操作系统释放锁，重试会清理未发布的 `.part`。取消、断网、校验错误或空间不足均不激活新模型，不回退云端。HF 及官方 CDN 使用 HTTPS，跳转次数最多 5 次，连接超时 15 秒、无数据读取超时 30 秒、单文件总期限 600 秒。

模型文件跨重启保留。「保存本地设置」持久化本地模型、源/目标语种、离线限制及翻译设备；不保存上传授权或自动开始采集。重启后文件仍会校验。磁盘剩余空间在安装线程读取，每个文件下载前预检。运行识别时禁用模型更换，停止后释放模型；ASR 单次原生调用仍不能强制中断，本地文字翻译通过终止独立进程取消。关闭控制窗口时，进行中的安装与会话保留在恢复入口。

## 本地识别方式

使用官方 sherpa-onnx 1.13.8 CPU 运行库。Zipformer 使用 streaming transducer；默认有文字时尾静音 600 ms 分段，无文字时至少 2.4 秒静音重置，最长 15 秒分段。能量阈值和前置缓冲不参与 streaming 模型端点。

SenseVoice 使用 offline API，按能量 VAD 结束的语音段识别，默认保留 200 ms 前置缓冲、至少 150 ms 有声、600 ms 尾静音、最长约 15 秒分段；输入按 30 秒上限截取，PCM 缓冲始终有界。只返回 Final，不伪造增量 Partial。CPU 解码时间单独显示，不包含加载、等待分段与字幕显示，不能作为端到端延迟。

音频不连续、队列过载或 2 秒无 PCM 时重置分段/流；加载积压直接丢弃，不无限追赶历史。采样率来自模型包，PipeWire 按所需速率输出单声道 PCM。当前固定 CPU：SenseVoice 4 个推理线程，streaming 2 个线程。检测到 NVIDIA GPU 不等于此 CPU 运行库已支持 CUDA。

## 导入已有模型与诊断

仍支持「导入已有模型…」。Streaming 清单示例为 `config/local-model.example.toml`；旧清单缺少 `kind` 与 `languages` 时按原 streaming 单语义读取。新增 `kind = "sense_voice"` 时使用 `[model]`、`[tokens]`，并要求 16 kHz；不要提供 encoder / decoder / joiner。`languages` 可列 en / ja / zh，`language` 为默认源语种。界面按模型声明切换源语种。所有文件在后台校验后才加载。

官方日语 ReazonSpeech offline Zipformer、Whisper 和 Paraformer 暂不支持此导入后端。模型清单中的来源和许可是提供方声明，不是应用的法律认证。未完成句子在停止时可能被丢弃。

无窗口诊断：

```sh
# 列出目录；不联网
mise run model:install -- --list
# 显式安装，使用与 GUI 完全相同的下载器
mise run model:install -- sensevoice-small-int8
# 重放自己的 WAV；模型路径由安装器打印，可选源语种
mise run asr:local-wav -- /absolute/path/model.toml /absolute/path/test.wav ja
# 捕获系统音频；不调用文字翻译
SUBVEIL_LOCAL_MODEL=/absolute/path/model.toml mise run asr:probe -- local 10 ja 16000
```

WAV 诊断不采集设备或输出文本，要求速率匹配、时长不超过 30 秒。20 ms 帧模拟实时输入，补两秒静音；streaming 验证 Partial 与 Final，SenseVoice 验证仅 Final。报告队列丢弃、重置与离线解码统计。系统音频诊断只有显式 `--print-transcripts` 才打印识别文本。命令行诊断限制 90 秒，应用安装仍按上述网络期限执行。

首次构建 `sherpa-onnx-sys` 会从官方 GitHub release 下载 CPU 原生运行库，不包含语音模型。可使用 `SHERPA_ONNX_LIB_DIR` 指定自行构建的同版本静态库目录，或用 `SHERPA_ONNX_ARCHIVE_DIR` 提供官方归档。Cargo.lock 固定 Rust 包，但上游构建脚本没有验证原生归档的发布者校验值；生产供应链应自行验证/构建并指定库目录。应用保持 forbid unsafe；FFI 由官方依赖封装。

## 独立文字翻译

默认关闭。在停止识别时点击「翻译服务」轮换关闭 / 本地离线翻译 / OpenAI-compatible / DeepL。本地模型无需上传授权。云端翻译须核对「文本上传目标」，点击「同意本次会话上传文本」，再开始识别。该授权只允许文字及最多三句源文上下文；本地 ASR 不因此上传音频。停止或切换翻译服务会撤销授权。语音服务自带译文时禁用独立翻译，避免重复请求。

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

云端任务从提交起总期限 6 秒，包含排队、网络、响应读取和重试；HTTP 请求另有 5 秒超时。429 / 5xx 最多重试两次，退避 250 / 500 ms，仍受总期限约束。错误只影响对应译文，ASR 继续处理。结果必须匹配 Session / Segment / revision，且仍为 Pending、未过期，才能更新字幕。取消会丢弃网络 future。

真实日语/英语识别质量、字幕延迟、收费与供应商兼容性需要使用用户模型及账号另行验收。离线协议测试不代表这些项目通过。
