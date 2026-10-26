// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! Veyra语义检查
//!
//! 对每个目标平台单独执行一次：
//!
//! 1. 按平台与flag裁剪条件编译分支
//! 2. 收集全局声明并解析类型
//! 3. 检查表达式类型与MVVM规则（只读状态、绑定、命令分发、纯表达式）
//! 4. 收集本地化文案
//! 5. 降级为[`veyra_ir::Program`]
//!
//! 即使存在错误也会返回尽可能完整的IR，调用方应先检查诊断再使用IR

mod catalog;
mod cfg;
mod decls;
mod expr;
mod screen;
mod view;

use indexmap::IndexMap;
use veyra_diagnostics::{Diagnostic, Diagnostics, Span};
use veyra_ir::{EnumDef, LocString, ModelDef, Platform, Program, RepositoryDef};
use veyra_syntax::ast;

use crate::cfg::Cfg;

/// 一次检查的目标
#[derive(Clone, Debug)]
pub struct Target {
    pub platform: Platform,
    /// veyra.toml中声明的自定义flag及取值
    pub flags: IndexMap<String, bool>,
}

impl Target {
    pub fn new(platform: Platform) -> Self {
        Self {
            platform,
            flags: IndexMap::new(),
        }
    }
}

/// 检查结果
#[derive(Debug)]
pub struct Analysis {
    pub program: Program,
    pub diagnostics: Diagnostics,
}

/// 对一组源文件执行语义检查
pub fn analyze(files: &[ast::File], target: &Target) -> Analysis {
    let mut checker = Checker::new(target);
    let program = checker.run(files);
    Analysis {
        program,
        diagnostics: checker.diags.sorted(),
    }
}

/// 全局声明
#[derive(Clone, Copy)]
pub(crate) enum Global<'a> {
    Enum(&'a ast::EnumDecl),
    Model(&'a ast::ModelDecl),
    Repository(&'a ast::RepositoryDecl),
    Screen(&'a ast::ScreenDecl),
    Unsupported,
}

pub(crate) struct Checker<'a> {
    pub cfg: Cfg<'a>,
    pub diags: Diagnostics,
    pub globals: IndexMap<String, (Global<'a>, Span)>,
    pub enums: IndexMap<String, EnumDef>,
    pub models: IndexMap<String, ModelDef>,
    pub repositories: IndexMap<String, RepositoryDef>,
    pub strings: IndexMap<String, (LocString, Span)>,
}

impl<'a> Checker<'a> {
    fn new(target: &'a Target) -> Self {
        Self {
            cfg: Cfg::new(target),
            diags: Diagnostics::new(),
            globals: IndexMap::new(),
            enums: IndexMap::new(),
            models: IndexMap::new(),
            repositories: IndexMap::new(),
            strings: IndexMap::new(),
        }
    }

    pub fn error(
        &mut self,
        code: &'static str,
        message: impl Into<String>,
        span: Span,
        label: impl Into<String>,
    ) {
        self.diags
            .push(Diagnostic::error(code, message).with_label(span, label));
    }

    pub fn unsupported(&mut self, what: &str, span: Span) {
        self.error("V9000", format!("{what}尚未支持"), span, "");
    }

    /// 声明层级不允许保留`debug`条件
    pub fn prune_decls<T>(&mut self, nodes: &'a [ast::Node<T>]) -> Vec<&'a T> {
        let pruned = self.cfg.prune(nodes, &mut self.diags);
        let mut out = Vec::new();
        for p in pruned {
            if p.debug.is_some() {
                self.diags.push(
                    Diagnostic::error("V6002", "`debug`条件只能用于view与修饰符")
                        .with_label(p.cond_span, "")
                        .with_note("声明在调试与发布构建中必须一致，平台与自定义flag不受此限制"),
                );
                continue;
            }
            out.push(p.item);
        }
        out
    }

    fn run(&mut self, files: &'a [ast::File]) -> Program {
        for file in files {
            for item in self.prune_decls(&file.items) {
                self.register(item);
            }
        }
        self.check_decls();
        let screens = self.check_screens();
        let mut strings: Vec<LocString> = self.strings.values().map(|(s, _)| s.clone()).collect();
        strings.sort_by(|a, b| a.key.cmp(&b.key));
        Program {
            enums: self.enums.values().cloned().collect(),
            models: self.models.values().cloned().collect(),
            repositories: self.repositories.values().cloned().collect(),
            components: Vec::new(),
            screens,
            strings,
        }
    }

    fn register(&mut self, item: &'a ast::Item) {
        let name = item.name();
        let global = match item {
            ast::Item::Enum(d) => Global::Enum(d),
            ast::Item::Model(d) => Global::Model(d),
            ast::Item::Repository(d) => Global::Repository(d),
            ast::Item::Screen(d) => Global::Screen(d),
            _ => {
                self.unsupported(&format!("`{}`声明", item.keyword()), name.span);
                Global::Unsupported
            }
        };
        if is_builtin_type(&name.name) {
            self.error(
                "V1002",
                format!("`{}`是内置类型名称", name.name),
                name.span,
                "请换一个名称",
            );
            return;
        }
        if let Some((_, prev)) = self.globals.get(&name.name) {
            self.diags.push(
                Diagnostic::error("V1001", format!("重复声明`{}`", name.name))
                    .with_label(name.span, "重复的声明")
                    .with_secondary(*prev, "首次声明于此"),
            );
            return;
        }
        self.check_type_name(name);
        self.globals.insert(name.name.clone(), (global, name.span));
    }

    pub fn check_type_name(&mut self, name: &ast::Ident) {
        if !name.name.starts_with(|c: char| c.is_ascii_uppercase()) {
            self.error(
                "V1021",
                format!("类型名称`{}`应以大写字母开头", name.name),
                name.span,
                "",
            );
        }
    }

    pub fn check_member_name(&mut self, name: &ast::Ident) {
        if !name.name.starts_with(|c: char| c.is_ascii_lowercase()) {
            self.error(
                "V1022",
                format!("成员名称`{}`应以小写字母开头", name.name),
                name.span,
                "",
            );
        }
    }
}

pub(crate) fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "Bool" | "Int" | "Double" | "String" | "Date" | "List" | "Stream" | "Action" | "Binding"
    )
}
