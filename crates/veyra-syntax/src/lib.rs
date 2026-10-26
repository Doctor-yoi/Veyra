// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! Veyra语言前端：词法分析、语法树与解析器。
//!
//! 解析流程为`源码 -> token -> AST`，语法对换行不敏感，
//! 遇到错误时会跳过当前行或当前块继续解析，以便一次报告尽可能多的问题。
//! 注释不进入AST，而是按位置单独返回，供格式化器还原。

pub mod ast;
mod lexer;
mod parser;
mod token;

pub use lexer::{dedent_raw, unescape};
pub use token::{Comment, Token, TokenKind};

use veyra_diagnostics::{Diagnostics, FileId};

/// 单个文件的解析结果。
#[derive(Clone, Debug)]
pub struct Parsed {
    pub file: ast::File,
    pub comments: Vec<Comment>,
    pub diagnostics: Diagnostics,
}

/// 解析一个源文件。
pub fn parse(file: FileId, src: &str) -> Parsed {
    let mut diagnostics = Diagnostics::new();
    let lexed = lexer::lex(file, src, &mut diagnostics);
    let mut parser = parser::Parser::new(file, src, lexed.tokens);
    let ast = parser.file();
    diagnostics.extend(parser.diags);
    Parsed {
        file: ast,
        comments: lexed.comments,
        diagnostics: diagnostics.sorted(),
    }
}
