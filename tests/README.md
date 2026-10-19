# 项目测试

从仓库根目录运行完整默认测试：

```sh
cargo test --workspace --locked
```

这条命令包含应用、CLI 和所有自有 workspace crate 的单元、集成与文档测试。
根目录单独运行 `cargo test` 只选择 `pedal` 主包；vendored `egui` / `egui_dock`
不是 workspace 成员，不包含在此命令中。需要修改 vendored 代码时另行验证对应库。

首次构建需要 Rust stable、平台编译工具和下载 Cargo.lock 中的依赖。Linux 还需要
`libxkbcommon-dev libwayland-dev libwayland-egl-backend-dev libasound2-dev`。
默认测试不需要 HarmonyOS SDK、Node、图形窗口或 `test_examples/`。
CI 从 checkout 执行同一命令，避免依赖开发者电脑上被忽略的文件。

## 目录与责任

| 位置 | 内容 | 默认执行 | 是否提交 |
| --- | --- | --- | --- |
| `src/**`、`crates/*/src/**` 中的 `#[cfg(test)]` | 私有实现的单元测试 | 是 | 是 |
| `tests/cli.rs` | 正式 CLI 的进程级测试，临时构造 ABC/HAP | 是 | 是 |
| `crates/tabookit/tests/` | 宏的集成测试 | 是 | 是 |
| `crates/pedal-abc-bridge/tests/regression/` | 合成字节码与编译器小样本回归，统一为 `regression` target | 是 | 是 |
| `crates/pedal-abc-bridge/tests/common/` | 必需小样本的读取辅助代码，供单元和集成测试复用 | 辅助模块 | 是 |
| `crates/pedal-abc-bridge/tests/fixtures/` | 小型 ABC、对应 TS/ETS、再生成脚本、可选 Node 对照脚本 | Rust 测试读取 ABC | 是 |
| `crates/pedal-abc-bridge/tests/corpus/` | 大型真实应用与系统框架语料回归，统一为 `corpus` target | 否，显示 `ignored` | 测试代码提交 |
| `test_examples/` | 大型 ABC/HAP/HSP 和本地 SDK 探针工程 | 仅显式运行 | 否 |

`regression/main.rs` 和 `corpus/main.rs` 是 Cargo 自动发现的两个测试入口，
具体用例按主题拆成模块；不要再把依赖大语料的测试混入默认测试或源码单元测试。

## 常用命令

```sh
# 只测字节码库（单元 + regression；corpus 显示 ignored）
cargo test -p pedal-abc-bridge --locked

# 某一组小样本回归；筛选器是模块名/测试名的一部分
cargo test -p pedal-abc-bridge --test regression iterator_recovery

# 只测正式 CLI
cargo test -p pedal --test cli

# 查看可选语料用例，不读取任何语料
cargo test -p pedal-abc-bridge --test corpus -- --list

# 显式执行全部语料测试
cargo test -p pedal-abc-bridge --test corpus -- --ignored

# 只准备了下载包时，仅执行对应的语料模块
cargo test -p pedal-abc-bridge --test corpus download:: -- --ignored
```

`corpus` 的所有测试均带 `#[ignore = "..."]`，即使本地存在语料，默认命令也不会加载。
显式运行的用例缺少所需文件时会失败并显示完整路径，不会以“通过”掩盖未执行的检查。
完整语料集相对仓库根目录的路径如下；文件须自行准备，不会自动下载：

```text
test_examples/
├── AppGallery/
│   ├── entry.hap
│   ├── service.hap
│   └── @hw-ae-business+download-install-default.hsp
├── Hinote/
│   └── HiNote.hap
└── system_framework/
    ├── crypto_framework_ets.abc
    └── camera_framework_taihe_abc.abc
```

这些断言针对特定真实应用版本，不保证任意同名包都满足断言；新增或更换语料时应说明
版本和回归场景，并同步检查测试预期。

## 样本维护

小样本缺失、损坏或反编译结果异常都必须让默认测试失败。不要把必需文件加入
`.gitignore`，也不要用 `if !exists() { return; }` 或 `Option` 跳过小样本检查。
样本源码、ABC 与再生成方法应一起提交；体积大的真实包放入 `test_examples/`。

目前有 22 个小型 ABC，总计约 80 KiB。详细场景和生成命令见
[fixtures 说明](../crates/pedal-abc-bridge/tests/fixtures/README.md) 及各场景子目录 README。
仅再生成 ABC 时需要 OpenHarmony ETS SDK；Node 行为对照还需要 Node 和 TypeScript
编译器，属于开发者可选验证，不属于默认 Cargo/CI 测试。

新增默认测试优先使用内存合成数据或小样本，文件输出使用临时目录；手动导出和审计
产物放到已忽略的 `target/`。`scripts/gen_isa.py`、`scripts/gen_isa_legacy.py`
是随仓库提供的 ISA 开发工具，普通测试不需要运行它们。
