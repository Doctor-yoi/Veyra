// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! Kotlin类型、字面量与表达式的映射

use std::collections::BTreeSet;

use veyra_codegen::{escape_ident, snake_case};
use veyra_ir::{BinaryOp, DateLength, Expr, ExprKind, FormatStyle, Type, UnaryOp};

const KEYWORDS: &[&str] = &[
    "as",
    "break",
    "class",
    "continue",
    "do",
    "else",
    "false",
    "for",
    "fun",
    "if",
    "in",
    "interface",
    "is",
    "null",
    "object",
    "package",
    "return",
    "super",
    "this",
    "throw",
    "true",
    "try",
    "typealias",
    "typeof",
    "val",
    "var",
    "when",
    "while",
];

pub(crate) fn ident(name: &str) -> String {
    escape_ident(name, KEYWORDS)
}

/// 生成文件的import集合
#[derive(Default)]
pub(crate) struct Imports {
    set: BTreeSet<String>,
    /// R类所在的包
    pub r_package: String,
    /// 是否用到了需要`@OptIn(ExperimentalMaterial3Api::class)`的API
    pub experimental: bool,
}

impl Imports {
    pub fn new(r_package: &str) -> Self {
        Self {
            set: BTreeSet::new(),
            r_package: r_package.to_string(),
            experimental: false,
        }
    }

    pub fn add(&mut self, path: &str) {
        self.set.insert(path.to_string());
    }

    pub fn add_r(&mut self) {
        let r = format!("{}.R", self.r_package);
        self.set.insert(r);
    }

    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.set.iter()
    }
}

pub(crate) fn ty(t: &Type, im: &mut Imports) -> String {
    match t {
        Type::Bool => "Boolean".into(),
        Type::Int => "Int".into(),
        Type::Double => "Double".into(),
        Type::String => "String".into(),
        Type::Date => {
            im.add("java.time.Instant");
            "Instant".into()
        }
        Type::List(inner) => format!("List<{}>", ty(inner, im)),
        Type::Optional(inner) => format!("{}?", ty(inner, im)),
        Type::Stream(inner) => {
            im.add("kotlinx.coroutines.flow.Flow");
            format!("Flow<{}>", ty(inner, im))
        }
        Type::Enum(n) | Type::Model(n) => n.clone(),
        Type::Unknown => "Nothing".into(),
    }
}

pub(crate) fn string_literal(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '$' => out.push_str("\\$"),
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

/// 本地化key对应的资源名
pub(crate) fn resource_name(key: &str) -> String {
    snake_case(key)
}

#[derive(Clone, Copy)]
pub(crate) struct ExprCx<'a> {
    /// state与computed的访问前缀，例如`state.`
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
        BinaryOp::Coalesce => (4, "?:"),
        BinaryOp::Add => (5, "+"),
        BinaryOp::Sub => (5, "-"),
        BinaryOp::Mul => (6, "*"),
        BinaryOp::Div => (6, "/"),
        BinaryOp::Rem => (6, "%"),
    }
}

pub(crate) fn expr(cx: ExprCx, e: &Expr, im: &mut Imports) -> String {
    emit(cx, e, im).0
}

pub(crate) fn wrap(cx: ExprCx, e: &Expr, min: u8, im: &mut Imports) -> String {
    let (s, p) = emit(cx, e, im);
    if p < min { format!("({s})") } else { s }
}

/// 后缀调用前的表达式，例如`x.dp`中的`x`
pub(crate) fn atom(cx: ExprCx, e: &Expr, im: &mut Imports) -> String {
    wrap(cx, e, ATOM, im)
}

fn emit(cx: ExprCx, e: &Expr, im: &mut Imports) -> (String, u8) {
    let a = |s: String| (s, ATOM);
    match &e.kind {
        ExprKind::Bool(b) => a(b.to_string()),
        ExprKind::Int(v) => a(v.to_string()),
        ExprKind::Double(v) => a(double_literal(*v)),
        ExprKind::String(s) => a(string_literal(s)),
        ExprKind::Nil => a("null".into()),
        ExprKind::Now => {
            im.add("java.time.Instant");
            a("Instant.now()".into())
        }
        ExprKind::List(items) if items.is_empty() => a("emptyList()".into()),
        ExprKind::List(items) => {
            let parts: Vec<_> = items.iter().map(|i| expr(cx, i, im)).collect();
            a(format!("listOf({})", parts.join(", ")))
        }
        ExprKind::EnumCase { enum_name, case } => a(format!("{enum_name}.{}", ident(case))),
        ExprKind::State(n) | ExprKind::Computed(n) => a(format!("{}{}", cx.state, ident(n))),
        ExprKind::Local(n) => a(ident(n)),
        ExprKind::Field(b, f) => a(format!("{}.{}", atom(cx, b, im), ident(f))),
        ExprKind::Count(b) => {
            let member = if b.ty == Type::String {
                "length"
            } else {
                "size"
            };
            a(format!("{}.{member}", atom(cx, b, im)))
        }
        ExprKind::IsEmpty(b) => a(format!("{}.isEmpty()", atom(cx, b, im))),
        ExprKind::First(b) => a(format!("{}.firstOrNull()", atom(cx, b, im))),
        ExprKind::Contains(x, y) => {
            let arg = expr(cx, y, im);
            a(format!("{}.contains({arg})", atom(cx, x, im)))
        }
        ExprKind::Unary(op, inner) => {
            let sym = match op {
                UnaryOp::Not => "!",
                UnaryOp::Neg => "-",
            };
            (format!("{sym}{}", wrap(cx, inner, PREFIX, im)), PREFIX)
        }
        ExprKind::Binary(op, l, r) => {
            let (p, sym) = binary_info(*op);
            let (lmin, rmin) = match op {
                BinaryOp::Coalesce => (p + 1, p),
                _ if p == 3 => (p + 1, p + 1),
                _ => (p, p + 1),
            };
            let ls = wrap(cx, l, lmin, im);
            let rs = wrap(cx, r, rmin, im);
            (format!("{ls} {sym} {rs}"), p)
        }
        ExprKind::Conditional(c, x, y) => {
            let cs = expr(cx, c, im);
            let xs = expr(cx, x, im);
            let ys = expr(cx, y, im);
            (format!("if ({cs}) {xs} else {ys}"), 0)
        }
        ExprKind::Plural { key, count } => {
            im.add("androidx.compose.ui.res.pluralStringResource");
            im.add_r();
            let c = expr(cx, count, im);
            a(format!(
                "pluralStringResource(R.plurals.{}, {c}, {c})",
                resource_name(key)
            ))
        }
        ExprKind::Format { value, style } => {
            let v = expr(cx, value, im);
            let date = |l: DateLength| match l {
                DateLength::Short => "FormatStyle.SHORT",
                DateLength::Medium => "FormatStyle.MEDIUM",
                DateLength::Long => "FormatStyle.LONG",
                DateLength::Full => "FormatStyle.FULL",
            };
            let call = match style {
                FormatStyle::Number => format!("veyraFormatNumber({v})"),
                FormatStyle::Percent => format!("veyraFormatPercent({v})"),
                FormatStyle::Currency(code) => {
                    format!("veyraFormatCurrency({v}, {})", string_literal(code))
                }
                FormatStyle::Date(l) => format!("veyraFormatDate({v}, date = {})", date(*l)),
                FormatStyle::Time(l) => format!("veyraFormatDate({v}, time = {})", date(*l)),
                FormatStyle::DateTime(l) => {
                    format!("veyraFormatDate({v}, date = {0}, time = {0})", date(*l))
                }
            };
            if matches!(
                style,
                FormatStyle::Date(_) | FormatStyle::Time(_) | FormatStyle::DateTime(_)
            ) {
                im.add("java.time.format.FormatStyle");
            }
            a(call)
        }
        ExprKind::Loc { key, args } => {
            im.add("androidx.compose.ui.res.stringResource");
            im.add_r();
            let mut parts = vec![format!("R.string.{}", resource_name(key))];
            for (_, v) in args {
                parts.push(expr(cx, v, im));
            }
            a(format!("stringResource({})", parts.join(", ")))
        }
    }
}

/// 转为Float，用于Slider与进度条
pub(crate) fn float(cx: ExprCx, e: &Expr, im: &mut Imports) -> String {
    match &e.kind {
        ExprKind::Double(v) => format!("{}f", double_literal(*v)),
        _ => format!("{}.toFloat()", atom(cx, e, im)),
    }
}

/// 转为dp
pub(crate) fn dp(cx: ExprCx, e: &Expr, im: &mut Imports) -> String {
    im.add("androidx.compose.ui.unit.dp");
    format!("{}.dp", atom(cx, e, im))
}
