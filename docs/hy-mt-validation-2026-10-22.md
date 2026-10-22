# HY-MT 本地适配与验证（2026-10-22）

## 模型与许可

用户已阅读完整许可，确认地区及用途符合条款，并明确授权约 1.13 GB 权重下载和本地验证。固定官方仓库 [tencent/HY-MT1.5-1.8B-GGUF](https://huggingface.co/tencent/HY-MT1.5-1.8B-GGUF/tree/265b2e615a7dc9b06c435dc878829ad99a512ba2)，revision `265b2e615a7dc9b06c435dc878829ad99a512ba2`。安装包同时校验权重、README 和完整 License.txt；不改变项目本身的许可证。

- `HY-MT1.5-1.8B-Q4_K_M.gguf`：1,133,080,512 字节，SHA-256 `4383ac0c3c8e476de98ff979c2a3f069f8c4fb385e7860cf2d28da896cc477c7`。
- `README.md`：8,088 字节，SHA-256 `a0bb039b004b8fa225ce1db982137ccc80c70cf4ebf122f519cf5ec48cb4cf98`。
- `License.txt`：16,270 字节，SHA-256 `d7d9db858500ac9073f4b5decef8e208454357226f535f65079ce4376047569f`。

许可不是 Apache/MIT，地域不包括欧盟、英国、韩国，另有用途等条款。应用展示[固定完整许可](https://huggingface.co/tencent/HY-MT1.5-1.8B-GGUF/blob/265b2e615a7dc9b06c435dc878829ad99a512ba2/License.txt)，点击安装/本机校验先弹原生确认；取消或确认失败不安装。确认不自动开始识别。导入自有清单仍由持有人保证许可与来源；不绕过 hash 校验。权重不打入程序 archive，不公开转发。

## 专用实现

新增独立 `hy_mt_translation` family，音频采样率为 0，与 Qwen 分开。模型清单一次解析、整文件校验后返回文件及 family，避免重复读取决定提示路径。ASR 拒绝把 HY-MT 当识别器，原生筛选、导入和恢复能识别其文字翻译身份。

使用权重内置 chat template，只有 user message、没有 system message。涉及中文时采用模型卡的中文翻译指令，其他中英日组合使用英文指令。历史和术语作为 JSON 引用数据；上下文超限先缩参考历史，不截当前句子或默默丢术语。引用与指令的分隔不构成 LLM 安全保证。

实测模型卡基础提示把中文“不要支付”改成日文“无需支付”，改变了命令含义。因此为两种指令增加通用忠实性约束：保留数字、货币、否定和命令语气，不扩写或补全残句。保留严格质量检查，未通过时不跳过该样本。此改动通过本次固定样本，不能保证所有否定句都正确。

采样沿用模型卡建议：top-k 20、top-p 0.6、temperature 0.7、repeat penalty 1.05；应用另固定 seed 42、重复窗口 64 tokens。Qwen 仍用原有提示与 greedy。输入最多 1536 tokens、上下文 2048、输出最多 256；超限报错，不显示截断译文。沿用分 batch/token 取消、请求期限、Final 优先和显式 CPU/Vulkan 选择，不回退其他后端。

API 先经 Context7 resolve/query `utilityai/llama-cpp-rs`，再核对锁定 llama-cpp-2 0.1.160 的 sampling/vocab 源码；实际提示来源为本固定 revision 模型卡。没有增加运行时依赖。

## 真实推理

`mise run translate:probe MANIFEST cpu|gpu` 使用公开固定文字，汇报长度、耗时及谓词，不打印译文。九项包含 en→zh、ja→zh 天气句、日语残句、六个翻译方向的数字/否定，以及附源文历史的术语干预。检查只覆盖指定含义、数字、货币及术语；不是 BLEU/COMET 或人工质量评分。英文允许美元符号和弯引号，日文拒绝把禁止变成“无需”。

最终 CPU 和 NVIDIA Vulkan 均为 9/9；CPU 单项 574–951 ms（天气句 671/681 ms），NVIDIA RTX 4060 Laptop Vulkan 单项 80–140 ms。样本很少，且与其他验证重叠，不代表 p95、端到端延迟或持续负载表现。

固定长请求取消后，在同一 helper 继续完成带上下文/术语的数字否定句。最终 CPU 取消确认 447 ms、后续请求 815 ms；NVIDIA Vulkan 取消 43 ms、后续 123 ms；Intel Arc MTL Vulkan 取消 191 ms、后续 711 ms。分别实际加载和推理，不用设备枚举替代。无法抢占单次原生 decode，取消在 batch/token 边界响应。

SenseVoice CPU 与 HY-MT NVIDIA Vulkan 的公开日语 WAV 离线链路：1 Final、1 完成译文、pending 0，无丢音频/段/事件；ASR 原生 149 ms / 6380 ms 分段音频，第一源文状态 6840 ms、第一中文状态 7240 ms，段尾到译文状态 540 ms。按真实节奏回放，不采集系统音频、不上传；时间止于软件状态，不含物理屏幕。此运行使用基础提示；通用忠实性提示的六方向验证单独记录，不能把早期端到端耗时当最终提示性能。

模型已用应用同一安装器实际校验并发布本地 manifest。生产原生界面回归覆盖取消许可确认不下载。仍需更多人工参考、自然对话/背景音乐、ASR+MT+GUI+游戏/视频并行资源、持续运行和物理屏幕验收。
