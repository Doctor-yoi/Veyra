// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi
//
// 本程序是自由软件：你可以在GNU Affero通用公共许可证第3版及Commons Clause
// 附加条件的约束下再发布或修改本程序，但不得出售本程序或以其为主要价值的服务
// 本程序不提供任何担保，详见仓库根目录的LICENSE文件
// 使用本程序生成的代码不受上述许可证约束

//! veyra，Veyra UI DSL编译器的命令行入口
//!
//! Veyra把`.vey`中声明的界面、状态、绑定与命令编译为各平台的原生代码，
//! 生成结果只依赖平台SDK，不需要任何Veyra运行时
//!
//! 编译流程：
//!
//! ```text
//! veyra.toml ──┐
//! ui/*.vey ────┼─> 解析 ─> 按平台裁剪与语义检查 ─> IR ─┬─> SwiftUI后端 ─> Swift与String Catalog
//! locales/ ────┘                                        └─> Compose后端 ─> Kotlin与strings.xml
//! ```
//!
//! 各阶段的实现分布在以下crate中：
//!
//! - `veyra-syntax`：词法分析与语法树
//! - `veyra-sema`：条件编译裁剪、类型检查与MVVM规则
//! - `veyra-ir`：后端共享的语义中间表示
//! - `veyra-l10n`：翻译文件同步与校验
//! - `veyra-backend-*`：平台代码生成
//! - `veyra-project`：配置解析与输出写入

mod driver;
mod init;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "veyra",
    version,
    about = "将Veyra UI DSL编译为SwiftUI与Jetpack Compose原生代码"
)]
struct Cli {
    /// 在指定目录中运行，默认为当前目录
    #[arg(short = 'C', long = "directory", global = true, value_name = "DIR")]
    dir: Option<PathBuf>,

    /// 诊断信息的着色方式
    #[arg(long, global = true, value_enum, default_value_t = Color::Auto)]
    color: Color,

    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum Color {
    Auto,
    Always,
    Never,
}

#[derive(Subcommand)]
enum Command {
    /// 创建新的Veyra项目
    Init {
        /// 项目目录，默认为当前目录
        path: Option<PathBuf>,
        /// 项目名称，默认取目录名
        #[arg(long)]
        name: Option<String>,
    },
    /// 检查源码，不写入任何文件
    Check(BuildArgs),
    /// 检查源码并生成代码
    Build(BuildArgs),
    /// 管理翻译文件
    L10n {
        #[command(subcommand)]
        command: L10nCommand,
    },
}

#[derive(Args, Clone, Default)]
pub(crate) struct BuildArgs {
    /// 只处理指定的后端，可重复
    #[arg(long, value_name = "NAME")]
    backend: Vec<String>,
    /// 覆盖veyra.toml中的flag，例如`--flag premium`或`--flag premium=false`
    #[arg(long = "flag", value_name = "NAME[=BOOL]")]
    flags: Vec<String>,
}

#[derive(Subcommand)]
enum L10nCommand {
    /// 显示各语言的翻译进度
    Status,
    /// 删除源码中已不存在的key
    Prune,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let color = match cli.color {
        Color::Auto => std::io::stderr().is_terminal(),
        Color::Always => true,
        Color::Never => false,
    };
    let dir = cli.dir.unwrap_or_else(|| PathBuf::from("."));
    let ok = match cli.command {
        Command::Init { path, name } => {
            init::run(&path.map(|p| dir.join(p)).unwrap_or(dir), name, color)
        }
        Command::Check(args) => driver::check(&dir, &args, color),
        Command::Build(args) => driver::build(&dir, &args, color),
        Command::L10n {
            command: L10nCommand::Status,
        } => driver::l10n_status(&dir, color),
        Command::L10n {
            command: L10nCommand::Prune,
        } => driver::l10n_prune(&dir, color),
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
