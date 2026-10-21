# M05 本地模型、GPU 与离线字幕

日期：2026-10-21，Asia/Shanghai。

## 初始检查与调研

- main 与 origin/main 同步；保留用户未提交的 .gitignore 修改。
- Rust 1.99.1、原生系统依赖与 PipeWire 检查通过。磁盘剩余约 345 GiB。
- 沙箱外只读查询确认 RTX 4060 Laptop、8188 MiB、驱动 615.78.08；沙箱内设备不可用。
- 现有 sherpa-onnx 1.13.8 为 CPU 静态库。上游 CUDA 实现可能回退 CPU，因此不能仅设置 provider="cuda" 后宣称 GPU 可用。
- Context7 查询 sherpa-onnx 与 utilityai/llama-cpp-rs，再核对发布源码。采用 llama-cpp-2 0.1.160 安全 Rust API，默认编译 Vulkan 后端，运行设备由用户显式选择 CPU / GPU。设备检测与模型加载均在后台执行；GPU 不可用则报错，不静默回退。
- 官方 Qwen/Qwen2.5-1.5B-Instruct-GGUF，固定 revision 91cad51170dc346986eccefdc2dd33a9da36ead9，Q4_K_M 权重约 1.12 GB，Apache-2.0。支持中英日文字翻译；生成质量须实测，不能以语言支持替代准确率结论。

## 实施清单

1. 复用显式下载、固定 SHA-256、进度与取消机制，加入翻译模型及磁盘空间预检。
2. 原生后台加载 GGUF，有限上下文与输出，逐 token 检查取消和截止时间；停止释放模型，不阻塞 UI 或音频线程。
3. GUI 选择本地翻译、CPU/GPU、安装或导入模型；显示真实可用设备、加载状态和错误。
4. 完全离线模式明确拒绝云端 ASR、云端翻译与模型网络下载；已安装模型可校验后选用。
5. 保留字幕 Session/Segment/revision 保护及有界队列，翻译失败不终止 ASR。补齐设备无关测试和真实 CPU/GPU、日英字幕链路验收。
6. mise run check、真实窗口运行、文档、审查提交并推送 origin/main。单次测试运行不超过 90 秒；长时稳定性不在本轮伪造验收。

## RTX 4060 Laptop 后续优化目标（2026-10-21）

参考用户提供的模型比较，增加资源与延迟验收规范，详见 [GPU ASR 选型与性能验收](gpu-asr-evaluation.md)。首次参考材料整理仅更新规划；随后按用户要求完成 Whisper CUDA 与短块管线，实现和目标实测见 [GPU ASR 验收](gpu-asr-validation.md)。

- 以 ASR 与文字翻译合计常驻显存尽量不超过 4 GiB、热态首条中文字幕 1–3 秒、持续处理速度至少 2 倍实时为优化目标。分别记录峰值显存、冷启动、原文与中文延迟，以及游戏 / 视频并行运行的影响。
- 优先评估 Rust 绑定 whisper.cpp 的 CUDA Whisper large-v3-turbo，与现有 CPU SenseVoice / Zipformer 比较；继续复用 Qwen 本地文字翻译。选定兼容版本、固定权重校验值并完成本机推理前，不增加可用能力声明。
- Whisper 滑窗重复解码与真正 streaming 分开描述。现有仅 Final 后翻译策略会等待句末；仅更换模型不能保证 1–3 秒中文目标。增量中文方案需要独立设计修订、稳定提交与过载策略。
- 保留 CPU 路径；CUDA 与 Vulkan 分开验收。SeamlessStreaming / SeamlessM4T / Canary 保留为研究候选，不凭参数量或公开跑分切换默认模型。

## 来源

实施中发现 sherpa 预编译静态库与本机 GCC 16 编译的 llama.cpp 在同一进程触发 `std::bad_alloc`。GDB 定位到 main 前的 C++ regex 静态初始化，而独立 llama 进程正常。最终将模型目录与校验拆为无推理运行库依赖的 `subveil-models`，使用独立 Rust `subveil-inference` 程序加载 GGUF；GPUI 进程不链接 llama。此变更同时提供可强制终止的取消与超时边界，保留 sherpa 与 GPUI 现有发布来源。

- https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.8/rust-api-examples/for-advanced-users.md
- https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.8/sherpa-onnx/csrc/session.cc
- https://docs.rs/llama-cpp-2/0.1.160/llama_cpp_2/
- https://github.com/utilityai/llama-cpp-rs
- https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/tree/91cad51170dc346986eccefdc2dd33a9da36ead9
