# M03 实施计划

日期：2026-10-20，Asia/Shanghai。

## 开始检查

- main 工作区干净，M02 已提交；origin/main 已配置。
- Rust 1.99.1，`mise run doctor` 通过；保持 GPUI 版本不变。
- M02 捕获仅消费电平，需新增有界 PCM 接口；保留原有采集与离线字幕演示。
- M02 的 30 分钟稳定性仍未验证，不将其视作已通过。

## 范围

1. 新增实际使用的 subveil-asr：统一 Provider、音频输入、Partial/Final 事件和取消。
2. 先实现音频驱动的 Mock，再实现 Deepgram Listen v1 WebSocket（nova-3，en/ja）。凭据只读 DEEPGRAM_API_KEY；云端上传必须在界面显式选择，不自动回退。
3. 基础可配置能量 VAD/分段：最小语音、前置缓冲、尾部静音、最大段长。能量门限不是 Silero，也不能保证噪声识别准确。
4. 本地 sherpa-onnx 使用官方 Rust OnlineRecognizer 安全绑定，作为可选构建特性。只接受用户提供的 streaming transducer 模型文件；不自动下载模型。英语/日语支持由实际模型决定，不能将离线分块称为 streaming。
5. 识别在独立后台线程运行。PCM/结果队列有界；音频过期、丢帧、连接代次变化重置识别，过载计数可见。
6. GPUI 提供 Provider、en/ja 选择、显式启动/停止与状态，结果进入已有 SubtitleState。重启会话拒绝旧结果，关闭控制窗口保留恢复入口。

## 验证

运行 mise run check；新增设备与付费 API 无关的 PCM 转发、VAD、事件修订、Final、错误恢复、取消、过载与过期测试。单次运行最多 90 秒。图形环境实际启动；真实云端和模型测试若缺少凭据/模型，明确记录未验证，不声称里程碑全部验收通过。

## 范围调整（2026-10-20）

本地模型与 sherpa-onnx 实现延后至 M04；移除当前本地后端代码、构建特性和依赖。M03 扩展阿里云、Google Gemini、火山引擎、腾讯云、百度、OpenAI、Azure OpenAI、xAI，保留 Deepgram；另提供自定义 DashScope / OpenAI Realtime 端点。

用户已选择显示云语音接口自带译文；不另外调用文字翻译服务。不播放服务返回的语音，不创建音频反馈回路。按服务协议区分可修订句段与连续文字流，缺乏关联标识时不猜测跨句配对。每家凭据独立验证，上传目标与收费提示随服务选择更新。

调研使用官方协议和发布资料。当前仍缺真实云端凭据，自动测试使用脱敏协议样例与内存 WebSocket；真实质量、权限和计费验收保持未验证。
