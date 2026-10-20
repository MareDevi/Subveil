# 云语音服务配置

核对日期：2026-10-20，Asia/Shanghai。以下为已实现的协议适配，不代表账号权限、模型可用性、真实语音质量或服务收费已经验收。所有云端调用均可能收费；本轮未配置真实密钥、未调用付费接口。

## 配置与启动

凭据通过启动进程的环境变量提供，不写入 TOML、日志或仓库。在控制窗口选择服务和目标语种后，核对显示的上传目标，再点击「同意上传并开始」。仅选择服务不会联网；连接与初始化可能产生服务用量。停止会取消会话与未处理结果，已发送音频无法撤回。

| 服务 / CLI ID | 接口或模型 | PCM 输入 | 必需环境变量 |
| --- | --- | --- | --- |
| 阿里云 / `alibaba` | LiveTranslate `qwen3.8-livetranslate-flash-realtime`；北京业务空间 | 16 kHz | `DASHSCOPE_API_KEY`、`DASHSCOPE_WORKSPACE_ID` |
| Google Gemini / `gemini` | Live `gemini-3.5-live-translate-preview` | 16 kHz | `GEMINI_API_KEY` |
| 火山引擎 / `volcano` | 豆包同传 2.0，`volc.service_type.10053`，S2T | 16 kHz | `VOLCENGINE_API_KEY`（新版语音控制台 Key） |
| 腾讯云 / `tencent` | ASR 实时语音翻译，`hunyuan-translation-lite` | 16 kHz | `TENCENT_APP_ID`、`TENCENT_SECRET_ID`、`TENCENT_SECRET_KEY` |
| 百度 / `baidu` | `/ws/realtime_speech_trans` | 16 kHz | `BAIDU_APP_ID`、`BAIDU_API_KEY`（机器翻译应用 API Key） |
| OpenAI / `openai` | `gpt-realtime-translate` + `gpt-realtime-whisper` | 24 kHz | `OPENAI_API_KEY` |
| Azure OpenAI / `azure-openai` | 同一资源内的实时翻译与实时转写部署 | 24 kHz | `AZURE_OPENAI_ENDPOINT`、`AZURE_OPENAI_API_KEY`、`AZURE_OPENAI_DEPLOYMENT`、`AZURE_OPENAI_TRANSCRIPTION_DEPLOYMENT` |
| xAI / `xai` | `grok-voice-latest` + `grok-transcribe`，回合式译文 | 24 kHz | `XAI_API_KEY` |
| Deepgram / `deepgram` | Listen v1 `nova-3`，仅原文 | 可选 16 / 24 / 48 kHz | `DEEPGRAM_API_KEY` |
| 自定义 DashScope / `custom-dashscope` | `run-task` / `task-started` / `result-generated` 识别协议 | 16 kHz | `CUSTOM_DASHSCOPE_URL`、`CUSTOM_DASHSCOPE_MODEL`、`CUSTOM_DASHSCOPE_API_KEY` |
| 自定义 OpenAI Realtime / `custom-openai` | GA transcription 会话，`intent=transcription` | 24 kHz | `CUSTOM_OPENAI_URL`、`CUSTOM_OPENAI_MODEL`、`CUSTOM_OPENAI_API_KEY` |

所有输入均为单声道 PCM16LE；系统采集的 F32 由后台管线转换。GUI 在切换服务时选择所需采样率，启动时再次按接口要求指定；不把全部模型固定为 16 kHz。CLI 显式传入错误速率会拒绝启动。Mock 不识别实际语音，不上传，保留 en / ja 音频驱动样本。

Azure 端点填写 HTTPS 资源根地址，例如 `https://RESOURCE.openai.azure.com`；接受 `openai.azure.com`、`openai.azure.cn`、`openai.azure.us` 资源域名。部署名称填实际部署名，不是通用聊天模型名。域名通过校验不证明地域开放了模型。

自定义端点填写完整 WebSocket URL；远程必须使用 `wss://`，只有 `localhost`、`127.0.0.1`、`[::1]` 可使用 `ws://`。不接受 URL 用户名、密码、查询参数或片段。自定义 OpenAI 会自行添加 `intent=transcription`，不能用 HTTP 文件转写或 Chat Completions 地址替代。当前兼容 GA session.update 转写协议，不承诺所有旧版或第三方 Realtime 方言可用。`gpt-live-transcribe` / `gpt-realtime-whisper` 的 turn_detection 为 null，其他模型使用 server_vad。

腾讯采用排序参数、HMAC-SHA1 和 Base64 签名；每次重连重新生成 voice_id 和 nonce。Google 和腾讯按官方鉴权机制生成内部查询参数；连接错误不会打印完整 URL、请求头或服务端正文。

## 原文、译文与语言

- GUI 提供 en / ja 源语种提示及 zh / en / ja 目标。阿里 LiveTranslate、Google、OpenAI、Azure 使用服务自动识别源语种；界面明确标记自动识别，不伪造检测出的语言标签。源语种选择不会改变模型支持范围。
- 阿里使用 item_id / previous_item_id 关联；腾讯使用 sentence_id；火山使用原文与译文起始时间；百度按服务串行 MID / FIN 句段处理。xAI 将 response_id 绑定到已提交的语音回合，不把迟到译文分配给后来开始的讲话。
- OpenAI / Azure / Google 的连续原文和译文以滚动窗口展示，分别限制为 8192 字节，UTF-8 截断安全。不把 elapsed_ms 当唯一段 ID，不声称连续原文和译文具有逐句精确对齐；没有句级结束标记时不伪造 Final。
- 其他协议支持服务明确返回的 Partial / Final。译文包含 session、segment、revision；源文修订后拒绝旧译文。早到译文先暂存，未知关联不猜配。接口自带译文会显示；Deepgram 和两种自定义识别当前只输出原文。
- 本轮不另发文字翻译请求。返回的音频均不播放、不保存；部分模型仍会在服务端生成语音，这可能影响用量，不承诺只收取文字费用。
- 火山 S2T 的语言方向必须包含中文或英语；其他语言、地域、部署和模型权限仍以账号及官方协议为准。界面可选项不代表全部组合已实测。

## 传输与上限

每个云语音会话使用独立后台线程。PCM 和字幕结果分别有界；原文事件与自带译文各 64 条，溢出丢最旧并计数。服务状态最多保留 64 段文本、512 个淘汰标识和 256 个近期事件标识，达到标识上限后重建连接。单段文字最多 16 KiB。

服务配置确认之前不发送音频；确认后清除初始化积压。阿里、Google、xAI、自定义服务按 100 ms 包发送；火山 80 ms；腾讯、百度 40 ms；OpenAI、Azure 200 ms。积压超过 500 ms、PCM 过期 250 ms、序号/代次不连续时重建上下文，不无限追赶旧音频。音频不足一包时留在有界缓冲，不补造声音。

WebSocket 握手与配置确认各最多 10 s，写入 3 s，无音频 2 s、无服务消息 30 s 超时。错误后按 2 / 4 / 8 s 重试；当前未实现服务会话恢复，重连丢弃旧音频和上下文，段 ID 保持递增。Google goAway 同样触发重建，不承诺无缝轮换。取消检查约每 20 ms 一次，不等待网络超时结束；本机不保存音频。

## 无窗口诊断

```sh
mise run asr:probe -- mock 3 ja 16000
# 先在启动环境配置所选服务的变量；只有此显式参数允许上传。
mise run asr:probe -- alibaba 10 en 16000 --allow-upload
mise run asr:probe -- openai 10 ja 24000 --allow-upload
```

诊断默认目标为 zh，默认仅打印统计；仅显式添加 `--print-transcripts` 时输出原文与译文。单次 1–60 秒，外层时限 90 秒。凭据不要写进命令参数或复制到 Issue。

## 官方协议来源

- [阿里 LiveTranslate](https://help.aliyun.com/zh/model-studio/qwen3-5-livetranslate-flash-realtime)、[客户端事件](https://help.aliyun.com/zh/model-studio/live-translator-client-events)、[服务端事件](https://help.aliyun.com/zh/model-studio/live-translator-server-events)。
- [Google Live 翻译](https://ai.google.dev/gemini-api/docs/live-api/live-translate)。
- [火山同传 2.0](https://docs.volcengine.com/docs/DoubaoVoice/SimultaneousInterpretation20APIAccessDocumentation?lang=zh)。
- [腾讯实时语音翻译](https://cloud.tencent.com/document/product/1093/127565)。
- [百度实时语音翻译](https://cloud.baidu.com/doc/MT/s/Sl9p2h5k9)。
- [OpenAI Realtime 翻译](https://developers.openai.com/api/docs/guides/realtime-translation)、[服务端事件](https://developers.openai.com/api/reference/resources/realtime/translation-server-events)、[转写配置](https://developers.openai.com/api/docs/guides/realtime-transcription)。
- [Azure OpenAI WebSocket](https://learn.microsoft.com/en-us/azure/foundry/openai/how-to/realtime-audio-websockets)。
- [xAI Voice](https://docs.x.ai/developers/rest-api-reference/inference/voice)。
- [DashScope 识别客户端事件](https://help.aliyun.com/en/model-studio/fun-asr-client-events)。

本地 ASR、模型导入和运行库集成属于 M04；当前不提供本地后端、模型下载或假本地实现。
