# M00 技术调研

调研日期：2026-10-20。先查询 Context7，再核对已发布 crate 与官方源码；主线文档可能领先或落后于发布包。

## 结论与版本

| 项目 | 选择 / 状态 | 依据 |
| --- | --- | --- |
| Rust | Stable 1.99.1，edition 2024，mise 固定 | 本机 mise 工具链实际执行结果 |
| GPUI | `gpui-pre =0.3.9`，依赖名 `gpui` | gpui-component 0.8.0 发布元数据要求精确快照 |
| 平台实现 | `gpui-pre-platform =0.3.9`，依赖名 `gpui_platform` | 平台与 GPUI 使用同一快照；启用 font-kit、wayland、x11、runtime_shaders |
| UI 组件 | `gpui-component =0.8.0` | crates.io 已发布版本，初始化 `init`，窗口根使用 `Root::new` |
| 日志 | tracing 0.1 + tracing-subscriber 0.3 | Cargo.lock 固定实际解析版本，stderr 输出，可用 RUST_LOG 过滤 |
| PipeWire | 调研 0.10.1；M00 不引入依赖 | 发布源码的 audio-capture 示例，Rust 最低版本 1.80 |
| ASR / 翻译 / Tokio | 后续里程碑按需引入 | M00 无推理与网络任务，避免增加未使用依赖 |

`gpui-pre 0.3.9` 的发布元数据标注 Zed revision `93d745668805029476e6d7fbb19652116fed51de`，Zed crate version 0.2.2。这是上游代码的发布快照，并非本项目 fork；不能因此宣称 GPUI API 已稳定。

原需求中的 `gpui-ce/gpui-component` 与当前 `longbridge/gpui-component` 路线已分化。前者主线使用 `gpui-ce`，后者发布包 0.8.0 使用 `gpui-pre`。本项目选择后者，避免不同 GPUI 来源产生不兼容类型。不要将未经核对的 Context7 示例直接复制进代码。

## Wayland Layer Shell

已核对 **所选发布版** 的 `src/platform/layer_shell.rs`、`src/platform.rs` 与 `src/window.rs`：

- `WindowKind::LayerShell(LayerShellOptions)` 存在，需要 Linux + wayland feature。
- `Layer::Overlay`、`Anchor::BOTTOM`、边距与 `KeyboardInteractivity::None` 可表达字幕窗口基本要求。
- `WindowBackgroundAppearance::Transparent` 控制透明背景。
- `Window::set_input_region(Some(&[]))` 将输入区域置空，文档说明仅 Linux Wayland 支持指针和触摸穿透；`None` 恢复默认输入区域。
- 缺少协议时存在 `LayerShellNotSupportedError`，不能用普通顶层窗口静默伪装相同能力。

M00 仅创建普通控制窗口。上述为源码能力确认，**不是浮窗运行验收**。M01 必须独立验证透明、底部定位、无键盘焦点、鼠标穿透、多屏与全屏播放。GNOME 等环境的协议支持及独占全屏行为不得一概承诺。X11 后端仅用于普通控制窗口兼容，不代表 Layer Shell 在 X11 可用。

## PipeWire API 与风险

0.10.1 发布包示例使用 `main_loop::MainLoopRc::new`、`context::ContextRc::new`、`connect_rc`、`stream::StreamBox::new`，再通过 listener 接收格式变化和处理缓冲。示例明确展示 `STREAM_CAPTURE_SINK = "true"` 的 monitor 捕获选项，以及 `TARGET_OBJECT` 的目标选择。

示例默认 Capture 不代表自动选择系统输出 monitor。M02 仍需实现默认 sink metadata 发现、目标跟踪、重连、格式协商、时间戳和有界队列。上游示例中的终端打印、expect 和处理方式不能原样放入实时回调。M00 没有连接 PipeWire 或录音。

## 环境与架构

本机：Arch Linux，Wayland 会话，Rust 1.99.1；wayland-client 1.26.0、xkbcommon 1.13.2、PipeWire 1.6.9、Vulkan 1.4.363、fontconfig 2.18.3、FreeType 26.7.20、OpenSSL 3.6.5、zstd 1.5.7。

初始 Workspace 只有 `crates/subveil`：应用入口、日志、窗口生命周期与控制视图。没有业务模型时不建立 core 空库。后续出现真实需求再将数据模型、管线和平台适配拆出。GPUI 负责事件循环；未来 Tokio 运行时必须通过明确通道与 UI 交互，不能阻塞 UI。

风险：GPUI 及组件库仍快速变化；完整组件库会引入较大传递依赖；首次编译需要网络、磁盘和内存；Linux 运行需要 GPU/Vulkan 驱动、字体及图形会话。精确固定 GPUI 快照与 Cargo.lock，降低 API 漂移。`runtime_shaders` 是平台 crate 提供的 macOS shader 编译选项，与组件库推荐配置一致，不改变 Linux GPU 驱动要求；macOS 未经验证。

开源许可证尚未由项目所有者指定，M00 不擅自选择。发布前须确认项目许可证、依赖许可证及资源授权。

## 官方来源

- [GPUI 上游](https://github.com/zed-industries/zed/tree/main/crates/gpui)
- [上游 Layer Shell 示例](https://github.com/zed-industries/zed/blob/main/crates/gpui/examples/layer_shell.rs)
- [gpui-pre 0.3.9 源码](https://docs.rs/crate/gpui-pre/0.3.9/source/)
- [gpui-component 0.8.0 源码](https://docs.rs/crate/gpui-component/0.8.0/source/)
- [组件库主线 Workspace](https://github.com/longbridge/gpui-component/blob/main/Cargo.toml)
- [需求引用的 CE 分支](https://github.com/gpui-ce/gpui-component/blob/main/Cargo.toml)
- [PipeWire 0.10.1 捕获示例](https://docs.rs/crate/pipewire/0.10.1/source/examples/audio-capture.rs)
- [PipeWire 官方音频捕获示例](https://docs.pipewire.org/audio-capture_8c-example.html)
- [mise Rust 工具链](https://mise.jdx.dev/lang/rust.html)
- [mise 任务配置](https://mise.jdx.dev/tasks/task-configuration.html)

验收结果见 `docs/m00-validation.md`；不以文档调研替代运行验证。
