# 云配置、模型目录与增量翻译验收

日期：2026-10-21，Asia/Shanghai。

## 已实现

- 所有现有语音服务的凭据、端点、业务空间与 Azure 部署名均可在原生应用填写。独立 OpenAI-compatible / DeepL 的端点、密钥与模型也可填写。GUI 使用显式快照，无进程环境写入、无凭据环境回退。
- 密钥输入遮蔽；可保存、读取、清除系统 Secret Service 配置。最长 15 秒；输入输出最多 64 KiB，后台处理，不将凭据写入明文设置文件。失败后内存配置仍可使用；清除操作即使系统删除失败也会清空内存，并说明系统删除未确认。
- OpenAI、Gemini、Deepgram 查询并筛选已适配的语音模型；OpenAI-compatible 文字服务支持同源 `/models` 发现。其他服务明确显示固定协议或手填部署，没有伪造成功的模型枚举。列表不等于账号推理授权。
- 可安装模型从 5 项扩为 7 项：新增 Whisper Small Q5_1 与 Qwen2.5 0.5B Instruct Q4_K_M，固定官方 revision、大小与 SHA-256。其他新运行时列研究候选，不能安装或启动。
- 原生 Zipformer ASR 保留状态复用。稳定 Partial 翻译默认关闭，可与 Final-only 对照。请求绑定 session、segment、请求 revision 和实际前缀；兼容的尾部增长不取消稳定前缀，改写、较新请求、Final、停止与段淘汰会拒绝旧结果。临时译文明示，Final 重新翻译；排队修订合并，HTTP 可取消过时推理，本地推理结果再检查有效性。

## 自动检查

最终 `mise run check` 通过：rustfmt、workspace 构建、全部目标严格 Clippy、111 个测试；0 失败、0 忽略。单次测试命令由 90 秒 timeout 包裹。测试覆盖显式凭据隔离及脱敏、所有云协议初始化、模型发现过滤、Unicode 词界、连续增长不饥饿、提交限频、临时前缀与 Final 关联、同 revision 的 Final 晋升、源文改写、旧会话拒绝、模型安装与现有回归。

初次在沙箱内运行时，3 个模型下载 HTTP 测试因 loopback 监听返回 `Operation not permitted`；随后在允许本机监听的环境运行相同 `mise run check` 全部通过，未跳过测试或更改断言。Cargo.lock 只增加应用对已有 tokio 的直接依赖，不升级依赖版本。

## 真实本地推理

输入为已有英文 Zipformer 20M 模型仓库的公开 `test_wavs/0.wav`，固定 revision `d42f2d9f7ca24806fb667456a18a9f1b60f70d16`；WAV SHA-256：`6bc58a4efdf20daac252b6b1502632601a71efe0308f6757dc1eda34891a7e4f`。直接按 20 ms PCM 块实时节奏送入队列，未采集用户系统音频、未上传或打印任何文字。翻译为已有 Qwen2.5 1.5B Q4_K_M，ASR 与翻译均为 CPU。

| 本次样本 | Final-only | 稳定 Partial 模式 |
| --- | ---: | ---: |
| 首次 Partial 事件 | 未统计 | 2340 ms |
| 首次有效临时中文 | 不提交 | 5220 ms |
| Final 原文 | 7460 ms | 7460 ms |
| 最终中文 | 8720 ms | 8810 ms |
| 临时提交 / 接收 / 拒绝 / 失败 | 0 / 0 / 0 / 0 | 2 / 2 / 0 / 0 |
| 最终译文 / 未完成 Final | 1 / 0 | 1 / 0 |
| ASR 丢音频 / 丢事件 / 重置 | 0 / 0 / 0 | 0 / 0 / 0 |

临时结果必须为成功响应且包含中文字符才计入首次有效临时中文。此样本中，临时中文比对照组最终中文早 3500 ms；不是多音频统计、准确率评估或用户屏幕实际呈现延迟。一次早期试验暴露严格整段 revision 匹配会丢弃尾部增长期间的译文，已以不可变前缀请求关联修复，并增加回归测试。

另用公开 SenseVoice 英文短 WAV 验证增量开启时的保守回退：没有稳定前缀提交，仍成功完成一次 Final 翻译。不能保证每段音频都会提前翻译。上述各次运行均少于 90 秒，没有进行 30 分钟长时测试。

复现：

```sh
mise run offline:probe -- ASR_MANIFEST TRANSLATION_MANIFEST WAV en cpu
mise run offline:probe -- ASR_MANIFEST TRANSLATION_MANIFEST WAV en cpu --incremental
```

## 原生窗口与未验证项

在当前 Hyprland / Wayland 实际启动普通控制窗口，使用临时配置路径；通过 Ctrl+2 切到「语音服务」并检查截图。服务选择、语言、上传入口、文字翻译、增量开关和滚动区可见，没有启动识别。测试窗口由 60 / 90 秒 timeout 结束。未提交截图或日志。

这只确认已查看页面的布局及导航，不代表所有凭据输入、下拉选择、错误状态、焦点恢复或系统密钥环保存/读取/清除均通过完整人工交互验收。没有使用真实云端凭据、没有付费推理调用；模型目录真实账号响应、账号地域权限、云服务质量与计费均未验收。

新增两个权重的发布元数据已核对，但本轮未下载权重或实测其速度、翻译质量、CPU / CUDA / Vulkan 推理。已有 Zipformer + Qwen1.5B 的 CPU 样本实测不代表新增模型或 GPU 验收。

Fujie 日语 Streaming Zipformer 的模型卡可读，但 Hugging Face API 返回 401，缺少可信的文件校验元数据；未加入可安装目录，未实现其假名标记规范化，也未声称日语原生流式推理已可用。Vosk、NeMo、SeamlessStreaming 等仍为研究候选。HY-MT 于 2026-10-22 独立接入并实测，见 [HY-MT 验证](hy-mt-validation-2026-10-22.md)。

调研来源、厂商差异与配置方法见 [云服务配置](cloud-services.md)、[计划与官方资料](cloud-streaming-plan.md)、[本地链路](local-asr-and-translation.md)。
