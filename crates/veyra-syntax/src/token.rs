// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

use veyra_diagnostics::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Ident,
    Int,
    Float,
    Str,
    /// `"""`包裹的多行原样字符串
    RawStr,

    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Lt,
    Gt,
    Le,
    Ge,
    EqEq,
    NotEq,
    Eq,
    Comma,
    Colon,
    Dot,
    Semi,
    Arrow,
    Question,
    QuestionQuestion,
    Bang,
    AndAnd,
    OrOr,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    At,

    HashIf,
    HashElseIf,
    HashElse,
    HashEndIf,

    Eof,
}

impl TokenKind {
    /// 用于诊断信息的描述。
    pub fn describe(self) -> &'static str {
        use TokenKind::*;
        match self {
            Ident => "标识符",
            Int => "整数",
            Float => "浮点数",
            Str => "字符串",
            RawStr => "多行字符串",
            LBrace => "`{`",
            RBrace => "`}`",
            LParen => "`(`",
            RParen => "`)`",
            LBracket => "`[`",
            RBracket => "`]`",
            Lt => "`<`",
            Gt => "`>`",
            Le => "`<=`",
            Ge => "`>=`",
            EqEq => "`==`",
            NotEq => "`!=`",
            Eq => "`=`",
            Comma => "`,`",
            Colon => "`:`",
            Dot => "`.`",
            Semi => "`;`",
            Arrow => "`->`",
            Question => "`?`",
            QuestionQuestion => "`??`",
            Bang => "`!`",
            AndAnd => "`&&`",
            OrOr => "`||`",
            Plus => "`+`",
            Minus => "`-`",
            Star => "`*`",
            Slash => "`/`",
            Percent => "`%`",
            At => "`@`",
            HashIf => "`#if`",
            HashElseIf => "`#elseif`",
            HashElse => "`#else`",
            HashEndIf => "`#endif`",
            Eof => "文件结尾",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    /// 前面是否有换行，用于错误恢复
    pub newline_before: bool,
}

/// 源码中的注释，格式化时用于还原。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub span: Span,
    pub text: String,
    /// 注释前同一行是否有代码
    pub trailing: bool,
}
