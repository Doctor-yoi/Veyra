// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 表达式、命令分发与绑定的检查

use indexmap::{IndexMap, IndexSet};
use veyra_diagnostics::{Diagnostic, Span};
use veyra_ir::{
    self as ir, ActionRef, BindingRef, ComponentParamType, ExprKind, LocArg, LocArgType, LocString,
    Segment, Type, parse_template,
};
use veyra_syntax::ast::{self, Arg, ExprKind as A};

use crate::{Checker, Global};

pub(crate) struct StateInfo {
    pub ty: Type,
    pub writable: bool,
    pub span: Span,
}

pub(crate) struct ActionInfo {
    pub params: Vec<ir::ParamDef>,
    pub is_async: bool,
}

/// screen中可被表达式引用的成员
#[derive(Default)]
pub(crate) struct ScreenInfo {
    pub state: IndexMap<String, StateInfo>,
    pub computed: IndexMap<String, Type>,
    pub actions: IndexMap<String, ActionInfo>,
    pub injects: IndexSet<String>,
    pub effects: IndexSet<String>,
}

pub(crate) enum Owner<'s> {
    Screen(&'s ScreenInfo),
    /// 组件只能访问自己的参数
    Component(&'s [ir::ComponentParam]),
    /// state默认值，只允许常量
    Constant,
}

pub(crate) struct Scope<'s> {
    pub owner: Owner<'s>,
    pub locals: Vec<(String, Type)>,
    /// 是否允许loc，只有view中允许
    pub allow_loc: bool,
    /// 外层`if available(...)`提高的系统版本
    pub available: Vec<(veyra_ir::AppleOs, veyra_catalog::Version)>,
}

impl<'s> Scope<'s> {
    pub fn new(owner: Owner<'s>, allow_loc: bool) -> Self {
        Self {
            owner,
            locals: Vec::new(),
            allow_loc,
            available: Vec::new(),
        }
    }

    /// 当前作用域中某个系统的最低版本
    pub fn min_version(
        &self,
        os: veyra_ir::AppleOs,
        deployment: veyra_catalog::Version,
    ) -> veyra_catalog::Version {
        self.available
            .iter()
            .filter(|(o, _)| *o == os)
            .map(|(_, v)| *v)
            .fold(deployment, std::cmp::max)
    }

    fn screen(&self) -> Option<&'s ScreenInfo> {
        match self.owner {
            Owner::Screen(s) => Some(s),
            Owner::Component(_) | Owner::Constant => None,
        }
    }

    pub fn is_action(&self, name: &str) -> bool {
        self.screen().is_some_and(|s| s.actions.contains_key(name))
    }

    pub fn is_action_param(&self, name: &str) -> bool {
        matches!(self.component_param(name), Some(ComponentParamType::Action))
    }

    fn component_param(&self, name: &str) -> Option<&'s ComponentParamType> {
        match self.owner {
            Owner::Component(ps) => ps.iter().find(|p| p.name == name).map(|p| &p.ty),
            _ => None,
        }
    }
}

/// 整数字面量在需要Double的位置提升为Double
pub(crate) fn promote(e: ir::Expr) -> ir::Expr {
    match e.kind {
        ExprKind::Int(v) => ir::Expr::new(ExprKind::Double(v as f64), Type::Double),
        ExprKind::Unary(ir::UnaryOp::Neg, inner) if matches!(inner.kind, ExprKind::Int(_)) => {
            ir::Expr::new(
                ExprKind::Unary(ir::UnaryOp::Neg, Box::new(promote(*inner))),
                Type::Double,
            )
        }
        _ => e,
    }
}

fn is_int_literal(e: &ir::Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_) => true,
        ExprKind::Unary(ir::UnaryOp::Neg, inner) => is_int_literal(inner),
        _ => false,
    }
}

/// 普通字符串中`\{`与`\}`还原为花括号
fn plain_string(s: &str) -> String {
    s.replace("\\{", "{").replace("\\}", "}")
}

fn binary_op(op: ast::BinaryOp) -> ir::BinaryOp {
    use ast::BinaryOp as B;
    match op {
        B::Coalesce => ir::BinaryOp::Coalesce,
        B::Or => ir::BinaryOp::Or,
        B::And => ir::BinaryOp::And,
        B::Eq => ir::BinaryOp::Eq,
        B::Ne => ir::BinaryOp::Ne,
        B::Lt => ir::BinaryOp::Lt,
        B::Le => ir::BinaryOp::Le,
        B::Gt => ir::BinaryOp::Gt,
        B::Ge => ir::BinaryOp::Ge,
        B::Add => ir::BinaryOp::Add,
        B::Sub => ir::BinaryOp::Sub,
        B::Mul => ir::BinaryOp::Mul,
        B::Div => ir::BinaryOp::Div,
        B::Rem => ir::BinaryOp::Rem,
    }
}

fn valid_loc_key(key: &str) -> bool {
    !key.is_empty()
        && key.split('.').all(|part| {
            part.starts_with(|c: char| c.is_ascii_lowercase())
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
}

impl<'a> Checker<'a> {
    pub(crate) fn mismatch(&mut self, span: Span, expected: &Type, found: &Type) {
        self.error(
            "V2001",
            format!("类型不匹配：期望`{expected}`，实际为`{found}`"),
            span,
            format!("类型为`{found}`"),
        );
    }

    /// 检查表达式，`expected`用于推断字面量与隐式成员并校验结果类型
    pub(crate) fn expr(&mut self, sc: &Scope, e: &ast::Expr, expected: Option<&Type>) -> ir::Expr {
        let mut out = self.expr_inner(sc, e, expected);
        if let Some(exp) = expected {
            if matches!(exp.unwrap_optional(), Type::Double) && is_int_literal(&out) {
                out = promote(out);
            }
            if !exp.accepts(&out.ty) {
                self.mismatch(e.span, exp, &out.ty);
            }
        }
        out
    }

    fn expr_inner(&mut self, sc: &Scope, e: &ast::Expr, expected: Option<&Type>) -> ir::Expr {
        if matches!(sc.owner, Owner::Constant) && !is_constant(e) {
            self.diags.push(
                Diagnostic::error("V2011", "默认值必须是常量")
                    .with_label(e.span, "")
                    .with_note("可以使用字面量、列表字面量、枚举成员与nil"),
            );
            return ir::Expr::unknown();
        }
        match &e.kind {
            A::Int(v) => match expected.map(Type::unwrap_optional) {
                Some(Type::Double) => ir::Expr::new(ExprKind::Double(*v as f64), Type::Double),
                _ => ir::Expr::new(ExprKind::Int(*v), Type::Int),
            },
            A::Float(v, _) => ir::Expr::new(ExprKind::Double(*v), Type::Double),
            A::Str(s, _) => ir::Expr::new(ExprKind::String(plain_string(s)), Type::String),
            A::Bool(b) => ir::Expr::new(ExprKind::Bool(*b), Type::Bool),
            A::Nil => match expected {
                Some(t) if t.is_optional() => ir::Expr::new(ExprKind::Nil, t.clone()),
                Some(t) if t.is_unknown() => ir::Expr::unknown(),
                _ => {
                    self.error("V2002", "nil只能用于可选类型", e.span, "");
                    ir::Expr::unknown()
                }
            },
            A::Ident(name) => self.ident(sc, name, e.span),
            A::ImplicitMember(id) => match expected.map(Type::unwrap_optional) {
                Some(Type::Enum(en)) => self.enum_case(en.clone(), id),
                Some(Type::Unknown) => ir::Expr::unknown(),
                _ => {
                    self.error(
                        "V2020",
                        format!("无法推断`.{}`所属的类型", id.name),
                        e.span,
                        "请写出完整的类型名",
                    );
                    ir::Expr::unknown()
                }
            },
            A::Member(base, name) => {
                if let A::Ident(n) = &base.kind
                    && self.lookup_local(sc, n).is_none()
                    && matches!(self.globals.get(n.as_str()), Some((Global::Enum(_), _)))
                    && matches!(self.lookup(n, base.span), Some(Global::Enum(_)))
                {
                    return self.enum_case(n.clone(), name);
                }
                let b = self.expr(sc, base, None);
                self.member(b, name, e.span)
            }
            A::Call(callee, args) => self.call(sc, callee, args, e.span),
            A::Unary(op, inner) => match op {
                ast::UnaryOp::Not => {
                    let i = self.expr(sc, inner, Some(&Type::Bool));
                    ir::Expr::new(ExprKind::Unary(ir::UnaryOp::Not, Box::new(i)), Type::Bool)
                }
                ast::UnaryOp::Neg => {
                    let i = self.expr(sc, inner, expected);
                    if !i.ty.is_numeric() && !i.ty.is_unknown() {
                        self.error("V2026", format!("`-`不能用于`{}`", i.ty), inner.span, "");
                    }
                    let ty = i.ty.clone();
                    ir::Expr::new(ExprKind::Unary(ir::UnaryOp::Neg, Box::new(i)), ty)
                }
            },
            A::Binary(op, l, r) => self.binary(sc, *op, l, r, e.span),
            A::Ternary(c, a, b) => {
                let c = self.expr(sc, c, Some(&Type::Bool));
                let a = self.expr(sc, a, expected);
                let hint = if a.ty.is_unknown() {
                    expected.cloned()
                } else {
                    Some(a.ty.clone())
                };
                let b = self.expr(sc, b, hint.as_ref());
                let (a, b) = self.unify_numeric(a, b);
                let ty = if a.ty.is_unknown() {
                    b.ty.clone()
                } else {
                    a.ty.clone()
                };
                ir::Expr::new(
                    ExprKind::Conditional(Box::new(c), Box::new(a), Box::new(b)),
                    ty,
                )
            }
            A::List(items) => {
                let elem = expected
                    .and_then(|t| t.unwrap_optional().element())
                    .cloned();
                if items.is_empty() {
                    return match elem {
                        Some(t) => ir::Expr::new(ExprKind::List(Vec::new()), Type::list(t)),
                        None => {
                            self.error("V2027", "无法推断空列表的元素类型", e.span, "");
                            ir::Expr::unknown()
                        }
                    };
                }
                let first = self.expr(sc, &items[0], elem.as_ref());
                let elem = elem.unwrap_or_else(|| first.ty.clone());
                let mut out = vec![first];
                for item in &items[1..] {
                    out.push(self.expr(sc, item, Some(&elem)));
                }
                ir::Expr::new(ExprKind::List(out), Type::list(elem))
            }
            A::Paren(inner) => self.expr(sc, inner, expected),
            A::Closure(c) => {
                self.error("V2074", "视图闭包只能作为组件参数", c.span, "");
                ir::Expr::unknown()
            }
            A::Error => ir::Expr::unknown(),
        }
    }

    fn lookup_local(&self, sc: &Scope, name: &str) -> Option<Type> {
        sc.locals
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, t)| t.clone())
    }

    fn ident(&mut self, sc: &Scope, name: &str, span: Span) -> ir::Expr {
        if let Some(ty) = self.lookup_local(sc, name) {
            return ir::Expr::new(ExprKind::Local(name.to_string()), ty);
        }
        match sc.component_param(name) {
            Some(ComponentParamType::Value(t)) => {
                return ir::Expr::new(ExprKind::Local(name.to_string()), t.clone());
            }
            Some(_) => {
                self.error(
                    "V3004",
                    format!("`{name}`是Action或Binding参数，不能作为值使用"),
                    span,
                    "",
                );
                return ir::Expr::unknown();
            }
            None => {}
        }
        if let Some(screen) = sc.screen() {
            if let Some(s) = screen.state.get(name) {
                return ir::Expr::new(ExprKind::State(name.to_string()), s.ty.clone());
            }
            if let Some(t) = screen.computed.get(name) {
                return ir::Expr::new(ExprKind::Computed(name.to_string()), t.clone());
            }
            if screen.actions.contains_key(name) {
                self.diags.push(
                    Diagnostic::error("V3004", format!("`{name}`是action，不能作为值使用"))
                        .with_label(span, "")
                        .with_note("action只能出现在action参数、onTap与lifecycle中"),
                );
                return ir::Expr::unknown();
            }
            if screen.injects.contains(name) {
                self.diags.push(
                    Diagnostic::error("V3009", format!("View不能直接访问注入的依赖`{name}`"))
                        .with_label(span, "")
                        .with_note("依赖只能在ViewModel中使用，请通过state向View暴露数据"),
                );
                return ir::Expr::unknown();
            }
        }
        if self.globals.contains_key(name) {
            self.error("V2022", format!("`{name}`是类型，不能作为值使用"), span, "");
        } else {
            self.error("V1010", format!("未定义的名称`{name}`"), span, "");
        }
        ir::Expr::unknown()
    }

    fn enum_case(&mut self, en: String, case: &ast::Ident) -> ir::Expr {
        let ok = self
            .enums
            .get(&en)
            .is_some_and(|d| d.cases.contains(&case.name));
        if !ok {
            self.error(
                "V1013",
                format!("枚举`{en}`没有成员`{}`", case.name),
                case.span,
                "",
            );
            return ir::Expr::unknown();
        }
        ir::Expr::new(
            ExprKind::EnumCase {
                enum_name: en.clone(),
                case: case.name.clone(),
            },
            Type::Enum(en),
        )
    }

    fn member(&mut self, base: ir::Expr, name: &ast::Ident, span: Span) -> ir::Expr {
        let n = name.name.as_str();
        let ty = base.ty.clone();
        let b = Box::new(base);
        match (&ty, n) {
            (Type::Unknown, _) => ir::Expr::unknown(),
            (Type::Optional(_), _) => {
                self.diags.push(
                    Diagnostic::error("V2021", format!("不能直接访问可选值`{ty}`的成员"))
                        .with_label(span, "")
                        .with_note("请先用`??`提供默认值"),
                );
                ir::Expr::unknown()
            }
            (Type::List(_) | Type::String, "count") => ir::Expr::new(ExprKind::Count(b), Type::Int),
            (Type::List(_) | Type::String, "isEmpty") => {
                ir::Expr::new(ExprKind::IsEmpty(b), Type::Bool)
            }
            (Type::List(inner), "first") => {
                let t = Type::optional((**inner).clone());
                ir::Expr::new(ExprKind::First(b), t)
            }
            (Type::Model(m), _) => {
                let field = self
                    .models
                    .get(m)
                    .and_then(|d| d.fields.iter().find(|f| f.name == n))
                    .map(|f| f.ty.clone());
                match field {
                    Some(t) => ir::Expr::new(ExprKind::Field(b, n.to_string()), t),
                    None => {
                        self.error("V2025", format!("`{m}`没有字段`{n}`"), name.span, "");
                        ir::Expr::unknown()
                    }
                }
            }
            _ => {
                self.error("V2025", format!("`{ty}`没有成员`{n}`"), name.span, "");
                ir::Expr::unknown()
            }
        }
    }

    fn call(&mut self, sc: &Scope, callee: &ast::Expr, args: &[Arg], span: Span) -> ir::Expr {
        match &callee.kind {
            A::Ident(n) if n == "loc" => return self.loc(sc, args, span),
            A::Ident(n) if n == "plural" => return self.plural(sc, args, span),
            A::Ident(n) if n == "format" => return self.format(sc, args, span),
            A::Ident(n) if n == "bind" => {
                self.error(
                    "V3013",
                    "bind()只能用于绑定参数",
                    span,
                    "例如TextField的text参数",
                );
                return ir::Expr::unknown();
            }
            A::Ident(n) if sc.screen().is_some_and(|s| s.actions.contains_key(n)) => {
                self.error(
                    "V3010",
                    format!("不能在表达式中调用action`{n}`"),
                    span,
                    "action只能出现在action参数中",
                );
                return ir::Expr::unknown();
            }
            A::Member(base, name) if name.name == "contains" => {
                let b = self.expr(sc, base, None);
                let elem = match &b.ty {
                    Type::String => Some(Type::String),
                    Type::List(t) => Some((**t).clone()),
                    Type::Unknown => None,
                    other => {
                        self.error(
                            "V2025",
                            format!("`{other}`没有成员`contains`"),
                            name.span,
                            "",
                        );
                        None
                    }
                };
                if args.len() != 1 || args[0].label.is_some() {
                    self.error("V2035", "contains需要一个位置参数", span, "");
                    return ir::Expr::unknown();
                }
                let a = self.expr(sc, &args[0].value, elem.as_ref());
                return ir::Expr::new(ExprKind::Contains(Box::new(b), Box::new(a)), Type::Bool);
            }
            _ => {}
        }
        self.diags.push(
            Diagnostic::error("V3005", "View中只能调用内置函数")
                .with_label(callee.span, "")
                .with_note("可用的函数：loc、plural、format；有副作用的操作请放进action"),
        );
        ir::Expr::unknown()
    }

    fn binary(
        &mut self,
        sc: &Scope,
        op: ast::BinaryOp,
        l: &ast::Expr,
        r: &ast::Expr,
        span: Span,
    ) -> ir::Expr {
        use ast::BinaryOp as B;
        let irop = binary_op(op);
        let make = |a: ir::Expr, b: ir::Expr, ty: Type| {
            ir::Expr::new(ExprKind::Binary(irop, Box::new(a), Box::new(b)), ty)
        };
        match op {
            B::And | B::Or => {
                let a = self.expr(sc, l, Some(&Type::Bool));
                let b = self.expr(sc, r, Some(&Type::Bool));
                make(a, b, Type::Bool)
            }
            B::Coalesce => {
                let a = self.expr(sc, l, None);
                let inner = match &a.ty {
                    Type::Optional(t) => (**t).clone(),
                    Type::Unknown => Type::Unknown,
                    other => {
                        self.error(
                            "V2023",
                            format!("`??`左侧必须是可选值，实际为`{other}`"),
                            l.span,
                            "",
                        );
                        other.clone()
                    }
                };
                let b = self.expr(sc, r, Some(&Type::optional(inner.clone())));
                let ty = if b.ty.is_optional() {
                    Type::optional(inner)
                } else {
                    inner
                };
                make(a, b, ty)
            }
            B::Eq | B::Ne => {
                let a = self.expr(sc, l, None);
                let b = self.expr(sc, r, Some(&a.ty));
                let (a, b) = self.unify_numeric(a, b);
                make(a, b, Type::Bool)
            }
            B::Lt | B::Le | B::Gt | B::Ge => {
                let a = self.expr(sc, l, None);
                let b = self.expr(sc, r, None);
                let (a, b) = self.unify_numeric(a, b);
                let ok = |t: &Type| {
                    t.is_numeric() || matches!(t, Type::String | Type::Date | Type::Unknown)
                };
                if !ok(&a.ty) || !ok(&b.ty) || !(a.ty.accepts(&b.ty)) {
                    self.error("V2028", format!("不能比较`{}`与`{}`", a.ty, b.ty), span, "");
                }
                make(a, b, Type::Bool)
            }
            B::Add | B::Sub | B::Mul | B::Div | B::Rem => {
                let a = self.expr(sc, l, None);
                let b = self.expr(sc, r, None);
                if op == B::Add && a.ty == Type::String && b.ty == Type::String {
                    return make(a, b, Type::String);
                }
                let (a, b) = self.unify_numeric(a, b);
                let ty = if a.ty.is_unknown() {
                    b.ty.clone()
                } else {
                    a.ty.clone()
                };
                let numeric = |t: &Type| t.is_numeric() || t.is_unknown();
                if !numeric(&a.ty) || !numeric(&b.ty) {
                    self.error(
                        "V2029",
                        format!("`{}`不能用于`{}`与`{}`", irop_symbol(irop), a.ty, b.ty),
                        span,
                        "",
                    );
                    return ir::Expr::unknown();
                }
                if !a.ty.accepts(&b.ty) {
                    self.diags.push(
                        Diagnostic::error("V2024", "Int与Double不能直接运算")
                            .with_label(span, "")
                            .with_note("请统一字段类型，或在ViewModel中计算后放入state"),
                    );
                }
                make(a, b, ty)
            }
        }
    }

    fn unify_numeric(&mut self, a: ir::Expr, b: ir::Expr) -> (ir::Expr, ir::Expr) {
        match (&a.ty, &b.ty) {
            (Type::Int, Type::Double) if is_int_literal(&a) => (promote(a), b),
            (Type::Double, Type::Int) if is_int_literal(&b) => (a, promote(b)),
            _ => (a, b),
        }
    }

    /// `loc("key", "模板{name}", name: expr)`
    fn loc(&mut self, sc: &Scope, args: &[Arg], span: Span) -> ir::Expr {
        if !sc.allow_loc {
            self.diags.push(
                Diagnostic::error("V4005", "loc只能在view中使用")
                    .with_label(span, "")
                    .with_note("ViewModel中的state保存原始数据，文案在View中组装"),
            );
            return ir::Expr::unknown();
        }
        let literal = |a: Option<&Arg>| match a {
            Some(Arg {
                label: None,
                value:
                    ast::Expr {
                        kind: A::Str(s, _),
                        span,
                    },
                ..
            }) => Some((s.clone(), *span)),
            _ => None,
        };
        let Some((key, key_span)) = literal(args.first()) else {
            self.error("V4002", "loc的第1个参数必须是字符串字面量的key", span, "");
            return ir::Expr::unknown();
        };
        let Some((template, template_span)) = literal(args.get(1)) else {
            self.error(
                "V4002",
                "loc的第2个参数必须是字符串字面量的源语言文案",
                span,
                "",
            );
            return ir::Expr::unknown();
        };
        if !valid_loc_key(&key) {
            self.diags.push(
                Diagnostic::error("V4002", format!("无效的本地化key`{key}`"))
                    .with_label(key_span, "")
                    .with_note("key由小写字母、数字与下划线组成，用`.`分段，例如`schedule.title`"),
            );
        }
        let segments = match parse_template(&template) {
            Ok(s) => s,
            Err(msg) => {
                self.error("V4004", msg, template_span, "");
                return ir::Expr::unknown();
            }
        };
        let mut placeholders: Vec<String> = Vec::new();
        for seg in &segments {
            if let Segment::Arg(n) = seg
                && !placeholders.contains(n)
            {
                placeholders.push(n.clone());
            }
        }
        let mut given: IndexMap<String, ir::Expr> = IndexMap::new();
        for arg in &args[2..] {
            let Some(label) = &arg.label else {
                self.error(
                    "V4006",
                    "loc的参数必须带名称",
                    arg.span,
                    "例如`count: items.count`",
                );
                continue;
            };
            let value = self.expr(sc, &arg.value, None);
            if !placeholders.contains(&label.name) {
                self.error(
                    "V4007",
                    format!("文案中没有占位符`{{{}}}`", label.name),
                    label.span,
                    "",
                );
                continue;
            }
            if !matches!(value.ty, Type::String | Type::Int | Type::Unknown) {
                self.diags.push(
                    Diagnostic::error(
                        "V4003",
                        format!("文案参数只能是String或Int，实际为`{}`", value.ty),
                    )
                    .with_label(arg.value.span, "")
                    .with_note("其他类型请先用format()格式化为String"),
                );
            }
            given.insert(label.name.clone(), value);
        }
        let mut loc_args = Vec::new();
        let mut ir_args = Vec::new();
        for p in &placeholders {
            match given.shift_remove(p) {
                Some(v) => {
                    let ty = if v.ty == Type::Int {
                        LocArgType::Int
                    } else {
                        LocArgType::String
                    };
                    loc_args.push(LocArg {
                        name: p.clone(),
                        ty,
                    });
                    ir_args.push((p.clone(), v));
                }
                None => self.error(
                    "V4006",
                    format!("缺少占位符`{{{p}}}`对应的参数"),
                    template_span,
                    "",
                ),
            }
        }
        self.record_string(
            LocString {
                key: key.clone(),
                source: template,
                args: loc_args,
                plural: None,
            },
            key_span,
        );
        ir::Expr::new(ExprKind::Loc { key, args: ir_args }, Type::String)
    }

    /// `plural("key", count, other: "共{count}项", one: "...")`
    fn plural(&mut self, sc: &Scope, args: &[Arg], span: Span) -> ir::Expr {
        if !sc.allow_loc {
            self.error("V4005", "plural只能在view中使用", span, "");
            return ir::Expr::unknown();
        }
        let key = match args.first() {
            Some(Arg {
                label: None,
                value:
                    ast::Expr {
                        kind: A::Str(s, _),
                        span,
                    },
                ..
            }) => (s.clone(), *span),
            _ => {
                self.error(
                    "V4002",
                    "plural的第1个参数必须是字符串字面量的key",
                    span,
                    "",
                );
                return ir::Expr::unknown();
            }
        };
        let (key, key_span) = key;
        if !valid_loc_key(&key) {
            self.error("V4002", format!("无效的本地化key`{key}`"), key_span, "");
        }
        let Some(Arg {
            label: None,
            value: count_expr,
            ..
        }) = args.get(1)
        else {
            self.error(
                "V4008",
                "plural的第2个参数必须是数量",
                span,
                "例如`plural(\"cart.items\", items.count, other: \"{count}件\")`",
            );
            return ir::Expr::unknown();
        };
        let count = self.expr(sc, count_expr, Some(&Type::Int));
        let mut forms = IndexMap::new();
        for arg in &args[2..] {
            let Some(label) = &arg.label else {
                self.error(
                    "V4008",
                    "复数形式必须带类别名称",
                    arg.span,
                    "例如`other: \"...\"`",
                );
                continue;
            };
            if !veyra_ir::PLURAL_CATEGORIES.contains(&label.name.as_str()) {
                self.diags.push(
                    Diagnostic::error("V4009", format!("未知的复数类别`{}`", label.name))
                        .with_label(label.span, "")
                        .with_note(format!(
                            "可用类别：{}",
                            veyra_ir::PLURAL_CATEGORIES.join("、")
                        )),
                );
                continue;
            }
            let A::Str(text, _) = &arg.value.kind else {
                self.error("V4008", "复数形式必须是字符串字面量", arg.value.span, "");
                continue;
            };
            match parse_template(text) {
                Ok(segs) => {
                    for seg in segs {
                        if let Segment::Arg(n) = seg
                            && n != "count"
                        {
                            self.error(
                                "V4007",
                                format!("复数文案只能使用`{{count}}`占位符，不能使用`{{{n}}}`"),
                                arg.value.span,
                                "",
                            );
                        }
                    }
                }
                Err(msg) => self.error("V4004", msg, arg.value.span, ""),
            }
            forms.insert(label.name.clone(), text.clone());
        }
        let Some(other) = forms.get("other").cloned() else {
            self.error("V4008", "plural必须提供`other`形式", span, "");
            return ir::Expr::unknown();
        };
        let s = LocString {
            key: key.clone(),
            source: other,
            args: vec![LocArg {
                name: "count".into(),
                ty: LocArgType::Int,
            }],
            plural: Some(veyra_ir::PluralForms { forms }),
        };
        self.record_string(s, key_span);
        ir::Expr::new(
            ExprKind::Plural {
                key,
                count: Box::new(count),
            },
            Type::String,
        )
    }

    /// `format(value, .number)`
    fn format(&mut self, sc: &Scope, args: &[Arg], span: Span) -> ir::Expr {
        let [
            Arg {
                label: None, value, ..
            },
            Arg {
                label: None,
                value: style,
                ..
            },
        ] = args
        else {
            self.error(
                "V2070",
                "format需要两个位置参数",
                span,
                "例如`format(price, .currency(\"CNY\"))`",
            );
            return ir::Expr::unknown();
        };
        let v = self.expr(sc, value, None);
        let (name, sargs, name_span) = match &style.kind {
            A::ImplicitMember(id) => (id.name.as_str(), &[][..], id.span),
            A::Call(callee, a) => match &callee.kind {
                A::ImplicitMember(id) => (id.name.as_str(), a.as_slice(), id.span),
                _ => ("", &[][..], style.span),
            },
            _ => ("", &[][..], style.span),
        };
        let date_len = |c: &mut Self| -> Option<ir::DateLength> {
            match sargs {
                [] => Some(ir::DateLength::Medium),
                [
                    Arg {
                        label: None,
                        value:
                            ast::Expr {
                                kind: A::ImplicitMember(id),
                                ..
                            },
                        ..
                    },
                ] if ir::DateLength::from_name(&id.name).is_some() => {
                    ir::DateLength::from_name(&id.name)
                }
                _ => {
                    c.error(
                        "V2071",
                        "日期样式需要`.short`、`.medium`、`.long`或`.full`",
                        style.span,
                        "",
                    );
                    None
                }
            }
        };
        let (fmt, accepts): (Option<ir::FormatStyle>, fn(&Type) -> bool) = match name {
            "number" => (Some(ir::FormatStyle::Number), Type::is_numeric),
            "percent" => (Some(ir::FormatStyle::Percent), |t| *t == Type::Double),
            "currency" => match sargs {
                [
                    Arg {
                        label: None,
                        value:
                            ast::Expr {
                                kind: A::Str(code, _),
                                ..
                            },
                        ..
                    },
                ] if code.len() == 3 && code.chars().all(|c| c.is_ascii_uppercase()) => (
                    Some(ir::FormatStyle::Currency(code.clone())),
                    Type::is_numeric,
                ),
                _ => {
                    self.error(
                        "V2071",
                        "currency需要ISO 4217货币代码",
                        style.span,
                        "例如`.currency(\"CNY\")`",
                    );
                    (None, Type::is_numeric)
                }
            },
            "date" => (date_len(self).map(ir::FormatStyle::Date), |t| {
                *t == Type::Date
            }),
            "time" => (date_len(self).map(ir::FormatStyle::Time), |t| {
                *t == Type::Date
            }),
            "dateTime" => (date_len(self).map(ir::FormatStyle::DateTime), |t| {
                *t == Type::Date
            }),
            _ => {
                self.diags.push(
                    Diagnostic::error("V2071", "无效的格式样式")
                        .with_label(name_span, "")
                        .with_note("可用样式：.number、.percent、.currency(\"CNY\")、.date、.time、.dateTime"),
                );
                (None, Type::is_numeric)
            }
        };
        let Some(fmt) = fmt else {
            return ir::Expr::unknown();
        };
        if !v.ty.is_unknown() && !accepts(&v.ty) {
            self.error(
                "V2072",
                format!("该格式样式不能用于`{}`", v.ty),
                value.span,
                "",
            );
        }
        ir::Expr::new(
            ExprKind::Format {
                value: Box::new(v),
                style: fmt,
            },
            Type::String,
        )
    }

    pub(crate) fn record_string(&mut self, s: LocString, span: Span) {
        if let Some((prev, prev_span)) = self.strings.get(&s.key) {
            if *prev != s {
                let prev_span = *prev_span;
                self.diags.push(
                    Diagnostic::error("V4001", format!("本地化key`{}`的定义不一致", s.key))
                        .with_label(span, "这里的文案或参数不同")
                        .with_secondary(prev_span, "首次定义于此")
                        .with_note("同一个key在所有位置必须使用相同的源文案与参数"),
                );
            }
            return;
        }
        self.strings.insert(s.key.clone(), (s, span));
    }

    /// 解析action位置的表达式
    pub(crate) fn action_ref(&mut self, sc: &Scope, e: &ast::Expr) -> Option<ActionRef> {
        let screen = sc.screen();
        let (name, args, name_span) = match &e.kind {
            A::Ident(n) => (n.as_str(), None, e.span),
            A::Call(callee, args) => match &callee.kind {
                A::Ident(n) => (n.as_str(), Some(args), callee.span),
                _ => return self.not_action(e.span),
            },
            _ => return self.not_action(e.span),
        };
        if let Some(ty) = sc.component_param(name) {
            if !matches!(ty, ComponentParamType::Action) {
                return self.not_action(name_span);
            }
            if args.is_some() {
                self.error(
                    "V3016",
                    "Action参数不接受实参",
                    e.span,
                    format!("改为`{name}`"),
                );
                return None;
            }
            return Some(ActionRef::Param(name.to_string()));
        }
        let Some(info) = screen.and_then(|s| s.actions.get(name)) else {
            if screen.is_some_and(|s| s.state.contains_key(name) || s.computed.contains_key(name)) {
                return self.not_action(name_span);
            }
            if screen.is_some_and(|s| s.effects.contains(name)) {
                self.diags.push(
                    Diagnostic::error("V3033", format!("`{name}`是effect，不能由View触发"))
                        .with_label(name_span, "")
                        .with_note("effect由ViewModel通过emit发出，View只能分发action"),
                );
                return None;
            }
            self.error("V1010", format!("未定义的action`{name}`"), name_span, "");
            return None;
        };
        let params = &info.params;
        let is_async = info.is_async;
        let Some(args) = args else {
            if !params.is_empty() {
                let names: Vec<_> = params
                    .iter()
                    .map(|p| format!("{}: {}", p.name, p.ty))
                    .collect();
                self.error(
                    "V3015",
                    format!("action`{name}`需要参数({})", names.join(", ")),
                    e.span,
                    "",
                );
                return None;
            }
            return Some(ActionRef::Dispatch {
                action: name.to_string(),
                args: Vec::new(),
                is_async,
            });
        };
        let mut values: Vec<Option<ir::Expr>> = vec![None; params.len()];
        for (i, arg) in args.iter().enumerate() {
            let index = match &arg.label {
                Some(l) => match params.iter().position(|p| p.name == l.name) {
                    Some(i) => i,
                    None => {
                        self.error(
                            "V2030",
                            format!("action`{name}`没有参数`{}`", l.name),
                            l.span,
                            "",
                        );
                        continue;
                    }
                },
                None if i < params.len() => i,
                None => {
                    self.error(
                        "V2035",
                        format!("action`{name}`只接受{}个参数", params.len()),
                        arg.span,
                        "多余的参数",
                    );
                    continue;
                }
            };
            if values[index].is_some() {
                self.error(
                    "V2031",
                    format!("参数`{}`重复", params[index].name),
                    arg.span,
                    "",
                );
                continue;
            }
            let ty = params[index].ty.clone();
            values[index] = Some(self.expr(sc, &arg.value, Some(&ty)));
        }
        let mut out = Vec::new();
        for (p, v) in params.iter().zip(values) {
            match v {
                Some(v) => out.push((p.name.clone(), v)),
                None => self.error("V2036", format!("缺少参数`{}`", p.name), e.span, ""),
            }
        }
        Some(ActionRef::Dispatch {
            action: name.to_string(),
            args: out,
            is_async,
        })
    }

    fn not_action(&mut self, span: Span) -> Option<ActionRef> {
        self.diags.push(
            Diagnostic::error("V3011", "此处需要action")
                .with_label(span, "")
                .with_note("例如`refresh`或`openCourse(course.id)`"),
        );
        None
    }

    /// 解析绑定位置的表达式，`ty`为控件要求的类型
    pub(crate) fn binding_ref(
        &mut self,
        sc: &Scope,
        e: &ast::Expr,
        ty: &Type,
    ) -> Option<BindingRef> {
        let screen = sc.screen();
        let target = match &e.kind {
            A::Call(callee, args) if matches!(&callee.kind, A::Ident(n) if n == "bind") => {
                match args.as_slice() {
                    [
                        Arg {
                            label: None,
                            value:
                                ast::Expr {
                                    kind: A::Ident(n),
                                    span,
                                },
                            ..
                        },
                    ] => (n.as_str(), *span),
                    _ => {
                        self.error(
                            "V3012",
                            "bind()需要一个state字段名",
                            e.span,
                            "例如`bind(query)`",
                        );
                        return None;
                    }
                }
            }
            A::Ident(n)
                if matches!(sc.component_param(n), Some(ComponentParamType::Binding(_))) =>
            {
                (n.as_str(), e.span)
            }
            A::Ident(n) if screen.is_some_and(|s| s.state.contains_key(n)) => {
                self.diags.push(
                    Diagnostic::error("V3017", "双向绑定需要写成bind(...)")
                        .with_label(e.span, "")
                        .with_note(format!("改为`bind({n})`")),
                );
                return None;
            }
            _ => {
                self.diags.push(
                    Diagnostic::error("V3012", "此处需要双向绑定")
                        .with_label(e.span, "")
                        .with_note("例如`bind(query)`，被绑定的state需要标记@writable"),
                );
                return None;
            }
        };
        let (name, span) = target;
        if let Some(pty) = sc.component_param(name) {
            return match pty {
                ComponentParamType::Binding(t) if t == ty || ty.is_unknown() => {
                    Some(BindingRef::Param {
                        name: name.to_string(),
                        ty: t.clone(),
                    })
                }
                ComponentParamType::Binding(t) => {
                    self.mismatch(span, ty, t);
                    None
                }
                _ => {
                    self.error("V3012", format!("参数`{name}`不是Binding"), span, "");
                    None
                }
            };
        }
        let Some(screen) = screen else {
            self.error("V1010", format!("未定义的名称`{name}`"), span, "");
            return None;
        };
        if let Some(state) = screen.state.get(name) {
            if !state.writable {
                self.diags.push(
                    Diagnostic::error("V3001", format!("state`{name}`是只读的，不能绑定"))
                        .with_label(span, "")
                        .with_secondary(state.span, "在这里添加@writable")
                        .with_note("只有标记@writable的state才会生成setter并允许View写入"),
                );
                return None;
            }
            // Unknown表示接受任意类型，由调用方另行检查
            if state.ty != *ty && !state.ty.is_unknown() && !ty.is_unknown() {
                self.mismatch(span, ty, &state.ty);
                return None;
            }
            return Some(BindingRef::State {
                field: name.to_string(),
                setter: ir::setter_name(name),
                ty: state.ty.clone(),
            });
        }
        if screen.computed.contains_key(name) {
            self.error(
                "V3002",
                format!("computed`{name}`是只读的，不能绑定"),
                span,
                "",
            );
            return None;
        }
        self.error("V1010", format!("未定义的state`{name}`"), span, "");
        None
    }
}

fn irop_symbol(op: ir::BinaryOp) -> &'static str {
    match op {
        ir::BinaryOp::Add => "+",
        ir::BinaryOp::Sub => "-",
        ir::BinaryOp::Mul => "*",
        ir::BinaryOp::Div => "/",
        ir::BinaryOp::Rem => "%",
        _ => "?",
    }
}

/// 常量表达式：字面量、列表、枚举成员与nil
fn is_constant(e: &ast::Expr) -> bool {
    match &e.kind {
        A::Int(_)
        | A::Float(..)
        | A::Str(..)
        | A::Bool(_)
        | A::Nil
        | A::ImplicitMember(_)
        | A::Error => true,
        A::Member(base, _) => matches!(base.kind, A::Ident(_)),
        A::Unary(ast::UnaryOp::Neg, inner) => matches!(inner.kind, A::Int(_) | A::Float(..)),
        A::List(items) => items.iter().all(is_constant),
        A::Paren(inner) => is_constant(inner),
        _ => false,
    }
}
