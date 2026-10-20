# M01 验收记录

日期：2026-10-20，Asia/Shanghai。范围仅为字幕浮窗和离线 mock 演示。

## 已完成

- `subveil-core` 提供字幕状态和 TOML 配置，不依赖 GPUI、音频设备、模型或网络。
- Transcript 携带 session、segment、revision、语言与音频起止时间。Partial 原位修订，Final 不回退为 Partial。
- 翻译状态区分未请求、等待、完成和失败；只接受匹配 session / segment / revision 的等待中结果，过期或重复返回不覆盖字幕。
- 历史数量和单段原文长度有界；新段 ID 递增，已淘汰段不能重新插入。过期条目保留在有限历史中，浮窗只显示最新未过期条目。
- GPUI Layer Shell：透明背景、Overlay Layer、底部居中锚定、不抢键盘焦点、空输入区域穿透。
- mock 驱动演示英文、日文、中文译文和长句；每个计时 tick 只处理一步，不在卡顿后集中回放积压事件。
- 隐藏/恢复浮窗与样式调整不重置会话。停止演示清空会话，旧结果失效。
- 主控制窗口关闭后打开恢复入口；恢复控制窗口复用会话和历史。关闭恢复入口或退出按钮可结束整个进程。
- 字体、字号、颜色、背景透明度、宽度、底部距离、最大行数、穿透和语言显示模式可配置。XDG 配置只在显式保存时写入，采用临时文件与 rename；无效配置提示持续可见。
- 提供 `mise run demo`、`--demo`、`--config`、无需图形会话的 `--help` / `--version`。

## 自动检查

`mise run check` 全部通过：

| 项目 | 结果 |
| --- | --- |
| cargo fmt | 通过 |
| cargo build --workspace --locked | 通过 |
| cargo clippy --workspace --all-targets --locked -- -D warnings | 通过，无豁免 |
| cargo test --workspace --locked | 15 项通过 |
| git diff --check | 通过 |

测试包括：配置 round trip、默认值、NaN / 无穷 / 越界 / 未知字段拒绝、XDG 路径、真实配置文件读写与坏文件保留、64 KiB 文件上限、Partial 修订、Final 稳定、旧翻译拒绝、重复结果、取消会话、过期、历史淘汰、失败保留原文、mock 演示完整周期、无显示器 CLI 和错误参数。

## Hyprland 实测

使用原生 Wayland 会话、当前 GPU、生成的本地无声测试视频。仅操作专用测试窗口；截图、视频、协议日志和临时配置均在 `/tmp`，未提交桌面内容。

| 验证 | 证据与结果 |
| --- | --- |
| Overlay Layer | compositor 的 level 3 中存在 namespace `subveil-subtitles` |
| 底部锚定 | 协议 `set_anchor(2)`、`set_margin(0, 0, 64, 0)`；自定义配置为 80 px，控件调整后为 96 px |
| 无键盘焦点 | `set_keyboard_interactivity(0)`；全屏时 active window 保持为专用播放器 |
| 鼠标穿透 | 协议设置空 region；真实鼠标点击字幕内部坐标，播放器 IPC 中 pause 从 false 变为 true |
| 透明背景 | 测试视频颜色在字幕面板后可见，文字保持不透明 |
| 普通/全屏视频 | 普通窗口与全屏测试播放器上方均保持字幕层，已截取全屏叠加图检查 |
| 长文本 | 英文和中文长样本在控制预览与浮窗中换行；超出设定行数时省略 |
| 隐藏恢复 | 点击隐藏后 compositor 中字幕层消失，恢复后 level 3 中重新出现 |
| 控制窗口关闭恢复 | 关闭主窗口后字幕层持续存在，恢复入口可点击；恢复后的进程 PID、session ID 相同，历史继续增长 |
| 样式与持久化 | 实际点击字体大小、背景透明度、宽度和位置；保存的临时配置分别为 38 px、约 0.55、960 px、96 px |
| 过期清除 | 默认 8 秒配置实际显示空白期，历史标记已过期；单调时钟测试同步覆盖边界 |
| X11 拒绝路径 | 取消 WAYLAND_DISPLAY / WAYLAND_SOCKET 后启动；控制界面显示原生 Wayland 要求，未创建普通窗口冒充字幕浮窗 |
| 坏配置提示 | `font_size = nan` 在演示启动后仍显示独立错误；原始文件未被覆盖 |

## 实测发现与修正

1. `open_window` 会同步渲染。如果在 Session Entity 的 update 中创建读取 Session 的视图，会触发重入读取 panic。所有新窗口通过 `App::defer` 在 update 完成后创建。
2. 所选 Wayland 后端的 `primary_display()` 固定返回 None。改用枚举输出，按布局 x / y 排序选择首个，并约束浮窗尺寸。
3. 字幕预览高度固定，避免 Partial / Final 更新使控制按钮上下跳动。
4. 配置错误与短期状态提示分开保存，开始演示不会掩盖坏配置。

## 已知限制与下一步

- 只验证了当前 Hyprland；GNOME、其他 compositor、独占全屏和其他 GPU 配置未验证。
- 多屏环境已确认浮窗落在布局首个输出；没有显示器选择或热插拔恢复功能，留到 M06。
- 样式几何变化会重建 Layer Shell 窗口，可能有短暂重映射；字幕状态与历史不受影响。
- 控制窗口后的恢复入口是普通小型窗口，不是系统托盘；平铺 compositor 可覆盖请求的窗口尺寸。
- 不包含音频、真实识别、翻译请求、模型管理或系统托盘。所有演示文案均为内置样本。
- M00 发现的第三方 Vulkan 隐式层问题未修改；本轮应用仍正常渲染。
- 实现提交 `3d358f4` 的 [GitHub CI](https://github.com/MareDevi/Subveil/actions/runs/39405907656) 已通过。后续截断标记、配置文件测试与文档提交的状态以 Actions 实时页面为准；本地完整质量门禁包含最终 15 项测试，已通过。

本轮止于 M01。下一里程碑 M02 将实现默认输出 monitor 捕获、PCM、重采样、电平、有界队列和设备恢复。
