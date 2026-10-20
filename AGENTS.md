# Subveil 开发规范

## 工作范围与顺序

- 项目是 Rust + GPUI 原生实时字幕翻译应用；Linux / Wayland / PipeWire 优先，保留跨平台边界。
- 原始需求见 `docs/development-plan.md`。按 M00 → M01 → M02 → M03 → M04 推进；当前仅执行 M00。
- 每个里程碑开始先检查文件、Git 状态、工具链和依赖，列出简短清单。先调研和更新规范，再开发。
- M00 仅建立最小可编译 Workspace、真实控制窗口、基础日志、文档与质量流程。不得提前实现音频、ASR、翻译或创建空壳 crate。
- 不覆盖用户工作，不声称未实现或未经实测的能力可用。编译失败先修复，禁止继续堆积功能。

## 环境与依赖

- 统一通过 `mise.toml` 管理工具版本、环境与任务。所有开发和 CI 命令使用 `mise run <task>` 或 `mise exec -- <command>`。
- 固定 Rust Stable 的具体版本，安装 rustfmt、Clippy。不要另建与 mise 冲突的 rust-toolchain 文件。
- 原生系统库由操作系统包管理器安装；mise 任务检查依赖，文档记录安装方式，不静默修改系统。
- 提交 Cargo.lock；构建和检查使用 `--locked`。升级依赖须核对兼容性并重新验证。
- 查询库、框架、SDK、API、CLI、云服务时，必须先用 Context7 `resolve-library-id`，再 `query-docs`。用户给出精确 ID 时可跳过 resolve。
- Context7 信息不完整时核对官方文档、发布元数据和实际源码，记录来源与日期；不得凭记忆编造 API 或版本。
- GPUI、平台 crate 和 gpui-component 必须使用兼容版本。当前组件库发布版使用 `gpui-pre` 快照；不得混入不同来源的 GPUI 类型。
- 优先用发布包和上游能力，不立即 fork GPUI。研究结论与限制写入 `docs/technical-research.md`。

## 架构与代码

- 只使用 Rust + GPUI；禁止 Electron、Tauri、WebView。按实际边界拆 crate，不提前创建目标目录树。
- GPUI 主线程仅处理界面；音频回调不得推理、联网、等待 UI 或阻塞。后续通过有界队列连接采集、ASR、翻译、UI。
- 队列必须定义过载策略，避免无限缓存和延迟累积。平台差异集中在适配层；不创建伪 Windows/macOS 实现。
- 运行时外部错误使用 Result，禁止随意 unwrap/expect、假成功、静默忽略不支持能力。避免不必要的 unsafe、全局可变状态和依赖。
- 字幕按 Session ID、Segment ID、revision 关联。Partial 修订同一字幕；过期或乱序翻译不得覆盖新状态。
- 区分真实 streaming ASR 与离线分块识别。采样率由模型能力决定，不能统一硬编码为 16 kHz。
- Layer Shell、普通 Wayland 窗口、X11 置顶是不同能力。透明不等于鼠标穿透；不抢焦点、输入区域、多屏、全屏场景必须分别验证。
- 不承诺所有 compositor 支持 Layer Shell 或所有独占全屏场景均显示字幕。

## 隐私

- 设置遵循 XDG 目录规范。优先系统凭据存储，禁止提交 API Key。
- 默认日志不包含音频、完整识别文本和密钥。云端传输须明确展示并获得用户选择，禁止静默上传或回退云端。
- 模型下载必须展示来源、许可证并校验完整性。

## 验证与交付

- 完成代码后执行 `mise run check`：格式检查、构建、严格 Clippy、测试。各任务可单独运行。
- 核心测试不能依赖真实设备或付费 API。后续重点覆盖缓冲边界、重采样、修订、乱序返回、取消、超时、设备切换和长期运行。
- 有图形环境时实际运行最小窗口；没有条件时明确记录未验证项。GUI 启动日志不等同于完整交互验收。
- 架构、版本和行为变化同步更新文档。每个里程碑汇报已完成、测试结果、已知问题、下一步。

## Git 与提交

- 必须使用 Conventional Commits：`type(scope): description`，例如 `docs: record M00 engineering rules`、`feat(app): add native control window`。
- 小步、勤提交：调研规范、工程基础、窗口实现、验证文档分别形成可审查提交。禁止为凑提交拆出已知损坏状态。
- 提交前检查 diff 与暂存区；禁止提交密钥、日志、target、下载模型、临时截图。提交当前任务明确修改的文件。
- 不自动推送、发布或更改许可证归属；许可证选择需项目所有者明确决定。

## 本机输出规范

遵循 `/home/maredevi/.config/codex/RTK.md`：命令输出可能已压缩，视为完整结果；仅在输出不可用时使用 `rtk proxy` 重试。
