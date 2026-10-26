// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

use veyra_diagnostics::{Diagnostic, Diagnostics, FileId, Span};

use crate::token::{Comment, Token, TokenKind};

pub struct Lexed {
    pub tokens: Vec<Token>,
    pub comments: Vec<Comment>,
}

/// 将源码切分为token，注释单独收集
pub fn lex(file: FileId, src: &str, diags: &mut Diagnostics) -> Lexed {
    let mut lexer = Lexer {
        file,
        src,
        bytes: src.as_bytes(),
        pos: 0,
        tokens: Vec::new(),
        comments: Vec::new(),
        line_has_code: false,
    };
    lexer.run(diags);
    Lexed {
        tokens: lexer.tokens,
        comments: lexer.comments,
    }
}

struct Lexer<'a> {
    file: FileId,
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    tokens: Vec<Token>,
    comments: Vec<Comment>,
    line_has_code: bool,
}

impl Lexer<'_> {
    fn span(&self, start: usize) -> Span {
        Span::new(self.file, start, self.pos)
    }

    fn peek(&self, offset: usize) -> u8 {
        self.bytes.get(self.pos + offset).copied().unwrap_or(0)
    }

    fn push(&mut self, kind: TokenKind, start: usize) {
        let newline_before = !self.line_has_code;
        self.line_has_code = true;
        self.tokens.push(Token {
            kind,
            span: self.span(start),
            newline_before,
        });
    }

    fn run(&mut self, diags: &mut Diagnostics) {
        while self.pos < self.bytes.len() {
            let start = self.pos;
            let c = self.peek(0);
            match c {
                b'\n' => {
                    self.pos += 1;
                    self.line_has_code = false;
                }
                b' ' | b'\t' | b'\r' => self.pos += 1,
                b'/' if self.peek(1) == b'/' => {
                    while self.pos < self.bytes.len() && self.peek(0) != b'\n' {
                        self.pos += 1;
                    }
                    self.comment(start);
                }
                b'/' if self.peek(1) == b'*' => {
                    self.pos += 2;
                    let mut depth = 1;
                    while self.pos < self.bytes.len() && depth > 0 {
                        if self.peek(0) == b'/' && self.peek(1) == b'*' {
                            depth += 1;
                            self.pos += 2;
                        } else if self.peek(0) == b'*' && self.peek(1) == b'/' {
                            depth -= 1;
                            self.pos += 2;
                        } else {
                            self.pos += 1;
                        }
                    }
                    if depth > 0 {
                        diags.push(
                            Diagnostic::error("V0003", "块注释未闭合")
                                .with_label(self.span(start), "从这里开始"),
                        );
                    }
                    self.comment(start);
                }
                b'"' if self.peek(1) == b'"' && self.peek(2) == b'"' => {
                    self.raw_string(start, diags)
                }
                b'"' => self.string(start, diags),
                b'#' => self.directive(start, diags),
                b'0'..=b'9' => self.number(start),
                c if c == b'_' || c.is_ascii_alphabetic() => {
                    while matches!(self.peek(0), b'_' | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9') {
                        self.pos += 1;
                    }
                    self.push(TokenKind::Ident, start);
                }
                _ => self.punct(start, diags),
            }
        }
        let end = self.bytes.len();
        self.tokens.push(Token {
            kind: TokenKind::Eof,
            span: Span::new(self.file, end, end),
            newline_before: true,
        });
    }

    fn comment(&mut self, start: usize) {
        let text = self.src[start..self.pos].to_string();
        self.comments.push(Comment {
            span: self.span(start),
            text,
            trailing: self.line_has_code,
        });
    }

    fn string(&mut self, start: usize, diags: &mut Diagnostics) {
        self.pos += 1;
        loop {
            match self.peek(0) {
                0 | b'\n' => {
                    diags.push(
                        Diagnostic::error("V0002", "字符串未闭合")
                            .with_label(self.span(start), "缺少结尾的`\"`"),
                    );
                    break;
                }
                b'\\' => self.pos += 2,
                b'"' => {
                    self.pos += 1;
                    break;
                }
                _ => self.pos += 1,
            }
        }
        self.push(TokenKind::Str, start);
    }

    fn raw_string(&mut self, start: usize, diags: &mut Diagnostics) {
        self.pos += 3;
        loop {
            if self.pos >= self.bytes.len() {
                diags.push(
                    Diagnostic::error("V0002", "多行字符串未闭合")
                        .with_label(Span::new(self.file, start, start + 3), "缺少结尾的`\"\"\"`"),
                );
                break;
            }
            if self.peek(0) == b'"' && self.peek(1) == b'"' && self.peek(2) == b'"' {
                self.pos += 3;
                break;
            }
            self.pos += 1;
        }
        self.push(TokenKind::RawStr, start);
    }

    fn number(&mut self, start: usize) {
        while self.peek(0).is_ascii_digit() || self.peek(0) == b'_' {
            self.pos += 1;
        }
        let mut kind = TokenKind::Int;
        if self.peek(0) == b'.' && self.peek(1).is_ascii_digit() {
            self.pos += 1;
            while self.peek(0).is_ascii_digit() || self.peek(0) == b'_' {
                self.pos += 1;
            }
            kind = TokenKind::Float;
        }
        self.push(kind, start);
    }

    fn directive(&mut self, start: usize, diags: &mut Diagnostics) {
        self.pos += 1;
        while self.peek(0).is_ascii_alphabetic() {
            self.pos += 1;
        }
        let kind = match &self.src[start..self.pos] {
            "#if" => TokenKind::HashIf,
            "#elseif" => TokenKind::HashElseIf,
            "#else" => TokenKind::HashElse,
            "#endif" => TokenKind::HashEndIf,
            other => {
                diags.push(
                    Diagnostic::error("V0004", format!("未知的编译指令`{other}`"))
                        .with_label(self.span(start), "")
                        .with_note("可用的指令：#if、#elseif、#else、#endif"),
                );
                return;
            }
        };
        self.push(kind, start);
    }

    fn punct(&mut self, start: usize, diags: &mut Diagnostics) {
        use TokenKind::*;
        let two = [self.peek(0), self.peek(1)];
        let (kind, len) = match &two {
            b"==" => (EqEq, 2),
            b"!=" => (NotEq, 2),
            b"<=" => (Le, 2),
            b">=" => (Ge, 2),
            b"->" => (Arrow, 2),
            b"??" => (QuestionQuestion, 2),
            b"&&" => (AndAnd, 2),
            b"||" => (OrOr, 2),
            _ => match two[0] {
                b'{' => (LBrace, 1),
                b'}' => (RBrace, 1),
                b'(' => (LParen, 1),
                b')' => (RParen, 1),
                b'[' => (LBracket, 1),
                b']' => (RBracket, 1),
                b'<' => (Lt, 1),
                b'>' => (Gt, 1),
                b'=' => (Eq, 1),
                b',' => (Comma, 1),
                b':' => (Colon, 1),
                b'.' => (Dot, 1),
                b';' => (Semi, 1),
                b'?' => (Question, 1),
                b'!' => (Bang, 1),
                b'+' => (Plus, 1),
                b'-' => (Minus, 1),
                b'*' => (Star, 1),
                b'/' => (Slash, 1),
                b'%' => (Percent, 1),
                b'@' => (At, 1),
                _ => {
                    let ch = self.src[start..].chars().next().unwrap_or('?');
                    self.pos += ch.len_utf8();
                    diags.push(
                        Diagnostic::error("V0001", format!("无法识别的字符`{ch}`"))
                            .with_label(self.span(start), ""),
                    );
                    return;
                }
            },
        };
        self.pos += len;
        self.push(kind, start);
    }
}

/// 解析普通字符串字面量的转义，输入包含两侧引号
pub fn unescape(raw: &str) -> Result<String, String> {
    let inner = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(raw);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('{') => out.push_str("\\{"),
            Some('}') => out.push_str("\\}"),
            Some(other) => return Err(format!("未知的转义序列`\\{other}`")),
            None => return Err("转义序列不完整".to_string()),
        }
    }
    Ok(out)
}

/// 解析多行原样字符串：去掉首尾空行并按最小缩进对齐
pub fn dedent_raw(raw: &str) -> String {
    let inner = &raw[3..raw.len().saturating_sub(3).max(3)];
    let inner = inner
        .strip_prefix('\n')
        .or_else(|| inner.strip_prefix("\r\n"))
        .unwrap_or(inner);
    let mut lines: Vec<&str> = inner.split('\n').collect();
    if lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| {
            if l.len() >= indent {
                &l[indent..]
            } else {
                l.trim_start()
            }
        })
        .map(|l| l.trim_end_matches('\r'))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        let mut d = Diagnostics::new();
        let lexed = lex(FileId(0), src, &mut d);
        assert!(d.is_empty());
        lexed.tokens.into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn lexes_operators_and_directives() {
        use TokenKind::*;
        assert_eq!(
            kinds("#if ios && !debug\na ?? b -> c >= 1.5 // x\n#endif"),
            vec![
                HashIf,
                Ident,
                AndAnd,
                Bang,
                Ident,
                Ident,
                QuestionQuestion,
                Ident,
                Arrow,
                Ident,
                Ge,
                Float,
                HashEndIf,
                Eof
            ]
        );
    }

    #[test]
    fn dedents_raw_string() {
        let raw = "\"\"\"\n    a\n      b\n    \"\"\"";
        assert_eq!(dedent_raw(raw), "a\n  b");
    }

    #[test]
    fn unescapes() {
        assert_eq!(unescape(r#""a\n\"b\"""#).unwrap(), "a\n\"b\"");
    }
}
