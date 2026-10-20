# 原生界面重构（2026-10-20）

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
