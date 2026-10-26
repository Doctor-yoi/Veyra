// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! Swift类型、字面量与表达式的映射

use veyra_codegen::{camel_case, escape_ident};
use veyra_ir::{BinaryOp, DateLength, Expr, ExprKind, FormatStyle, Type, UnaryOp};

const KEYWORDS: &[&str] = &[
    "Any",
    "Self",
    "as",
    "associatedtype",
    "break",
    "case",
    "catch",
    "class",
    "continue",
    "default",
    "defer",
    "deinit",
    "do",
    "else",
    "enum",
    "extension",
    "fallthrough",
    "false",
    "fileprivate",
    "for",
    "func",
    "guard",
    "if",
    "import",
    "in",
    "init",
    "inout",
    "internal",
    "is",
    "let",
    "nil",
    "open",
    "operator",
    "private",
    "protocol",
    "public",
    "repeat",
    "rethrows",
    "return",
    "self",
    "static",
    "struct",
    "subscript",
    "super",
    "switch",
    "throw",
    "throws",
    "true",
    "try",
    "typealias",
    "var",
    "where",
    "while",
];

pub(crate) fn ident(name: &str) -> String {
    escape_ident(name, KEYWORDS)
}

pub(crate) fn ty(t: &Type) -> String {
    match t {
        Type::Bool => "Bool".into(),
        Type::Int => "Int".into(),
        Type::Double => "Double".into(),
        Type::String => "String".into(),
        Type::Date => "Date".into(),
        Type::List(inner) => format!("[{}]", ty(inner)),
        Type::Optional(inner) => format!("{}?", ty(inner)),
        Type::Stream(inner) => format!("AsyncStream<{}>", ty(inner)),
        Type::Enum(n) | Type::Model(n) => n.clone(),
        Type::Unknown => "Never".into(),
    }
}

pub(crate) fn string_literal(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub(crate) fn double_literal(v: f64) -> String {
    let s = format!("{v:?}");
    if s.contains(['.', 'e', 'E']) {
        s
    } else {
        format!("{s}.0")
    }
}

/// 本地化key对应的访问器名称
pub(crate) fn l10n_name(key: &str) -> String {
    ident(&camel_case(key))
}

/// state与computed的访问前缀，例如`viewModel.state.`
#[derive(Clone, Copy)]
pub(crate) struct ExprCx<'a> {
    pub state: &'a str,
}

const ATOM: u8 = 10;
const PREFIX: u8 = 8;

fn binary_info(op: BinaryOp) -> (u8, &'static str) {
    match op {
        BinaryOp::Or => (1, "||"),
        BinaryOp::And => (2, "&&"),
        BinaryOp::Eq => (3, "=="),
        BinaryOp::Ne => (3, "!="),
        BinaryOp::Lt => (3, "<"),
        BinaryOp::Le => (3, "<="),
        BinaryOp::Gt => (3, ">"),
        BinaryOp::Ge => (3, ">="),
        BinaryOp::Coalesce => (4, "??"),
        BinaryOp::Add => (5, "+"),
        BinaryOp::Sub => (5, "-"),
        BinaryOp::Mul => (6, "*"),
        BinaryOp::Div => (6, "/"),
        BinaryOp::Rem => (6, "%"),
    }
}

pub(crate) fn expr(cx: ExprCx, e: &Expr) -> String {
    emit(cx, e).0
}

fn wrap(cx: ExprCx, e: &Expr, min: u8) -> String {
    let (s, p) = emit(cx, e);
    if p < min { format!("({s})") } else { s }
}

fn emit(cx: ExprCx, e: &Expr) -> (String, u8) {
    let atom = |s: String| (s, ATOM);
    match &e.kind {
        ExprKind::Bool(b) => atom(b.to_string()),
        ExprKind::Int(v) => atom(v.to_string()),
        ExprKind::Double(v) => atom(double_literal(*v)),
        ExprKind::String(s) => atom(string_literal(s)),
        ExprKind::Nil => atom("nil".into()),
        ExprKind::List(items) => atom(format!(
            "[{}]",
            items
                .iter()
                .map(|i| expr(cx, i))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        ExprKind::EnumCase { enum_name, case } => atom(format!("{enum_name}.{}", ident(case))),
        ExprKind::State(n) | ExprKind::Computed(n) => atom(format!("{}{}", cx.state, ident(n))),
        ExprKind::Local(n) => atom(ident(n)),
        ExprKind::Field(b, f) => atom(format!("{}.{}", wrap(cx, b, ATOM), ident(f))),
        ExprKind::Count(b) => atom(format!("{}.count", wrap(cx, b, ATOM))),
        ExprKind::IsEmpty(b) => atom(format!("{}.isEmpty", wrap(cx, b, ATOM))),
        ExprKind::First(b) => atom(format!("{}.first", wrap(cx, b, ATOM))),
        ExprKind::Contains(a, b) => {
            atom(format!("{}.contains({})", wrap(cx, a, ATOM), expr(cx, b)))
        }
        ExprKind::Unary(op, inner) => {
            let sym = match op {
                UnaryOp::Not => "!",
                UnaryOp::Neg => "-",
            };
            (format!("{sym}{}", wrap(cx, inner, PREFIX)), PREFIX)
        }
        ExprKind::Binary(BinaryOp::Rem, a, b) if a.ty == Type::Double => atom(format!(
            "{}.truncatingRemainder(dividingBy: {})",
            wrap(cx, a, ATOM),
            expr(cx, b)
        )),
        ExprKind::Binary(op, a, b) => {
            let (p, sym) = binary_info(*op);
            // ??右结合，其余左结合
            let (lmin, rmin) = if *op == BinaryOp::Coalesce {
                (p + 1, p)
            } else {
                (p, p + 1)
            };
            // 比较运算不能连写
            let (lmin, rmin) = if p == 3 { (p + 1, p + 1) } else { (lmin, rmin) };
            (
                format!("{} {sym} {}", wrap(cx, a, lmin), wrap(cx, b, rmin)),
                p,
            )
        }
        ExprKind::Conditional(c, a, b) => (
            format!(
                "{} ? {} : {}",
                wrap(cx, c, 1),
                wrap(cx, a, 1),
                wrap(cx, b, 0)
            ),
            0,
        ),
        ExprKind::Plural { key, count } => atom(format!(
            "L10n.{}(count: {})",
            l10n_name(key),
            expr(cx, count)
        )),
        ExprKind::Format { value, style } => atom(format!(
            "{}.formatted({})",
            wrap(cx, value, ATOM),
            format_style(style)
        )),
        ExprKind::Loc { key, args } => {
            if args.is_empty() {
                atom(format!("L10n.{}", l10n_name(key)))
            } else {
                let a: Vec<_> = args
                    .iter()
                    .map(|(n, v)| format!("{}: {}", n, expr(cx, v)))
                    .collect();
                atom(format!("L10n.{}({})", l10n_name(key), a.join(", ")))
            }
        }
    }
}

fn format_style(style: &FormatStyle) -> String {
    let date = |l: DateLength| match l {
        DateLength::Short => ".numeric",
        DateLength::Medium => ".abbreviated",
        DateLength::Long => ".long",
        DateLength::Full => ".complete",
    };
    let time = |l: DateLength| match l {
        DateLength::Short => ".shortened",
        DateLength::Medium => ".standard",
        DateLength::Long | DateLength::Full => ".complete",
    };
    match style {
        FormatStyle::Number => ".number".into(),
        FormatStyle::Percent => ".percent".into(),
        FormatStyle::Currency(code) => format!(".currency(code: {})", string_literal(code)),
        FormatStyle::Date(l) => format!("date: {}, time: .omitted", date(*l)),
        FormatStyle::Time(l) => format!("date: .omitted, time: {}", time(*l)),
        FormatStyle::DateTime(l) => format!("date: {}, time: {}", date(*l), time(*l)),
    }
}

/// 可直接作为`StringProtocol`参数的字符串；字面量包一层`String(...)`避免被当作本地化key
pub(crate) fn plain_string_arg(cx: ExprCx, e: &Expr) -> String {
    match &e.kind {
        ExprKind::String(s) => format!("String({})", string_literal(s)),
        _ => expr(cx, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Expr {
        Expr::new(ExprKind::String(v.into()), Type::String)
    }

    #[test]
    fn parenthesizes_by_precedence() {
        let cx = ExprCx { state: "" };
        let a = Expr::new(ExprKind::State("a".into()), Type::optional(Type::Int));
        let one = Expr::new(ExprKind::Int(1), Type::Int);
        let coalesce = Expr::new(
            ExprKind::Binary(BinaryOp::Coalesce, Box::new(a), Box::new(one.clone())),
            Type::Int,
        );
        let cmp = Expr::new(
            ExprKind::Binary(BinaryOp::Gt, Box::new(coalesce), Box::new(one.clone())),
            Type::Bool,
        );
        let not = Expr::new(ExprKind::Unary(UnaryOp::Not, Box::new(cmp)), Type::Bool);
        assert_eq!(expr(cx, &not), "!(a ?? 1 > 1)");
        assert_eq!(expr(cx, &s("a\"b")), "\"a\\\"b\"");
        assert_eq!(double_literal(16.0), "16.0");
    }
}
