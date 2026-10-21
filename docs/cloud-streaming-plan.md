# 应用内云服务配置与增量翻译

日期：2026-10-21，Asia/Shanghai。

## 检查

工作区干净，main；Rust 1.99.1 与原生依赖检查通过。Wayland 图形环境可见。现有模型目录含两个原生 Streaming Zipformer、SenseVoice、Whisper Turbo、Qwen2.5 1.5B。

## 实施边界

- GUI 使用显式设置快照，不读取凭据环境变量；CLI 保留旧变量兼容。凭据输入遮蔽，仅系统 Secret Service 持久化，不写明文 TOML。系统存储不可用时明确报错，当前进程仍可使用。
- 查询模型需要用户点击；不发送音频或字幕。模型发现按厂商协议解析并过滤到已实现协议；返回目录不证明账号推理权限。无通用目录接口时展示官方协议固定项，Azure 部署名手填，不伪造发现结果。
- 保留 Final-only；增加默认关闭的稳定 Partial 翻译。连续假设、词边界、稳定等待、最短长度和限频共同控制提交。译文为临时状态；Final 重新翻译。Session / Segment / revision 拒绝过时结果；仅 Final 进入上下文。
- 不把 token 输出或离线分块称为原生流式。保持现有 Zipformer 状态复用。核对日语模型来源、许可证、下载完整性；无法取到元数据的候选只能列为研究项。
- 保留离线模式与音频、文字分别授权。默认日志不包含凭据、字幕或响应正文。

## 验收

覆盖设置隔离和脱敏、模型发现协议过滤、Partial 修订与 Final 纠正、旧结果拒绝、限频和 Unicode 边界。执行 mise run check；单次测试最多 90 秒。无真实云凭据不宣称服务推理通过。

## 官方资料

- OpenAI： https://developers.openai.com/api/reference/resources/models/methods/list
- Gemini： https://ai.google.dev/api/models#method:-models.list
- Deepgram： https://developers.deepgram.com/reference/manage/models/list
- Azure 部署枚举为 ARM 管理接口，需要管理平面授权，不能用推理 API Key 代替： https://learn.microsoft.com/en-us/rest/api/aiservices/accountmanagement/deployments/list?view=rest-aiservices-accountmanagement-2024-10-01
- xAI： https://docs.x.ai/developers/rest-api-reference/inference/models
- 腾讯实时语音翻译： https://cloud.tencent.com/document/product/1093/127565
- 百度实时语音翻译： https://cloud.baidu.com/doc/MT/s/Sl9p2h5k9
- 日语实验模型： https://huggingface.co/fujie/sherpa_onnx_asr_csj_cejc_pron_comp_zipformer_stream_c320_la0 ，模型卡为 CC BY 4.0，输出假名及标记；公开卡片可读，但本轮 Hugging Face API 返回 401，不能编造校验值或本机 RTF。

阿里、火山与部分模型原始页面本轮抓取失败；沿用项目已记录的官方协议地址与限制，不以第三方资料作为新增 API 依据。Context7 核对 OpenAI 模型列表、腾讯鉴权及 sherpa 在线状态复用。
