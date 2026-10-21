# 原生界面重构（2026-10-20）

## 第二轮计划（2026-10-21）

主任务：查看双语字幕并控制会话。Session 继续拥有业务状态，ControlView 只拥有页面、焦点、折叠状态和各页滚动位置。使用标准 Sidebar、SidebarMenuItem、Button、Select、Switch、Progress、StatusBar；采用主题色、相对间距和 Lucide 图标。

将四页 TabBar 改为五个稳定侧栏入口：实时字幕、语音服务、模型库、字幕样式、运行诊断。保留 Ctrl+1–4 原有页面映射，新增 Ctrl+5 模型库与 Ctrl+B 折叠侧栏。默认实时页，`--models` 进入模型库。主区提供就近的服务设置入口和完整原文/译文，模型库承载安装、来源、许可证、完整性校验及取消。固定状态栏显示真实会话状态；长状态信息单独换行。

保留原生窗口装饰、字幕浮窗及恢复窗口。表单使用一致的标签宽度与内容轨道，窄窗口允许控件换行。浅色、深色、最小尺寸、键盘导航、离线演示和关闭恢复均纳入验收；未执行项目明确记录。

### 第二轮实施与验收

已实现五页 Sidebar 导航及图标折叠、固定工具栏、StatusBar、独立模型库、真实下载 Progress、离线 Switch、对齐表单、样式分区和双语历史。导航切换将焦点交回工作区，避免旧页面控件继续持有焦点。控制页面拆为 `ui/live.rs`、`services.rs`、`model_library.rs`、`appearance.rs`、`diagnostics.rs`，会话与推理仍由原 Session 管理。模型安装与设置在恢复、识别、导入或另一类模型下载期间禁用；取消入口仍可用。保留 gpui-kit 0.8.0 与全部原有业务依赖。

最终 `mise run check` 退出码 0：格式、锁定构建、严格 Clippy、93 项测试通过。下载器测试使用本机 HTTP fixture；沙箱禁用 socket 导致的 `Operation not permitted` 已在沙箱外复跑通过。`git diff --check` 通过。

真实 Hyprland / Wayland 限时离线演示中，逐页截图检查了实时、服务、模型库、样式、诊断。默认 1120×800 与最小 760×560 逻辑尺寸的工具栏、导航和状态栏可见，内容区独立滚动；明暗主题与 Ctrl+B 图标折叠均正常。按截图像素检查：侧栏/工具栏头部共享边界，内容标题、预览与历史使用同一左侧对齐线；表单标签与控件保持两条固定对齐线。未验证其他显示比例和应用 rem 缩放。

Ctrl+1–5、Ctrl+Shift+L、Ctrl+S 与恢复窗口 Enter 已实测。Hyprland Lua `send_shortcut` 的数字键用 `code:10` 至 `code:14` 注入；直接传数字字符串不能作为有效导航证据。保存写入 `/tmp/subveil-sidebar-final.toml`。关闭控制窗口后出现恢复窗口，Enter 恢复后 PID 29778 不变。截图、日志和测试配置仅保存 `/tmp`。最终 GUI 日志未出现 panic；系统现有 LSFG Vulkan 隐式层仍有加载警告，未阻止窗口渲染，没有更改系统驱动。

本轮未重新执行真实模型下载、GPU 推理或付费云端调用；沿用现有业务测试与先前推理验收，不将本轮 UI 验收描述为新的推理证明。下载进度的真实网络交互、所有 Select 的方向键/Escape 路径、屏幕阅读器、多显示比例、其他 compositor/平台与长期稳定性仍待验证。当前页面、侧栏折叠和明暗主题仅在窗口/进程内保存，不持久化。

## 调研与约束

已通过 Context7 查询 `/longbridge/gpui-kit`，并读取 GPUI Kit 的 Design Guides、Coding Guides 及组件发布源码。网站当前展示 0.7.3；crates.io 实际提供 `gpui-kit 0.8.0`，其 GPUI 为 `gpui-pre =0.3.9`，Component 为 0.8.0，与现有工程兼容。以发布包源码核对 API，不按网站示例降级。

应用改为仅依赖 `gpui-kit =0.8.0`，通过 facade 使用 GPUI、Component 和 assets；保留原生 Wayland Layer Shell 实现。不开启 GPUI Fast，不混用其他 GPUI 来源，不增加 WebView。

来源：

- https://gpui-kit.com/
- https://gpui-kit.com/docs/design-guides/
- https://gpui-kit.com/docs/coding-guides/
- https://docs.rs/crate/gpui-kit/0.8.0/source/
- https://docs.rs/crate/gpui-component/0.8.0/source/

## 任务与结构

主要任务是选择识别服务、显式开始会话、查看原文与服务译文、调整字幕浮窗。将原有长页拆成四个工作区：实时字幕、语音服务、字幕样式、运行诊断。导航与窗口工具栏保持固定，内容区独立滚动。实时字幕拥有主要预览与最近记录；服务页展示上传目标、语言与明确上传按钮；诊断信息不占据主任务区域。

业务状态继续由 Session 持有，关闭或重建控制窗口不丢失会话。工作区选择、焦点和滚动位置由 ControlView 持有。现有服务、采集、演示、保存与恢复入口全部保留，不引入新的网络行为。

使用标准 Button、TabBar、Switch 和滚动组件，依赖其键盘、焦点和禁用行为。提供工作区快捷键以及浮窗、保存快捷键；切换工作区不触发上传。控件按业务状态禁用，保存中禁止重复操作。

界面颜色使用主题语义 token，间距和字体使用 rem 比例。字幕本身的颜色、字号和背景透明度属于用户配置数据，继续按其配置渲染；窗口尺寸与 Layer Shell 几何是物理像素边界。支持明暗主题切换。保留系统窗口装饰。

## 验证计划

执行 `mise run check`，检查 facade 没有引入重复 GPUI 版本。在真实图形会话启动离线演示并检查运行状态；如工具允许，补充窗口截图及交互验证。没有实际执行的键盘、缩放、主题和 compositor 场景须明确列为未验证。单次测试不超过 90 秒，不使用付费接口。


## 实施与验收

已完成四工作区、固定工具栏/状态栏、服务下拉框、语种与运行状态禁用、独立音频诊断、字幕样式表单、鼠标穿透 Switch、最近八段原文、明暗主题、恢复入口。字幕浮窗与样式预览共用渲染；原来的采集和 ASR 生命周期保持不变。

快捷键：`Ctrl+1–4` 切换工作区；`Ctrl+Shift+O` 切换浮窗；`Ctrl+S` 保存样式；`Ctrl+Shift+L` 切换明暗主题；恢复入口的 `Enter` 恢复控制窗口。主题及当前工作区不持久化。

自动检查：最终代码的 `mise run check` 退出码 0，格式检查、锁定构建、严格 Clippy 与 62 项测试全部通过。`git diff --check` 通过；`cargo tree --locked -i gpui-pre` 确认仅使用 0.3.9。没有增加业务依赖或模型后端，没有调用付费 API。

真实 Hyprland / Wayland 运行：

- 离线演示显示英文、日文、中文及长句，实时预览与最近记录正常渲染。
- 通过 compositor 向 Subveil 自身窗口发送 Ctrl+2/3/4，实际检查服务、样式与诊断页截图。
- Ctrl+Shift+L 切换深色，检查默认尺寸与 760×560 逻辑像素窗口。工具栏、导航和状态栏保持可见，长表单在内容区裁切并由独立滚动区域承载。
- Ctrl+S 写入 `/tmp/subveil-ui-review.toml`，没有改写真实用户配置。
- 关闭主窗口后恢复入口出现；发送 Enter 后控制窗口重新出现，PID 一致，演示仍在运行。
- Tab 路径出现可见焦点，服务下拉列表可见。完整的方向键选择、Escape 焦点恢复、鼠标控件路径未形成完整验收证据，不据此宣称全部键盘行为通过。
- 最终运行日志未发现 panic 或缺少图标资源错误。每次运行限时 75 或 90 秒；截图和日志仅保存于 `/tmp`，没有提交。

未验证：其他 compositor、平台、显示缩放/应用 rem 缩放组合、全部控件的鼠标与键盘路径、屏幕阅读器、实际云端语音与长期稳定性。主题仅按当前进程切换；该限制已在 README 中说明。保留此前 M03 的真实服务验收限制。
