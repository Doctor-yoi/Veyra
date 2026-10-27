// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! Veyra编译器的源码位置与诊断信息
//!
//! 所有阶段（解析、语义检查、代码生成）都通过[`Diagnostic`]报告问题，
//! 由[`render`]统一渲染为rustc风格的文本输出
//!
//! 错误码分段：
//!
//! - `V0xxx` 词法与语法
//! - `V1xxx` 名称解析
//! - `V2xxx` 类型检查
//! - `V3xxx` MVVM规则
//! - `V4xxx` 本地化
//! - `V5xxx` 数据存储
//! - `V6xxx` 条件编译与平台扩展
//! - `V9xxx` 项目配置与输出

mod render;
mod source;

pub use render::{RenderOptions, render, render_all};
pub use source::{FileId, SourceFile, SourceMap, Span};

/// 诊断级别
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

/// 指向源码区间的标注
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub span: Span,
    pub message: String,
    pub primary: bool,
}

/// 一条诊断信息
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn error(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, code, message)
    }

    pub fn warning(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, code, message)
    }

    fn new(severity: Severity, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            severity,
            code,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// 添加主标注
    pub fn with_label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            message: message.into(),
            primary: true,
        });
        self
    }

    /// 添加次要标注
    pub fn with_secondary(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            message: message.into(),
            primary: false,
        });
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// 诊断收集器
#[derive(Clone, Debug, Default)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    pub fn extend(&mut self, other: Diagnostics) {
        self.items.extend(other.items);
    }

    pub fn has_errors(&self) -> bool {
        self.items.iter().any(Diagnostic::is_error)
    }

    pub fn error_count(&self) -> usize {
        self.items.iter().filter(|d| d.is_error()).count()
    }

    pub fn warning_count(&self) -> usize {
        self.items
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 回退到之前的数量，用于尝试性检查
    pub fn truncate(&mut self, len: usize) {
        self.items.truncate(len);
    }

    /// 取出某个位置之后的诊断
    pub fn split_off(&mut self, at: usize) -> Vec<Diagnostic> {
        self.items.split_off(at)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Diagnostic> {
        self.items.iter()
    }

    /// 按位置排序并去除完全重复的条目
    pub fn sorted(mut self) -> Self {
        self.items.sort_by_key(|d| {
            let span = d.labels.iter().find(|l| l.primary).map(|l| l.span);
            (span.map(|s| (s.file.0, s.start)), d.severity)
        });
        self.items.dedup();
        self
    }

    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.items
    }
}

impl From<Diagnostic> for Diagnostics {
    fn from(diagnostic: Diagnostic) -> Self {
        Self {
            items: vec![diagnostic],
        }
    }
}

impl IntoIterator for Diagnostics {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a> IntoIterator for &'a Diagnostics {
    type Item = &'a Diagnostic;
    type IntoIter = std::slice::Iter<'a, Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}
