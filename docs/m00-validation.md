# M00 验收记录

日期：2026-10-20（Asia/Shanghai）。范围仅为技术验证与工程初始化。

## 已完成

- 先读取原始规划并调研，再编写和提交 AGENTS.md，随后开始工程开发。
- mise 固定 Rust 1.99.1 与 rustfmt / Clippy，提供统一开发任务。
- 建立单应用 crate 的最小 Cargo Workspace，提交锁文件，不创建空壳业务模块。
- 使用兼容发布组合 gpui-pre 0.3.9 / gpui-component 0.8.0，实现真实 GPUI 控制窗口。
- 初始化组件系统与 Root，显示中文工程状态、隐私说明和退出按钮。
- tracing 日志写入 stderr；继承 RUST_LOG，未设置时默认 info。
- 提供环境检查脚本、README、技术调研、架构边界和 CI 流程。
- 按独立工作单元使用 Conventional Commits 提交。

## 自动检查

| 检查 | 结果 |
| --- | --- |
| `mise run doctor` | 通过，Rust 与所需原生库可用 |
| `mise run build` | 通过，实际编译并链接 GPUI 与组件库 |
| `mise run fmt:check` | 通过 |
| `mise run clippy` | 通过，`--workspace --all-targets --locked -- -D warnings` |
| `mise run test` | 通过，当前 0 个测试；M00 尚无可独立验证的业务逻辑 |
| `git diff --check` | 通过 |

上述任务通过 `mise run check` 顺序执行。首次编译发现 on_window_closed 回调需要 `(App, WindowId)` 两个参数；按所选发布包源码修正并通过检查。

## 实际运行

- 在现有 Hyprland / Wayland 会话执行 `mise run run`，并使用 `RUST_LOG=info mise run run` 复核启动日志。
- compositor 返回 `class=subveil`、`title=Subveil`、`mapped=true`、`hidden=false`、`xwayland=false`，确认原生 Wayland 窗口实际映射。
- 仅截取应用窗口并检查：中英文文字、状态面板和退出按钮正常显示，无字体缺失或文字截断。
- 日志确认 NVIDIA GeForce RTX 4060 Laptop GPU 通过配置测试并被选为 Vulkan 渲染设备，随后输出 `control window opened`。
- 使用 compositor 向测试进程的窗口发送正常关闭请求，应用退出码为 0，未残留后台进程。

本机平铺布局覆盖了应用请求的 760 × 460 初始窗口尺寸，属于 compositor 正常行为。截图与运行日志保留在临时目录，未提交用户桌面数据。

## 已知问题与验证边界

- 本机第三方 Vulkan 隐式层 `liblsfg-vk-layer.so` 缺少 `vkGetInstanceProcAddr`，loader 报错并跳过该层。GPUI 仍成功选择 GPU 并渲染窗口。未修改用户 Vulkan 配置。
- 本机 RUST_LOG 原值为 warn，因此默认启动不显示 info；使用显式 RUST_LOG=info 已验证日志。
- GPUI 的 `activate(true)` 在 Linux 是无操作；已移除该调用，窗口使用正常创建时的焦点行为。
- 自动检查不代表 GUI 交互覆盖。退出按钮已渲染，未模拟鼠标点击；正常窗口关闭与进程退出已实测。
- CI 文件已提供，尚未在 GitHub runner 运行；Ubuntu、X11、其他 compositor、Windows、macOS 未实测。
- 当前没有字幕浮窗、配置持久化、系统托盘、音频捕获、ASR、翻译或云端连接；不声称可用。
- 项目开源许可证仍待所有者指定，尚未添加 LICENSE。

## 下一步

M00 验收完成后停止。本轮不执行 M01。后续 M01 单独实现并验收 Wayland Layer Shell、透明双语字幕、输入穿透、无键盘焦点及字幕状态管理，继续保持无需音频设备的 mock 驱动。
