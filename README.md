# Subveil

Rust + GPUI 原生跨平台实时字幕翻译应用，优先支持 Linux / Wayland / PipeWire。

当前处于 **M00：技术验证与工程初始化**。已建立工程基础与最小控制窗口；尚不支持音频采集、ASR、翻译或字幕浮窗。程序不会采集音频，也不会连接云端服务。

## 开发环境

统一使用 [mise](https://mise.jdx.dev/) 管理工具与任务，Rust 版本固定在 `mise.toml`。Cargo.lock 固定依赖解析结果。不要另行配置 Rust 工具链版本。

Arch Linux / CachyOS 原生依赖示例（需自行确认系统包操作）：

```sh
sudo pacman -S --needed base-devel pkgconf clang cmake fontconfig freetype2 \
  libxkbcommon libxkbcommon-x11 wayland libxcb openssl zstd vulkan-headers \
  vulkan-icd-loader pipewire
```

安装适合 GPU 的 Vulkan 驱动和中文字体，例如 `noto-fonts-cjk`。PipeWire 在 M00 仅检查开发库，实际采集留到 M02。

```sh
mise trust
mise install
mise run doctor
mise run check
mise run run
```

运行需要正常的 Wayland 或 X11 图形会话。窗口关闭或点击退出按钮结束进程。`RUST_LOG=subveil=debug mise run run` 可调整日志级别，日志写入 stderr，不默认生成日志文件。

## 常用任务

| 命令 | 用途 |
| --- | --- |
| `mise run build` | 锁定依赖构建 |
| `mise run run` | 启动控制窗口 |
| `mise run fmt` | 格式化 |
| `mise run fmt:check` | 格式检查 |
| `mise run clippy` | 全目标严格检查 |
| `mise run test` | Workspace 测试 |
| `mise run check` | 顺序运行全部质量检查 |
| `mise run doctor` | 检查工具链与原生依赖 |

## 工程与文档

目前只包含一个应用 crate；按实际需求逐步拆分，避免空模块。GUI 使用 GPUI，组件使用 gpui-component，同一版本的 gpui-pre 快照提供一致的类型。

- [开发规范](AGENTS.md)
- [完整开发规划](docs/development-plan.md)
- [技术调研与架构限制](docs/technical-research.md)
- [M00 验收记录](docs/m00-validation.md)

必须使用 Conventional Commits，按可审查的小步勤提交。项目许可证待所有者确认，尚未授予开源许可证。
