# 应用内本地模型目录与安装

日期：2026-10-20，Asia/Shanghai。

用户要求将本地模型选择、下载与运行放入应用，以便使用及完整链路验证。本轮明确提前实施 M05 的模型下载子集，不实施本地翻译或 GPU 后端。

## 检查与选型

- main 与 origin/main 同步，仅用户 .gitignore 修改未提交，保留。
- Rust 1.99.1、构建系统依赖检查通过。
- 本机 Core Ultra 9 185H，16 核 22 线程，内存约 32 GB（可用约 22 GiB），磁盘剩余 347 GB。
- RTX 4060 Laptop，8188 MiB 显存，NVIDIA 615.78.08。沙箱内 GPU 设备不可见，沙箱外只读查询正常。现有 sherpa 原生库为 CPU，不据此宣称 CUDA 可用。
- 使用当前官方 sherpa-onnx 1.13.8，保留 streaming transducer；增加 SenseVoice offline API，通过有界 VAD 分段调用，只返回 Final。

候选：英文 Zipformer 20M INT8（约 44 MB），中英 Zipformer INT8（约 198 MB），SenseVoice Small INT8（约 240 MB，日/英/中）。前两者模型卡标注 Apache-2.0；SenseVoice 上游模型卡引用 FunASR Model License，不能误标为代码的 MIT 许可证。

## 实施清单

1. 固定模型仓库 revision、文件长度和 SHA-256；目录展示来源、许可证、下载量、语言与运行模式。
2. 下载由显式按钮触发，后台流式写入、长度与哈希验证、同目录临时文件原子发布，清单最后提交。支持进度、取消、重试、缓存复用；校验失败不激活。
3. 存入 XDG_DATA_HOME/subveil/models，未设置时使用 ~/.local/share/subveil/models；不写仓库或 /tmp 作为正式存储。
4. GPUI 使用标准 Select / Button / Link。模型库状态拥有下载生命周期；Session 接收已安装模型并切换到本地识别。运行期间禁用模型切换，安装不自动开启音频采集。
5. 下载与识别仅在各自后台线程执行；仅模型 GET 请求，不上传音频或文本，无云端回退。
6. 离线测试覆盖损坏、截断、超限、取消、缓存与模型发布；真实下载后以官方日英 WAV 验证两种运行模式。单次验收命令最多 90 秒。

来源（Context7 resolve/query 后核对）：
- https://docs.rs/sherpa-onnx/1.13.8/sherpa_onnx/
- https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17
- https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20
- https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17
- https://huggingface.co/FunAudioLLM/SenseVoiceSmall/blob/main/README.md
- https://github.com/modelscope/FunASR/blob/main/MODEL_LICENSE
