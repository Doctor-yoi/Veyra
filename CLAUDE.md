# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

Veyra是一门静态类型、强制MVVM的UI声明语言（`.vey`），编译器用Rust编写，把同一份源码生成SwiftUI（iOS 17+）与Jetpack Compose代码，生成结果不依赖任何Veyra运行时。语言特性的完整说明见`README.zh-CN.md`/`README.md`，`examples/schedule`是覆盖大部分特性的完整示例工程。

## 常用命令

CI（`.github/workflows/ci.yml`）依次执行以下检查，提交前应全部通过：

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -q -p veyra-cli -- -C examples/schedule fmt --check
```

- 单个crate的测试：`cargo test -p veyra-sema`；单个测试：`cargo test -p veyra-backend-swiftui --test generate generates_schedule`
- 在示例工程上运行编译器：`cargo run -q -p veyra-cli -- -C examples/schedule build`（或`check`、`l10n status`）
- Swift输出类型检查（需macOS + Xcode 16+）：`scripts/check-swift.sh`

### 快照测试（insta）

大部分测试是insta快照。修改诊断信息或代码生成后，快照会失败并生成`*.snap.new`。确认差异符合预期后用`INSTA_UPDATE=always cargo test -p <crate>`（或安装`cargo-insta`后`cargo insta review`）更新，并把更新后的`.snap`一并提交。

- `crates/veyra-sema/tests/ui/*.vey`：每个文件分别按iOS与Android检查，诊断渲染结果写入`ui__<文件名>.snap`；两平台输出不同时快照内用`[ios]`/`[android]`分段。新增语义规则时在这里加`err_*.vey`用例
- `crates/veyra-backend-*/tests/generate.rs`：用`fixtures.vey`、`storage.vey`生成每个文件并逐一快照，快照名为路径中`/`替换为`__`。设置`VEYRA_DUMP=<目录>`可把生成文件写到磁盘查看
- `crates/veyra-syntax/tests`：解析与格式化快照；`crates/veyra-cli/tests/cli.rs`：在临时目录中运行`veyra`二进制的端到端测试

## 架构

编译流水线（由`crates/veyra-cli/src/driver.rs`编排）：

1. `veyra-project`：读取`veyra.toml`、发现`sources`下的源文件
2. `veyra-syntax`：词法、语法分析得到AST（`ast.rs`），`fmt.rs`是保留注释的格式化器
3. driver解析`import`：路径相对于当前文件，未列入`sources`的被import文件会自动加载
4. `veyra-sema::analyze`：**对每个启用的后端各执行一次**。按`Target`（平台+flags）裁剪`#if`条件编译（`cfg.rs`），然后做名称解析、按import控制可见性、类型检查与MVVM规则检查，最后降级为`veyra_ir::Program`。多个平台报告的相同诊断由driver去重；未使用的import在所有平台检查完后统一报告
5. `veyra-l10n`：合并各平台的`LocString`，同步`locales/<locale>.toml`（新增key写空占位、校验占位符），构建`Catalog`
6. 后端（`veyra-backend-swiftui`、`veyra-backend-compose`）实现`veyra_codegen::Backend`，输入已检查的IR与Catalog，返回`OutputFile`列表
7. `veyra-project::write_outputs`：写入文件并维护`.veyra/manifest.json`，删除上次生成但本次不再生成、且仍带`GENERATED_NOTICE`标记的文件

关键约定：

- **IR是前后端之间的契约**（`veyra-ir`）：条件编译已裁剪（只剩需要原生保留的`debug`），名称已解析、表达式都带类型，读取/双向绑定/命令分发已是不同的节点。后端只做到平台惯用写法的映射，不做语义检查；新的语义规则应放在`veyra-sema`
- 内置组件与修饰符的参数表在`veyra-sema/src/catalog.rs`，平台专属组件（`ios.*`、`android.*`）也在此登记，新增内置组件需要同时修改catalog、IR与两个后端
- 新增平台：实现`Backend` trait，并在`driver.rs`的`registry()`中登记；`Platform`枚举中已预留`Harmony`但尚无后端
- 文件归属：`Ownership::Generated`（`.generated.swift`/`.generated.kt`，每次覆盖）与`Ownership::UserOwned`（ViewModel骨架，仅首次创建）。`examples/schedule`中的ViewModel与App入口是手写的，生成文件已被`.gitignore`忽略
- 诊断码按阶段分段：`V0xxx`语法、`V1xxx`–`V6xxx`语义及本地化等、`V9xxx`项目/CLI/后端配置。新增诊断前先grep已有编号避免冲突
- 后端配置段名即`Backend::name()`（`[swiftui]`、`[compose]`），整段`toml::Table`原样传给后端（如`di = "factory"`/`di = "hilt"`、`database_version`）

## 代码风格

- 每个`.rs`文件以SPDX许可证头开头（`AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0`与`Copyright (C) 2026 Doctor-yoi`），新文件照抄
- 代码注释、文档注释与诊断信息使用中文，中文与英文/代码之间不加空格（如``未声明的flag`{name}` ``），句末不加句号
- 工作区lint：禁止`unsafe`，clippy警告在CI中视为错误；rustfmt `max_width = 100`
- 提交信息格式：`type[scope]: 中文描述`，例如`feat[sema]: 支持plural与format`、`fix[swiftui]: ...`；scope通常是crate简称（`syntax`、`sema`、`codegen`、`swiftui`、`compose`、`cli`）或`examples`、`vscode`
