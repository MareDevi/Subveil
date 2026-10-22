# 固定公开样本验证（2026-10-22）

本次测试评估已接入路径，不把少量样本上的参考一致率当作通用准确率。没有私人录音、系统音频采集、云上传或付费 API。

## 方法与边界

`mise run asr:quality -- MANIFEST WAV REFERENCE_FILE LANGUAGE cpu clean|quiet|tonal|silence [VAD_MANIFEST]` 使用 SenseVoice Small INT8 CPU，以 20 ms 帧实时回放公开 WAV；可选固定 Silero VAD，对照能量检测。每次最长 85 秒；回放后最多排空五秒，不能保证更慢的工作均完成。quiet 为振幅乘 0.08；tonal 混入 220/277.18/329.63 Hz、各振幅 0.025 的合成音；silence 为全零。合成音不是自然音乐、游戏混音或真实低声语料。

参考来自发布方的公开演示结果，未经独立人工转录；指标仅表示与该参考的一致程度。大小写及标点归一，CER 使用非空白 Unicode 字符，WER 只对英文按空白分词。中文、日文没有合适分词，不输出 WER。脚本仅输出聚合数值，不输出识别文本。公开来源：[SenseVoice 示例](https://k2-fsa.github.io/sherpa/onnx/sense-voice/python-api.html)、[模型说明](https://k2-fsa.github.io/sherpa/onnx/sense-voice/pretrained.html)、[FunAudioLLM 演示](https://fun-audio-llm.github.io/)。

固定音频来自 `csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17` revision `2365baeacb507f821a0c8120fcee3d484dba7a07`，下载时核对发布方 Git blob。SHA-256：

- en：`eb1eb008904465b74c304aad8342e8c7d3c6e61ffe9f66adcaca9cf0f76a93f4`
- ja：`460bd8dccb0d2a5f4e29c628f837be4082d13defc64c3fc21dd1b6bb0e119095`
- zh：`b77f1794fe374a0ba1ee1dc458bfaf9349496cbbfc32780c50ba3c5a7ad8e373`

## 结果

以下是字符编辑数/参考字符数；不是平均 CER。英文括号为词编辑数/15 词。每种语言仅一段公开音频。

| 语言 | 变体 | 能量 VAD | Silero VAD |
| --- | --- | --- | --- |
| en | clean | 3/66（1/15） | 0/66（0/15） |
| en | quiet | 0/66（0/15） | 0/66（0/15） |
| en | tonal | 3/66（1/15） | 0/66（0/15） |
| ja | clean | 0/34 | 3/34 |
| ja | quiet | 0/34 | 3/34 |
| ja | tonal | 0/34 | 2/34 |
| zh | clean | 0/13 | 1/13 |
| zh | quiet | 0/13 | 1/13 |
| zh | tonal | 1/13 | 1/13 |

六次全零对照均无 Final、无虚假文字。24 次均没有丢音频、丢段或丢事件；每次一个初始 generation 连续性重置。日文低振幅 Silero 返回两段 Final，其余非静音各一段；分段数量不能单独证明漏识别或重复。

Silero 对英文参考一致率较高，但日文和中文部分样本较低。因此保持显式选择和能量对照，不自动宣称神经检测更好。未覆盖自然背景音乐、口吃、长暂停、多人说话、长时运行或 Whisper 跨块质量。

整进程峰值 RSS 为 401128–423468 KiB，包含模型加载及回放，不含 GUI/文字翻译。没有游戏、视频或 ASR+MT 并行资源测试。少量运行与构建/演示重叠，不用这些数据推断空闲机器延迟。

工具记录分段音频末尾到 Final 接收的最大耗时；能量检测与 Silero 的分段末尾语义不同，不用该字段比较 VAD 延迟。首次采集文件字段名曾为 p95，当前已改为 max；每次只有 0–2 段，不构成有意义的尾延迟分布。

## GUI 绘制指标

生产浮窗增加有界 256 项状态接收至 GPUI paint 回调指标，分别记录源文/译文的接收、绘制和未绘制替换数，以及最近 256 个样本的 p50/p95。按 session/segment/revision 与每阶段序号拒绝过时或重复绘制。默认日志不包含字幕；调试日志仅含阶段与耗时。开始新会话清空指标。

该指标止于 GPUI 绘制回调，不能证明 compositor 合成、屏幕扫描、文字未被裁切或用户读完。隐藏语种、隐藏窗口和被替换的状态没有绘制耗时样本，诊断页明确展示此边界。

生产 OverlayView 测试确认纯译文模式只计译文，重复绘制不重复计数，切换双语后计源文。实际 Wayland 离线演示 12 秒，源文三次 43.249/32.651/31.248 ms，译文一次 89.297 ms；这些是 mock 状态到回调的少量样本，不是完整音频到屏幕或通用 p95。未采集音频。
