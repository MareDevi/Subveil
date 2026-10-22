# Linux 安装包与构建边界

`mise run package:cpu` 生成本机 Linux CPU Whisper / Vulkan 翻译包；`mise run package:cuda` 另外加入独立 `subveil-whisper-cuda`。产物位于 `target/packages`，包含三个或四个程序、安装脚本、逐文件 SHA-256、依赖许可清单和发布 crate 内的许可文本。权重、密钥、字幕和用户设置不打包。当前项目尚未声明自己的分发许可证；这些是本地验收产物，不自动上传或发布，公开分发前仍需所有者决定项目许可并核对第三方许可完整性。

解压后运行 `bash install.sh /absolute/prefix`，默认安装到 `$HOME/.local`。程序保存在 `lib/subveil`，`bin/subveil` 是相对软链接，桌面文件写入对应前缀。无需 root；更新 CPU 包会移除该前缀中的旧 CUDA helper，以防混装。安装不下载模型、不读取密钥、不开始采集，也不设置系统服务。安装前缀必须为绝对路径，空格与桌面 Exec 保留字符会转义，换行路径拒绝。

`subveil --check-installation` 不启动 GUI，也不加载模型或枚举 GPU；它要求安装目录含受校验的 `bundle-backend` 声明，并按 CPU/CUDA 包角色验证所需 helper 存在、可执行，协议/应用版本/构建标识一致，CPU 或 CUDA 能力符合文件角色。缺少声明时自检失败；开发目录请先用 `mise run package:debug` 生成完整包。每个 helper `--capabilities` 只报告编译能力；实际推理仍须独立验收。`SUBVEIL_BUILD_ID` 可在整套编译前统一设置为发布提交标识；不能为单个 helper 使用不同标识。

普通 `mise run build` 始终生成 CPU `target/debug/subveil-whisper`。CUDA 在 `target/cuda` 构建，然后复制为 `target/debug/subveil-whisper-cuda`，两者不会互相覆盖。CUDA 仍仅支持设备 0，设备/显存/原生加载失败明确报错，不回退 CPU。`SUBVEIL_WHISPER_BIN` 诊断覆盖也必须通过握手，并不绕过后端验证。

安装机需要兼容的 Linux/glibc、libstdc++、PipeWire、XCB/XKB、Wayland 与 Vulkan loader/驱动，以及字体。密钥环为可选的 libsecret/Secret Service。CUDA 包额外要求匹配的 NVIDIA 驱动；驱动和 CUDA Toolkit 不打包。CI 在 Ubuntu 24.04 构建并检查 CPU 包；本机生成包只证明本机 ABI，不代表更旧发行版兼容。字幕浮窗要求 compositor 支持 Layer Shell；X11 和 Windows/macOS 没有伪造的采集/浮窗适配。

本地验收可用 `mise run package:debug` 快速打包已编译产物；它会 strip 副本，保留原构建。安装自检不等于干净物理机器 GUI、CUDA 推理、多屏或长时稳定性验收。


2026-10-22 已实际构建优化 CPU/CUDA archive（协议 3），不含模型。安装 smoke 扩展覆盖两个包的独立空白前缀、逐文件校验、各必需 helper/后端声明缺失、协议错配及 CUDA 更新为 CPU 的旧 helper 清理。新增声明修复了“CUDA helper 被删后误当 CPU 包通过”的实际缺陷。干净物理机器、其他发行版及公开发布仍未验收；本机临时前缀不替代这些边界。
