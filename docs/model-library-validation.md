# 模型库选型与运行验收

日期：2026-10-21，Asia/Shanghai。调研开始于 2026-10-20。

用户授权提前实施 M05 的模型下载子集；本轮完成应用内目录、显式安装与校验、选择运行，以及 SenseVoice 离线分段后端。未实施本地翻译、CUDA 或完整 M05 模型管理。

## 本机配置与选择

- Intel Core Ultra 9 185H：16 核、22 线程，AVX2 / AVX-VNNI。
- 内存约 32 GB，系统显示 30 GiB，总可用内存约 22 GiB；检查时项目磁盘剩余约 347 GiB。
- NVIDIA RTX 4060 Laptop：8188 MiB 显存，驱动 615.78.08；另有 Intel Arc 核显。沙箱内无法访问 GPU，沙箱外 nvidia-smi 正常。
- 当前 sherpa-onnx 1.13.8 运行库仅使用 CPU。GPU 检测正常不能替代 CUDA 原生库、执行提供者和兼容性验证。

| 模型 | 实际目录下载量 | 语言 | 方式 | 建议 |
| --- | --- | --- | --- | --- |
| SenseVoice Small INT8 | 239.5 MB | 日 / 英 / 中 | offline + 能量 VAD，仅 Final | 日语首选候选；接受句末等待 |
| Zipformer English 20M INT8 | 43.6 MB | 英 | 真正 streaming | 英文低延迟与轻量基线 |
| Zipformer Chinese / English INT8 | 198.3 MB | 英 / 中 | 真正 streaming | 中英场景候选，需比较实际内容准确率 |

这是根据可用内存、CPU 能力、后端兼容性与短样本推理作出的选型，不是全语料质量排名。SenseVoice 日语验收解决了前一轮只有清单语言入口、没有已验证日语模型的问题；它没有被改称 streaming。

## 下载与运行实现

- GUI 标准 Select 选择模型，显示模式、语言、下载量、CPU、来源及可点击的完整许可证。显式点击「安装并选用」才下载；完成后再点击「开始本地识别」。CLI `mise run models` 可直接打开模型页面。
- 固定 HF 仓库 revision 和每个文件的长度 / SHA-256，含来源 README。权重使用发布方 LFS SHA-256；小文件先对照发布方 Git blob ID，再生成目录内 SHA-256。不会以浮动 main 作为下载版本。
- 正式目录为 XDG_DATA_HOME/subveil/models，或 ~/.local/share/subveil/models。三个模型均通过应用同一 Download::start 安装器真实下载至本机正式目录，合计约 481.4 MB，全部通过校验。没有权重或 WAV 进入仓库。
- 后台流式下载、有界块读取、进度、取消、重试、完整文件缓存复用；长度和哈希通过才原子发布，运行清单最后发布。独占安装锁防止同包并发安装；崩溃后释放锁，重试清理未提交 .part。
- 不支持部分文件 HTTP Range 续传；已验证完整文件可复用。网络跳转仅支持 HF 官方 HTTPS CDN，无音频或文本上传。
- SenseVoice 使用 offline API、4 个 CPU 推理线程、有界 PCM 缓冲与 VAD 分段；streaming 使用 2 个推理线程。运行时锁定模型，停止后释放；单次原生调用不能强制中断。
- 安装进度由模型库状态拥有，关闭控制窗口仍保留恢复入口。安装失败不激活新模型，不静默转为云端。

## 实测

公开 WAV 与模型都从相应固定 revision 获取，测试 WAV 对照发布方 Git blob ID 验证。通过实际 FrameReceiver / Recognition，以 20 ms PCM 块模拟实时输入，补两秒尾静音。只输出计数，不打印文本或上传音频。

| 模型与官方样本 | Partial / Final | 重置 / 丢块 / 丢事件 | 单段 decode 用时 |
| --- | --- | --- | --- |
| 英文 20M：test_wavs/0.wav | 16 / 1 | 0 / 0 / 0 | 未单独计时 |
| SenseVoice：日语 ja.wav | 0 / 1 | 0 / 0 / 0 | 135 ms / 6380 ms 音频 |
| SenseVoice：英语 en.wav | 0 / 1 | 0 / 0 / 0 | 150 ms / 6700 ms 音频 |
| 中英 Zipformer：test_wavs/0.wav，zh 提示 | 24 / 4 | 0 / 0 / 0 | 未单独计时 |

补充资源测量：单独 SenseVoice 日语 WAV 进程峰值 RSS 为 411168 KiB（约 402 MiB），该次 decode 141 ms，整个程序耗时 12.86 秒（含加载与按真实时钟重放）。这些数字不含完整 GUI 内存，也不是长时或多场景基准。decode 不含模型加载、等待句末分段、翻译或显示，不能当作端到端延迟。

## 自动检查与边界

- `mise run check` 通过：格式、锁定构建、严格 Clippy、78 项测试。应用 3、CLI 2、ASR 43、音频 11、字幕/配置 12、翻译 7。
- 新增 7 项测试覆盖目录固定版本/路径与清单兼容性、校验下载后才发布、无网络缓存复用、损坏与超限不激活、取消及同包安装锁、HTTP 响应中途停滞取消与临时文件清理、PCM 前置缓冲与有界分段。
- `mise run doctor` 与 `git diff --check` 通过。未升级 GPUI 或现有依赖版本；Cargo.lock 只增加 ASR 对既有 reqwest / rustls 的依赖关系。
- 尝试引入 GPUI Kit 0.8.0 test-support 时，解析涉及 gpui-fast-macos 对 cocoa =0.26.0 的约束，与现有 gpui-pre-macos 锁定 cocoa 0.26.1 冲突。未改动或混用 GUI 来源以绕过，未将未编译的 UI 测试记为通过。
- 真实 Wayland 启动 `subveil --models`，10 秒时限后结束，未提前退出；仍有既存第三方 Vulkan 隐式层错误。没有自动化核实最终像素、下载按钮、选择器、取消、恢复和键盘操作，因此完整 GUI 交互尚待人工验收。
- 没有真实系统播放音频到模型、翻译接口和浮窗的完整闭环质量验收；真实翻译账号、长时稳定性、GPU 推理仍未验证。现有文字翻译授权与错误隔离继续保留。
- 模型文件跨重启保留，当前选中项暂不持久化；没有模型删除、磁盘空间预检、GPU 选择或本地翻译。写入空间不足会明确失败，不产生已激活的未校验模型。

## 来源与许可

- 英文模型：https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17/tree/d42f2d9f7ca24806fb667456a18a9f1b60f70d16 ，模型卡 Apache-2.0。
- 中英模型：https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20/tree/98590b7ed6443e77b714204da2757d75e1a642f4 ，模型卡 Apache-2.0。
- SenseVoice ONNX：https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/tree/2365baeacb507f821a0c8120fcee3d484dba7a07 。转换仓库指向上游模型；上游模型卡引用自定义 FunASR Model License，未误标为 Apache-2.0。
- 上游模型卡：https://huggingface.co/FunAudioLLM/SenseVoiceSmall/blob/main/README.md 。
- 完整条款：https://github.com/modelscope/FunASR/blob/main/MODEL_LICENSE 。
- Rust API：https://docs.rs/sherpa-onnx/1.13.8/sherpa_onnx/ ，Context7 后核对实际 1.13.8 源码。
