# M04 实施计划

日期：2026-10-20，Asia/Shanghai。

## 初始检查

- main 与 origin/main 同步；用户已有 .gitignore 修改，保留且不提交。
- Rust 1.99.1、rustfmt、Clippy 与 Linux 系统依赖检查通过。
- M03 统一字幕模型已具备 Final 翻译 Pending / Complete / Failed 与过期保护。
- M03 云端真实账号验收仍未完成，不据此宣称线上可用。

## 范围与方案

1. 官方 sherpa-onnx 1.13.8 安全 Rust 包，CPU streaming transducer；用户通过 TOML 清单导入 encoder、decoder、joiner、tokens、采样率、语种、来源、许可证及 SHA-256。后台校验并加载，原生错误不回退云端。
2. 独立 subveil-translate crate：Translator trait、OpenAI-compatible Chat Completions 与 DeepL HTTP，Final 驱动、有界队列、有限上下文、超时、取消、限流退避和错误显示。
3. GUI 保留现有设置布局，增加本地模型导入、翻译服务选择和独立文本上传授权。运行时锁定配置，停止会话撤销后台结果。
4. 既有语音服务自带译文优先；不对每个 Partial 重复调用翻译。
5. 核心测试无需模型、设备或付费接口；模型与日英真实语音验收单独记录。

不下载模型，不实现 GPU、本地翻译或 M05 模型管理。首次构建原生依赖会获取官方预编译运行库，可通过 SHERPA_ONNX_LIB_DIR 使用自行构建版本。
