# M05 使用与短时验收

日期：2026-10-21，Asia/Shanghai。

> 下文保留原 M05 基线记录。新增 Whisper Turbo CPU / CUDA、短块字幕与资源实测见 [GPU ASR 使用与验收](gpu-asr-validation.md)；原表中 ASR 仍为 CPU 的描述只适用于 sherpa 后端。

## 开始使用

1. `mise run doctor`，再运行 `mise run models`。首次需构建原生推理库；Arch/CachyOS 需 shaderc 提供 glslc，及适合设备的 Vulkan 驱动。
2. 识别选择 SenseVoice（日 / 英 / 中，句末输出）或 Zipformer（英文 / 中英，真正 streaming）。核对来源与许可，点击「安装并选用」。现有本机三个 ASR 模型可校验后复用。
3. 「翻译服务」选择「本地离线翻译」，核对官方 Qwen2.5 1.5B Instruct Q4_K_M，点击「安装并选用翻译模型」。约 1117.3 MB，Apache-2.0，固定版本及 SHA-256。此模型已安装在本机 XDG 数据目录。
4. 「翻译运行设备」轮换 CPU 和检测到的 Vulkan GPU。本机可见 RTX 4060 Laptop 与 Intel Arc；实测加速使用 RTX 4060。GPU 失败明确报错，不自动选择 CPU 或云端。语音识别仍使用 CPU。
5. 选源语种及目标语种（中 / 英 / 日），开启「离线模式」，点击「开始本地识别」。等待翻译模型准备后才打开系统音频采集；语音模型随后在后台加载。无需 API Key、模型服务或文本上传授权。
6. 「实时字幕」预览及浮窗显示原文与译文。「停止识别与音频」取消会话，释放本地翻译进程和语音模型。识别未结束的句子可能丢弃。
7. 「保存本地设置」保存模型清单路径、语种、离线模式和设备名至与字幕样式同目录的 `local-models.toml`。默认使用 XDG 配置目录；恢复时不自动开始采集，不恢复任何上传授权。原设备不可用时保留失败状态，须显式重新选择设备。

完全离线模式禁止云端 ASR、云端文字翻译和模型下载。「安装并选用」此时只校验本机已缓存文件；缺失或损坏明确失败。首次安装需先关闭离线限制。来源 / 许可证链接仍是用户主动打开的外部链接。

自有 GGUF 可通过「导入翻译清单…」导入，格式见 `config/local-translation.example.toml`。本轮验证 Qwen2.5 架构及内置 chat template，不保证任意 GGUF、语言或硬件兼容。

## 实现边界

- CPU ASR：sherpa-onnx 1.13.8；原有 streaming 与 SenseVoice 分段语义保留。没有宣称 sherpa CUDA 或 GPU ASR 已可用。
- CPU / Vulkan 翻译：llama-cpp-2 0.1.160。独立 `subveil-inference` 程序隔离与 sherpa 预编译 C++ 库的 ABI 冲突，也保护 GUI 不受原生崩溃影响。安装时将两个程序放在同一目录；高级诊断可通过绝对路径 `SUBVEIL_INFERENCE_BIN` 指定 helper。
- 下载器与校验移至共享 `subveil-models`，ASR 保留公开再导出；无推理库类型交叉或 GPUI 来源变化。下载前读取可用磁盘空间，并按文件预检；进度显示校验 / 下载阶段、完成字节、错误和取消。权重、公开测试 WAV、截图与日志均未进入 Git。
- 模型加载限时 30 秒；GUI 本地翻译任务从提交起最多 30 秒（包括排队）。8 条输入、64 条结果，过载淘汰最旧请求并显示失败。IPC 每行最多 64 KiB，读取通道容量 1；请求编号和原有 Session/Segment/revision 双重关联。超时 / 停止终止 helper，无静默重试和云端回退。
- 本地翻译按句使用模型内置模板，无历史上下文；最多 1536 输入 token、256 输出 token、2048 上下文。超限明确失败，不显示截断译文。云端保留既有 6 秒期限及最多三句上下文。
- 原文不经命令行、临时文件或日志传递。模型进程仅通过管道接收当前句。默认禁用原生详细日志。

## 实测记录

机器：Core Ultra 9 185H，约 32 GB 内存；NVIDIA RTX 4060 Laptop 8188 MiB，驱动 615.78.08。同一 Vulkan runtime 也枚举 Intel Arc，但未对 Arc 执行推理验收。

| 场景 | 结果 |
| --- | --- |
| 固定英文与日文短句，经 Qwen CPU 翻译为中文 | 两句通过「天气」语义检查；分别约 661 / 611 ms；停止后确认模型卸载 |
| 同样短句经 RTX Vulkan 翻译为中文 | 两句通过相同检查；首次约 3361 ms（含冷启动影响），随后约 100 ms；停止后确认卸载 |
| 官方日语 WAV → SenseVoice → RTX 翻译 → SubtitleState | 1 Final、1 中文译文、0 pending；按真实时钟重放加尾静音约 10.24 秒 |
| 受限沙箱内官方英语 WAV → CPU ASR → CPU 翻译 → SubtitleState | 1 Final、1 中文译文、0 pending；约 10.19 秒，无需设备或联网 |
| 官方英文 WAV 经真实 PipeWire 系统输出 monitor → SenseVoice → RTX 翻译 → SubtitleState | 1 Final、1 中文译文、0 pending；采集及排空约 10.19 秒 |
| 最终版本的真实日语 PipeWire 系统输出链路 | 1 Final、1 中文译文、0 pending；采集及排空约 15.52 秒，整体命令约 19.20 秒 |
| 推理过程中 NVIDIA 进程表 | `subveil-inference` 标记 C+G，约 938 MiB 显存；证明实际发生 GPU 模型卸载 |
| Wayland 模型页面启动及截图检查 | 恢复本地模型、ja→zh、离线模式与 RTX 设备；实际窗口可渲染，25 秒限时内未提前退出 |

这些是短时功能诊断，C++ 运行库采用 RelWithDebInfo，Rust 为开发构建；不是 release 性能基准、翻译质量评分或端到端延迟承诺。GPU 首次编译 / 驱动预热可能慢于 CPU。最终对开发构建 SHA-256 依赖启用优化，避免 GB 权重校验占满加载期限；正式 release 自带优化。

公开 WAV 来源为固定版本 SenseVoice 仓库 `test_wavs/ja.wav` 与 `test_wavs/en.wav`，读取后核对发布方 Git blob ID，未上传任何用户音频。SHA-256 分别为 `460bd8dccb0d2a5f4e29c628f837be4082d13defc64c3fc21dd1b6bb0e119095`、`eb1eb008904465b74c304aad8342e8c7d3c6e61ffe9f66adcaca9cf0f76a93f4`。

## 自动验证与未验证项

- `mise run check` 通过：格式、锁定构建、全目标严格 Clippy、84 项测试。应用 4、CLI 2、ASR 37、音频 11、核心 12、模型 8、翻译 10。
- 新测试覆盖离线安装拒绝缺失 / 损坏、翻译权重校验、设置恢复、原生进程退出及超长协议、取消及超时强制终止。原有损坏下载、缓存复用、安装锁、HTTP 停滞取消、过载、修订与过期保护继续通过。
- 测试需要绑定本机回环 HTTP/WebSocket 测试桩；沙箱禁止绑定端口时出现 `Operation not permitted`，已在真实本机环境重跑完整门禁通过，不调用真实付费服务。
- 真实窗口存在既有第三方 LSFG Vulkan 隐式层诊断，未改系统设置；应用可启动和渲染。截图只检查显示，不代表按钮、键盘、焦点、滚动、取消和恢复入口已完成全面交互验收。
- 30 分钟稳定性、多场景识别 / 翻译质量、任意 AMD / Intel GPU、其他 compositor 和独占全屏仍未验证。托盘、全局快捷键、打包发布、历史导出等继续属于 M06。未使用真实云端翻译账号验收。

## 重复诊断

```sh
mise run translate:probe --devices
mise run translate:probe /absolute/path/translation/model.toml cpu
mise run translate:probe /absolute/path/translation/model.toml gpu
mise run offline:probe /absolute/path/asr/model.toml /absolute/path/translation/model.toml /absolute/path/ja.wav ja gpu
# 最后参数显式播放公开 WAV，经真实系统输出采集；请勿在有私密音频播放时运行。
mise run offline:probe /absolute/path/asr/model.toml /absolute/path/translation/model.toml /absolute/path/en.wav en gpu --system
```

单次诊断有 90 秒外部时限；不会输出原文或译文。仅固定公开短句诊断检查特定语义，音频链路诊断检查 Final、非空中文译文和字幕身份，不据此声称识别准确率。

来源：官方 [Qwen GGUF](https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/tree/91cad51170dc346986eccefdc2dd33a9da36ead9)、[SenseVoice ONNX 与 WAV](https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/tree/2365baeacb507f821a0c8120fcee3d484dba7a07)、[llama-cpp-2 API](https://docs.rs/llama-cpp-2/0.1.160/llama_cpp_2/)。模型许可与来源在应用安装前展示，未改变项目许可证归属。
