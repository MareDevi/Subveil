# M01：Wayland 字幕浮窗

M00 已完成，用户于 2026-10-20 授权推送远程并进入下一里程碑。本轮仅执行 M01。

## 任务清单

1. 检查仓库与远程，推送已有 M00 提交。
2. 用 Context7 与固定版本源码确认 Layer Shell、输入区域、Entity 观察、计时任务和窗口关闭 API。
3. 建立独立于 GPUI 的字幕状态：Session / Segment / revision、Partial / Final、翻译状态、过期和有限历史；测试修订与乱序拒绝。
4. 创建透明 Overlay Layer 窗口，底部锚定，无键盘焦点，空输入区域实现点击穿透。
5. 使用可停止的 mock 驱动演示增量原文和译文，明确标记模拟，无录音和联网。
6. 支持字体、字号、背景透明度、宽度、底部距离、显示模式及穿透配置。TOML 配置遵循 XDG 路径，错误可见。
7. 控制视图与字幕共用状态；控制窗口关闭后保留小型恢复入口，恢复时不重置字幕。托盘仍留到 M06。
8. 运行完整 mise 检查，实际验证 Wayland 协议、界面、关闭恢复、长文本、隐藏恢复和鼠标穿透；记录不能验证的项目。
9. 使用 Conventional Commits 小步提交，推送已验证的 M01 结果。

## 技术决策

- 保持 GPUI 快照与组件库版本不变。
- 新增 `subveil-core`，容纳真实使用的字幕状态与配置；平台窗口集中在应用 `overlay` 模块，不创建 Windows/macOS 空实现。
- GPUI Entity 持有会话状态，窗口仅观察；mock 任务归会话所有，停止时取消，过期使用单调时钟。
- 样式与几何参数变化可重建 Layer Shell 窗口，但不能重置会话、字幕或 mock 进度。
- 不支持 Layer Shell 或非 Wayland 会话时，控制界面给出真实错误，不回退到冒充浮窗的普通窗口。
- 无键盘交互与点击穿透是独立设置；透明背景本身不保证穿透。
- 关闭控制窗口后打开小型会话窗口，提供恢复与退出。关闭小型窗口即结束程序，避免无管理入口的后台浮窗。

## 核对来源

- [GPUI 0.3.9 Layer Shell 类型](https://docs.rs/crate/gpui-pre/0.3.9/source/src/platform/layer_shell.rs)
- [GPUI Window 输入区域与窗口生命周期](https://docs.rs/crate/gpui-pre/0.3.9/source/src/window.rs)
- [GPUI Entity Context](https://docs.rs/crate/gpui-pre/0.3.9/source/src/app/context.rs)
- [所选 Linux 平台实现](https://docs.rs/crate/gpui-pre-linux/0.3.9/source/src/linux/wayland/window.rs)
- [Rust TOML 序列化接口](https://docs.rs/toml/latest/toml/)

静态源码显示所需 API 可用；是否满足实际 compositor 行为，以本里程碑验收记录为准。
