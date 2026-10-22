# CI 原生依赖缓存修复

2026-10-22，远端 `baf2157` 的 [Rust quality run 39752274856](https://github.com/MareDevi/Subveil/actions/runs/39752274856) 在 build 阶段报错：`could not find native static library sherpa-onnx-c-api`，不是测试失败。

日志确认恢复了旧 Rust 缓存，随后未运行 sherpa 原生下载。锁定 sherpa-onnx-sys 1.13.8 的构建脚本将预编译库置于 `target/sherpa-onnx-prebuilt`，仅监听环境变量，不监听原生文件存在性。rust-cache 会清理非 Cargo 构建目录，却可保留链接到该目录的 build-script 输出；恢复后 Cargo 错误复用这一输出。

CI 在恢复 Rust 缓存后仅清理 `sherpa-onnx-sys` 包的 Cargo 构建产物，强制重新执行上游原生库检查/下载和链接设置。保留其他依赖缓存；不修改 Cargo.lock、运行时功能、上游源码或本机模型。原生下载失败仍让构建失败，不跳过识别功能、测试或安装验证。

接口先经 Context7 resolve/query `/swatinem/rust-cache`，再核对锁定 sherpa 构建脚本。来源：[rust-cache 配置及清理](https://github.com/Swatinem/rust-cache)、[sherpa 构建脚本](https://docs.rs/crate/sherpa-onnx-sys/1.13.8/source/build.rs)。修复后的远端结果待本次提交触发后记录。


第一次仅清理 Cargo 包后，[run 39753051357](https://github.com/MareDevi/Subveil/actions/runs/39753051357) 证明构建脚本已重新编译，但仍缺少静态库。这排除了“只需让脚本重跑”的假设：rust-cache 清理文件后保留空的 `lib/` 目录，上游 `lib_dir.is_dir()` 因而误判已下载。修复补充删除 CI 自己的 `target/sherpa-onnx-prebuilt`，迫使重新获取完整原生依赖。删除仅在 runner 的构建目录，不影响模型、源代码或用户文件；不更改上游源码。完整远端验证继续跟踪。
