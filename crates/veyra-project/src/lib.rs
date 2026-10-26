// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! Veyra项目：`veyra.toml`配置、源文件发现与输出写入
//!
//! 写入规则：
//!
//! - 生成文件内容变化时才写入，避免触发无意义的增量编译
//! - 开发者自有文件存在即跳过，从不覆盖
//! - `.veyra/manifest.json`记录上次生成的文件，不再生成的文件会被删除，
//!   删除前会确认文件仍带有Veyra的生成标记

mod config;
mod output;

pub use config::{CONFIG_FILE, Config, Lint, LintLevel, Project, ProjectSection};
pub use output::{WriteReport, write_outputs};
