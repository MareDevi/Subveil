# Subveil

Rust + GPUI 原生跨平台实时字幕翻译应用，优先支持 Linux / Wayland / PipeWire。

当前已实现 **M02：PipeWire 系统音频采集**，并保留 M01 透明双语浮窗和离线 mock 演示。点击「开始系统音频采集」后捕获默认输出的 monitor；音频只在本机内存处理，不保存、不上传。真实 ASR 与翻译尚未实现，字幕仍是独立模拟样本。

## 开发环境

统一使用 [mise](https://mise.jdx.dev/) 管理工具与任务，Rust 版本固定在 `mise.toml`。Cargo.lock 固定依赖解析结果。不要另行配置 Rust 工具链版本。

Arch Linux / CachyOS 原生依赖示例（需自行确认系统包操作）：

```sh
sudo pacman -S --needed base-devel pkgconf clang cmake fontconfig freetype2 \
  libxkbcommon libxkbcommon-x11 wayland libxcb openssl zstd vulkan-headers \
  vulkan-icd-loader pipewire
```

安装适合 GPU 的 Vulkan 驱动和中文字体，例如 `noto-fonts-cjk`。音频采集需要运行中的 PipeWire 与提供默认输出 metadata 的会话管理器（本机验证为 WirePlumber）。

```sh
mise trust
mise install
mise run doctor
mise run check
mise run run
# 启动离线字幕演示
mise run demo
```

控制窗口可在 Wayland 或 X11 会话运行；字幕浮窗需要支持 `zwlr_layer_shell_v1` 的 Linux Wayland compositor。X11 会明确提示不支持浮窗，不创建伪置顶窗口。

采集或演示运行时，关闭主控制窗口会保留会话，并打开小型恢复入口。点击「恢复控制窗口」继续操作，关闭恢复入口或点击「退出 Subveil」结束程序。系统托盘仍留到 M06。

`RUST_LOG=subveil=debug mise run run` 可调整日志级别，日志写入 stderr，不保存识别文本或音频。

## 系统音频采集

控制窗口内点击「开始系统音频采集」。默认捕获系统默认输出的 monitor，不采麦克风；没有默认输出时等待设备。切换默认输出或设备断开后自动重新发现，PipeWire 服务中断后每秒重试。界面区分等待、捕获与连接错误，并显示设备、电平、静音、PCM 样本、丢块和重连次数。

默认输出单声道 F32LE、48 kHz PCM。停止采集后可切换为 16 / 24 / 48 kHz，采样率是管线参数，后续由 ASR Provider 能力决定。PipeWire 完成混音和 sinc 重采样，不修改输出音量或强制图采样率。静音阈值为 RMS < -60 dBFS；连续 500 ms 无 PCM 时也清除电平，但连接状态会独立显示。

PCM 队列固定 16 块，每块最多 2048 个单声道样本；满时淘汰最旧块，超过 250 ms 的旧块不再消费。丢块与丢样本累计统计包含溢出、过期和流切换丢弃；无效缓冲单独计数。音频回调不等待 UI、不执行推理或网络操作。M02 消费端仅计算电平，尚未接入识别。

无需图形环境的本机诊断：

```sh
# 默认运行 10 秒，48 kHz；只打印统计，不保存音频
mise run audio:probe
# 运行 60 秒，输出 24 kHz PCM
mise run audio:probe -- 60 24000
```

诊断运行至时限后取消并等待线程退出。正常 GUI 启动不会自动采集；`--demo` 也只启动模拟字幕。音频采样率暂不持久化。Linux 以外平台会明确拒绝采集。

## 字幕样式

控制窗口可调整字体、字号、译文颜色、背景不透明度、宽度、底部距离、行数、显示模式和鼠标穿透。只在点击「保存样式」时持久化，保存操作在后台执行。

默认配置路径为 `$XDG_CONFIG_HOME/subveil/subtitle.toml`；XDG_CONFIG_HOME 未设置或不是绝对路径时，使用 `$HOME/.config/subveil/subtitle.toml`。无有效目录时明确提示，可手动指定路径：

```sh
mise exec -- cargo run --locked -p subveil -- --demo --config /tmp/subveil-subtitle.toml
```

完整选项见 [配置示例](config/subtitle.example.toml)。修改 TOML 后重启加载；无效配置显示错误并暂用默认值，不自动覆盖原文件。当前不支持热重载。

默认显示原文与译文，每种语言最多 3 行；超长文本换行后省略。字幕静默 8 秒后清除，最多保留 32 条历史。仅原文、仅译文模式可切换。停止演示会取消当前模拟会话并清空字幕；隐藏浮窗继续保留会话。

M01 使用屏幕布局中最左、其次最上方的输出，宽度与高度按该输出约束。显示器选择及热插拔适配留到 M06。当前 Hyprland 已验证普通与全屏测试视频叠加；其他 compositor 和独占全屏场景不保证相同行为。

## 常用任务

| 命令 | 用途 |
| --- | --- |
| `mise run build` | 锁定依赖构建 |
| `mise run run` | 启动控制窗口 |
| `mise run demo` | 启动离线 mock 双语浮窗 |
| `mise run audio:probe -- 10 48000` | 无窗口音频统计诊断 |
| `mise run fmt` | 格式化 |
| `mise run fmt:check` | 格式检查 |
| `mise run clippy` | 全目标严格检查 |
| `mise run test` | Workspace 测试 |
| `mise run check` | 顺序运行全部质量检查 |
| `mise run doctor` | 检查工具链与原生依赖 |

## 工程与文档

Workspace 包含 `subveil` 应用、`subveil-core` 核心状态与 `subveil-audio` 音频适配器。核心状态和配置不依赖 GPUI，设备与付费 API 均不参与核心测试。GUI 使用 GPUI 与 gpui-component，同一版本的 gpui-pre 快照提供一致的类型。

- [开发规范](AGENTS.md)
- [完整开发规划](docs/development-plan.md)
- [技术调研与架构限制](docs/technical-research.md)
- [M00 验收记录](docs/m00-validation.md)
- [M01 实施计划](docs/m01-plan.md)
- [M01 验收记录](docs/m01-validation.md)
- [M02 实施计划](docs/m02-plan.md)
- [M02 验收记录](docs/m02-validation.md)

必须使用 Conventional Commits，按可审查的小步勤提交。项目许可证待所有者确认，尚未授予开源许可证。
