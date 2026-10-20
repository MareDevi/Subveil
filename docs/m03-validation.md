# M03 开发与验收记录

日期：2026-10-20，Asia/Shanghai。

状态：开发实现已落地；真实云端和本地模型语音验收尚未完成。未进入 M04。

## 已实现

- subveil-asr 提供统一 Config / Provider / Recognition 和已有 TranscriptEvent；Mock、Deepgram、可选 Sherpa 使用同一会话与修订模型。
- 音频驱动 Mock 输出明确标记的 en/ja 样本，区别于原有离线双语演示。
- Deepgram nova-3 Listen v1，单声道 linear16、en/ja、真实 Partial/is_final、空 Final 与重复响应处理、TLS、响应大小上限、KeepAlive、超时、断开重试与取消。
- 云端只读 DEEPGRAM_API_KEY，界面明确目标和费用风险。选择 Provider 不上传；必须点击上传按钮才启动。缺凭据拒绝启动，无静默回退；停止不再发音频。已发送数据无法撤回。
- 可选 sherpa-onnx 1.13.8 官方 Rust OnlineRecognizer，用于本地 streaming transducer；CPU 推理在后台，模型路径与速率由用户配置。没有自动模型下载。
- Mock 基础能量 VAD；本地原生 endpoint；云端服务端 endpointing。具体参数和差异见 README，不将能量阈值宣传为 Silero。
- M02 PCM 后台消费后经独立 16 块队列交给识别；结果队列 64 事件。丢帧、过期、速率或设备代次变化重置上下文，溢出有计数。无音频录制。
- GPUI 提供 Provider、en/ja、采样率、显式启停、状态与丢弃统计；结果进入已有字幕和历史。取消后拒绝旧结果，关闭控制窗口保留恢复入口。
- 收到重复 Partial 且音频结束时间前进至少 500 ms 时刷新修订，减少文字暂时不变时字幕 TTL 提前过期；过载或中断产生的旧 Partial 保留至正常过期，不伪造 Final。
- 新增 asr:probe、asr:check；单次诊断最多 60 秒，测试命令限制 90 秒。

## 自动检查

`mise run check` 已通过：格式、锁定依赖构建、严格 Clippy、Workspace 测试。

`mise run asr:check` 已通过：所有特性 Workspace 构建、严格 Clippy、Workspace 测试；覆盖本地运行库实际链接，测试无需真实模型或付费凭据。

40 项测试覆盖原有 25 项、新增 PCM handoff 1 项及 ASR 14 项：日语修订、Final 稳定、空 Final、旧范围重复响应、真实内存 WebSocket PCM 编码与 Partial/Final、关闭后新连接段编号、虚拟时钟下音频中断和服务响应超时、能量 VAD、短脉冲过滤、最大段长、非法配置、输入代次/序号/过期、输出过载、Mock 音频链路和取消、长段 TTL 刷新、错误信息不泄露服务端正文。

内存 WebSocket 使用 duplex 流，不访问网络；超时测试推进虚拟时钟，不实际等待 30 秒。重连用例覆盖会话重建和段编号，不代表真实服务所有网络故障。`git diff --check` 通过。

## 运行验证

- 本机真实 PipeWire 至 Mock ASR 诊断：`mise run asr:probe -- mock 3 ja 16000`。输出为 16 kHz、ja、cloud upload=false；约第 1/2 秒捕获块数为 89/179，0 丢块、0 ASR 重置，3 秒内收到 7 个样本识别事件，取消并退出成功。不打印识别正文，不保存音频；样本事件不等同真实语音识别。
- 实际启动 M03 GPUI 控制窗口，截取应用自身窗口检查 Provider、语言、上传提示、状态、预览和统计区域，均正常显示。没有把启动日志当作按钮、恢复入口或真实 ASR 的完整交互验收。
- Wayland socket 受沙箱限制，首次启动失败；经授权运行环境启动成功。已有第三方 Vulkan 隐式层错误仍出现，窗口正常渲染。
- 没有配置云端凭据，没有调用付费服务或上传音频；没有下载模型，没有执行本地模型推理。

## 未完成验收与限制

- 待提供 DEEPGRAM_API_KEY，验证系统英语/日语音频的实际 Partial、Final、准确率、延迟、断网与恢复；当前协议测试不能替代真实服务验收。
- 待提供经过来源、许可证和完整性核对的 streaming 模型，验证本地英语/日语识别。模型语言能力不能由 GUI 的语言标签改变；当前不包含模型管理器。
- 本地模型加载与单次原生 decode 无法强制取消，取消在调用之间生效。文件存在检查不证明模型有效或支持所选语言。
- GUI 仍使用默认 VAD 参数，ASR 设置不持久化；尚未逐一交互验证 ASR 按钮、采集切换和恢复入口。已有 M01/M02 验收记录保留其各自范围。
- 能量 VAD 不保证噪声环境表现；非 Silero。无硬件 PTS、真实服务延迟基准或长期资源稳定性证明。M02 的 30 分钟稳定性仍未验证。

下一步仍在 M03：配置凭据和本地模型，完成真实语音与恢复验收，再决定进入 M04。
