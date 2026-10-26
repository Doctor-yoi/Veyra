// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 格式化器：按统一风格重新输出语法树，保留注释与声明之间的空行
//!
//! 注释按位置挂在随后的节点前，同一行代码之后的注释保留在行尾
//! 输出只依赖语法树与注释位置，因此格式化结果是幂等的

use veyra_diagnostics::Span;

use crate::Parsed;
use crate::ast::*;
use crate::token::Comment;

const INDENT: &str = "  ";
const MAX_WIDTH: usize = 100;

/// 格式化一个已成功解析的文件
pub fn format(parsed: &Parsed, src: &str) -> String {
    let mut f = Formatter {
        src,
        comments: &parsed.comments,
        next: 0,
        lines: Vec::new(),
        level: 0,
    };
    f.file(&parsed.file);
    let mut out = f.lines.join("\n");
    while out.ends_with("\n\n") {
        out.pop();
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

trait Spanned {
    fn span(&self) -> Span;
}

impl Spanned for Item {
    fn span(&self) -> Span {
        Item::span(self)
    }
}

impl Spanned for Ident {
    fn span(&self) -> Span {
        self.span
    }
}

impl Spanned for Field {
    fn span(&self) -> Span {
        self.span
    }
}

impl Spanned for MethodSig {
    fn span(&self) -> Span {
        self.span
    }
}

impl Spanned for QueryDecl {
    fn span(&self) -> Span {
        self.span
    }
}

impl Spanned for LifecycleHook {
    fn span(&self) -> Span {
        self.span
    }
}

impl Spanned for ViewNode {
    fn span(&self) -> Span {
        ViewNode::span(self)
    }
}

impl Spanned for Modifier {
    fn span(&self) -> Span {
        self.span
    }
}

impl Spanned for ScreenMember {
    fn span(&self) -> Span {
        match self {
            ScreenMember::Inject(d) => d.span,
            ScreenMember::State(b) | ScreenMember::Computed(b) => b.span,
            ScreenMember::Actions(b) | ScreenMember::Effects(b) => b.span,
            ScreenMember::Lifecycle(b) => b.span,
            ScreenMember::View(b) => b.span,
        }
    }
}

fn node_span<T: Spanned>(n: &Node<T>) -> Span {
    match n {
        Node::Item(t) => t.span(),
        Node::Cfg(g) => g.span,
    }
}

/// `}`的位置
fn close_of(span: Span) -> u32 {
    span.end.saturating_sub(1)
}

type Emit<'f, 'a, T> = &'f mut dyn FnMut(&mut Formatter<'a>, &T);

struct Formatter<'a> {
    src: &'a str,
    comments: &'a [Comment],
    next: usize,
    lines: Vec<String>,
    level: usize,
}

impl<'a> Formatter<'a> {
    fn line(&mut self, text: impl AsRef<str>) {
        let text = text.as_ref();
        if text.is_empty() {
            self.lines.push(String::new());
        } else {
            self.lines
                .push(format!("{}{text}", INDENT.repeat(self.level)));
        }
    }

    fn blank(&mut self) {
        if self.lines.last().is_some_and(|l| !l.is_empty()) {
            self.lines.push(String::new());
        }
    }

    fn indented(&mut self, f: impl FnOnce(&mut Self)) {
        self.level += 1;
        f(self);
        self.level -= 1;
    }

    fn width(&self) -> usize {
        self.level * INDENT.len()
    }

    /// 两个位置之间是否有空行
    fn blank_between(&self, a: u32, b: u32) -> bool {
        let (a, b) = (a as usize, b as usize);
        b > a
            && self
                .src
                .get(a..b)
                .is_some_and(|t| t.matches('\n').count() >= 2)
    }

    /// 输出位于`pos`之前的注释，各占一行
    fn leading(&mut self, pos: u32) {
        let mut prev: Option<u32> = None;
        while let Some(c) = self.comments.get(self.next) {
            if c.span.start >= pos {
                break;
            }
            if let Some(p) = prev
                && self.blank_between(p, c.span.start)
            {
                self.blank();
            }
            for l in c.text.lines() {
                self.line(l.trim_end());
            }
            prev = Some(c.span.end);
            self.next += 1;
        }
        if let Some(p) = prev
            && self.blank_between(p, pos)
        {
            self.blank();
        }
    }

    /// 输出紧跟在`end`之后、位于同一行的注释
    fn trailing(&mut self, end: u32) {
        while let Some(c) = self.comments.get(self.next) {
            let between = self
                .src
                .get(end as usize..c.span.start as usize)
                .unwrap_or("\n");
            if !c.trailing || c.span.start < end || between.contains('\n') {
                break;
            }
            if let Some(last) = self.lines.last_mut() {
                last.push_str("  ");
                last.push_str(c.text.trim_end());
            }
            self.next += 1;
        }
    }

    /// 下一个待输出的位置，用于判断空行
    fn next_pos(&self, pos: u32) -> u32 {
        self.comments
            .get(self.next)
            .map(|c| c.span.start.min(pos))
            .unwrap_or(pos)
    }

    fn nodes<T: Spanned>(&mut self, nodes: &[Node<T>], close: Option<u32>, f: Emit<'_, 'a, T>) {
        let mut prev_end: Option<u32> = None;
        for n in nodes {
            let span = node_span(n);
            if let Some(pe) = prev_end
                && self.blank_between(pe, self.next_pos(span.start))
            {
                self.blank();
            }
            self.leading(span.start);
            match n {
                Node::Item(t) => f(self, t),
                Node::Cfg(g) => self.cfg(g, f),
            }
            self.trailing(span.end);
            prev_end = Some(span.end);
        }
        if let Some(c) = close {
            if let Some(pe) = prev_end
                && self
                    .comments
                    .get(self.next)
                    .is_some_and(|x| x.span.start < c)
                && self.blank_between(pe, self.next_pos(c))
            {
                self.blank();
            }
            self.leading(c);
        }
    }

    fn cfg<T: Spanned>(&mut self, g: &CfgGroup<T>, f: Emit<'_, 'a, T>) {
        for (i, b) in g.branches.iter().enumerate() {
            if i > 0 {
                self.leading(b.span.start);
            }
            let head = match (&b.cond, i) {
                (Some(c), 0) => format!("#if {}", cfg_expr(c)),
                (Some(c), _) => format!("#elseif {}", cfg_expr(c)),
                (None, _) => "#else".to_string(),
            };
            self.line(head);
            self.nodes(&b.items, None, f);
        }
        self.leading(g.end_span.start);
        self.line("#endif");
    }

    /// 带花括号的块，空块输出为`{}`
    fn braced<T: Spanned>(
        &mut self,
        head: &str,
        nodes: &[Node<T>],
        span: Span,
        f: Emit<'_, 'a, T>,
    ) {
        let close = close_of(span);
        let has_comment = self
            .comments
            .get(self.next)
            .is_some_and(|c| c.span.start < close);
        if nodes.is_empty() && !has_comment {
            self.line(format!("{head} {{}}"));
            return;
        }
        self.line(format!("{head} {{"));
        self.indented(|f2| f2.nodes(nodes, Some(close), f));
        self.line("}");
    }

    fn file(&mut self, file: &'a File) {
        if let Some(first) = file.imports.first() {
            self.leading(first.span.start);
            if file.imports.len() == 1 {
                self.line(format!("import {}", string(&first.path)));
                self.trailing(first.span.end);
            } else {
                self.line("import (");
                self.indented(|f| {
                    for i in &file.imports {
                        f.leading(i.span.start);
                        f.line(string(&i.path));
                        f.trailing(i.span.end);
                    }
                });
                self.line(")");
            }
            if !file.items.is_empty() {
                self.blank();
            }
        }
        let mut emit = |f: &mut Formatter<'a>, item: &Item| f.item(item);
        self.nodes(&file.items, Some(self.src.len() as u32), &mut emit);
    }

    fn item(&mut self, item: &Item) {
        match item {
            Item::Enum(d) => {
                let cases = flat_idents(&d.cases);
                let single = format!(
                    "enum {} {{ {} }}",
                    d.name.name,
                    cases.as_deref().unwrap_or_default().join(", ")
                );
                let has_comment = self
                    .comments
                    .get(self.next)
                    .is_some_and(|c| c.span.start < d.span.end);
                match cases {
                    Some(_) if !has_comment && single.len() + self.width() <= MAX_WIDTH => {
                        self.line(single)
                    }
                    _ => {
                        let mut emit = |f: &mut Formatter<'a>, c: &Ident| f.line(&c.name);
                        self.braced(
                            &format!("enum {}", d.name.name),
                            &d.cases,
                            d.span,
                            &mut emit,
                        );
                    }
                }
            }
            Item::Model(d) => self.fields(&format!("model {}", d.name.name), &d.fields, d.span),
            Item::Entity(d) => self.fields(
                &format!("entity {}{}", d.name.name, attrs(&d.attrs)),
                &d.fields,
                d.span,
            ),
            Item::Settings(d) => {
                self.fields(&format!("settings {}", d.name.name), &d.fields, d.span)
            }
            Item::Secure(d) => self.fields(&format!("secure {}", d.name.name), &d.fields, d.span),
            Item::Repository(d) => {
                let mut emit = |f: &mut Formatter<'a>, m: &MethodSig| f.line(method(m));
                self.braced(
                    &format!("repository {}", d.name.name),
                    &d.methods,
                    d.span,
                    &mut emit,
                );
            }
            Item::Store(d) => {
                let mut emit = |f: &mut Formatter<'a>, q: &QueryDecl| f.query(q);
                self.braced(
                    &format!("store {} for {}", d.name.name, d.entity.name),
                    &d.queries,
                    d.span,
                    &mut emit,
                );
            }
            Item::Component(d) => {
                let head = if d.params.is_empty() {
                    format!("component {}", d.name.name)
                } else {
                    format!("component {}({})", d.name.name, params(&d.params))
                };
                let mut emit = |f: &mut Formatter<'a>, n: &ViewNode| f.view_node(n);
                self.braced(&head, &d.body, d.span, &mut emit);
            }
            Item::Screen(d) => {
                let mut emit = |f: &mut Formatter<'a>, m: &ScreenMember| f.member(m);
                self.braced(
                    &format!("screen {}", d.name.name),
                    &d.members,
                    d.span,
                    &mut emit,
                );
            }
        }
    }

    fn fields(&mut self, head: &str, fields: &[Node<Field>], span: Span) {
        let mut emit = |f: &mut Formatter<'a>, field: &Field| f.line(field_text(field));
        self.braced(head, fields, span, &mut emit);
    }

    fn query(&mut self, q: &QueryDecl) {
        let head = format!(
            "query {}({}) -> {}",
            q.name.name,
            params(&q.params),
            ty(&q.ret)
        );
        if q.filter.is_none() && q.order.is_empty() && q.limit.is_none() {
            self.line(format!("{head} {{}}"));
            return;
        }
        self.line(format!("{head} {{"));
        self.indented(|f| {
            if let Some(e) = &q.filter {
                f.leading(e.span.start);
                f.line(format!("where {}", expr(e)));
            }
            if let Some(first) = q.order.first() {
                f.leading(first.span.start);
                let keys: Vec<_> = q
                    .order
                    .iter()
                    .map(|k| {
                        format!(
                            "{} {}",
                            k.field.name,
                            if k.descending { "desc" } else { "asc" }
                        )
                    })
                    .collect();
                f.line(format!("orderBy {}", keys.join(", ")));
            }
            if let Some(e) = &q.limit {
                f.leading(e.span.start);
                f.line(format!("limit {}", expr(e)));
            }
            f.leading(close_of(q.span));
        });
        self.line("}");
    }

    fn member(&mut self, m: &ScreenMember) {
        match m {
            ScreenMember::Inject(d) => self.line(format!("inject {}: {}", d.name.name, d.ty.name)),
            ScreenMember::State(b) => self.fields("state", &b.items, b.span),
            ScreenMember::Computed(b) => self.fields("computed", &b.items, b.span),
            ScreenMember::Actions(b) | ScreenMember::Effects(b) => {
                let head = if matches!(m, ScreenMember::Actions(_)) {
                    "actions"
                } else {
                    "effects"
                };
                let mut emit = |f: &mut Formatter<'a>, s: &MethodSig| f.line(method(s));
                self.braced(head, &b.items, b.span, &mut emit);
            }
            ScreenMember::Lifecycle(b) => {
                let mut emit = |f: &mut Formatter<'a>, h: &LifecycleHook| {
                    f.line(format!("{}: {}", h.event.name, expr(&h.action)))
                };
                self.braced("lifecycle", &b.items, b.span, &mut emit);
            }
            ScreenMember::View(b) => {
                let mut emit = |f: &mut Formatter<'a>, n: &ViewNode| f.view_node(n);
                self.braced("view", &b.items, b.span, &mut emit);
            }
        }
    }

    fn view_node(&mut self, n: &ViewNode) {
        match n {
            ViewNode::Element(el) => self.element(el),
            ViewNode::If(node) => self.if_node(node, "if"),
            ViewNode::Native(n) => {
                self.line(format!("native {} {{", n.platform.name));
                self.indented(|f| {
                    f.line("\"\"\"");
                    for l in n.code.lines() {
                        f.line(l);
                    }
                    f.line("\"\"\"");
                });
                self.line("}");
            }
        }
    }

    fn if_node(&mut self, node: &IfNode, keyword: &str) {
        let mut emit = |f: &mut Formatter<'a>, n: &ViewNode| f.view_node(n);
        self.line(format!("{keyword} {} {{", expr(&node.cond)));
        self.indented(|f| f.nodes(&node.then, Some(close_of(node.then_end)), &mut emit));
        match &node.else_ {
            None => self.line("}"),
            Some(ElseBranch::If(nested)) => {
                self.line("}");
                self.continue_else_if(nested);
            }
            Some(ElseBranch::Block(items, end)) => {
                self.line("} else {");
                self.indented(|f| f.nodes(items, Some(close_of(*end)), &mut emit));
                self.line("}");
            }
        }
    }

    /// 把`} else if`接在上一行的右花括号后
    fn continue_else_if(&mut self, node: &IfNode) {
        let mut emit = |f: &mut Formatter<'a>, n: &ViewNode| f.view_node(n);
        if let Some(last) = self.lines.last_mut() {
            last.push_str(&format!(" else if {} {{", expr(&node.cond)));
        }
        self.indented(|f| f.nodes(&node.then, Some(close_of(node.then_end)), &mut emit));
        match &node.else_ {
            None => self.line("}"),
            Some(ElseBranch::If(nested)) => {
                self.line("}");
                self.continue_else_if(nested);
            }
            Some(ElseBranch::Block(items, end)) => {
                self.line("} else {");
                self.indented(|f| f.nodes(items, Some(close_of(*end)), &mut emit));
                self.line("}");
            }
        }
    }

    fn element(&mut self, el: &Element) {
        let mut head = el.path_string();
        if el.has_parens || !el.args.is_empty() || el.closure.is_none() {
            head.push_str(&format!("({})", args(&el.args)));
        }
        let simple_mods: Option<Vec<String>> = el
            .modifiers
            .iter()
            .map(|m| match m {
                Node::Item(m) => Some(modifier(m)),
                Node::Cfg(_) => None,
            })
            .collect();
        match &el.closure {
            Some(c) => {
                let open = if c.params.is_empty() {
                    format!("{head} {{")
                } else {
                    let ps: Vec<_> = c.params.iter().map(|p| p.name.as_str()).collect();
                    format!("{head} {{ {} in", ps.join(", "))
                };
                let close = close_of(c.span);
                let has_comment = self
                    .comments
                    .get(self.next)
                    .is_some_and(|x| x.span.start < close);
                if c.body.is_empty() && !has_comment && c.params.is_empty() {
                    self.line(format!("{head} {{}}"));
                } else {
                    self.line(open);
                    let mut emit = |f: &mut Formatter<'a>, n: &ViewNode| f.view_node(n);
                    self.indented(|f| f.nodes(&c.body, Some(close), &mut emit));
                    self.line("}");
                }
                self.modifier_lines(&el.modifiers);
            }
            None => match simple_mods {
                Some(mods) if mods.len() <= 1 => {
                    let single = format!("{head}{}", mods.concat());
                    if single.len() + self.width() <= MAX_WIDTH {
                        self.line(single);
                    } else {
                        self.line(head);
                        self.indented(|f| f.modifier_lines(&el.modifiers));
                    }
                }
                _ => {
                    self.line(head);
                    self.indented(|f| f.modifier_lines(&el.modifiers));
                }
            },
        }
    }

    fn modifier_lines(&mut self, mods: &[Node<Modifier>]) {
        let mut emit = |f: &mut Formatter<'a>, m: &Modifier| f.line(modifier(m));
        self.nodes(mods, None, &mut emit);
    }
}

fn flat_idents(nodes: &[Node<Ident>]) -> Option<Vec<String>> {
    nodes
        .iter()
        .map(|n| match n {
            Node::Item(i) => Some(i.name.clone()),
            Node::Cfg(_) => None,
        })
        .collect()
}

fn cfg_expr(e: &CfgExpr) -> String {
    match e {
        CfgExpr::Ident(i) => i.name.clone(),
        CfgExpr::Not(inner, _) => format!("!{}", cfg_expr(inner)),
        CfgExpr::And(a, b) => format!("{} && {}", cfg_expr(a), cfg_expr(b)),
        CfgExpr::Or(a, b) => format!("{} || {}", cfg_expr(a), cfg_expr(b)),
        CfgExpr::Paren(inner, _) => format!("({})", cfg_expr(inner)),
    }
}

fn string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn attrs(attrs: &[Attr]) -> String {
    attrs.iter().map(|a| format!(" @{}", a.name.name)).collect()
}

fn ty(t: &TypeExpr) -> String {
    match &t.kind {
        TypeExprKind::Named { name, args } if args.is_empty() => name.name.clone(),
        TypeExprKind::Named { name, args } => {
            format!(
                "{}<{}>",
                name.name,
                args.iter().map(ty).collect::<Vec<_>>().join(", ")
            )
        }
        TypeExprKind::Optional(inner) => format!("{}?", ty(inner)),
    }
}

fn field_text(f: &Field) -> String {
    let mut out = format!("{}: {}", f.name.name, ty(&f.ty));
    if let Some(d) = &f.default {
        out.push_str(&format!(" = {}", expr(d)));
    }
    out.push_str(&attrs(&f.attrs));
    out
}

fn params(ps: &[Param]) -> String {
    ps.iter()
        .map(|p| format!("{}: {}", p.name.name, ty(&p.ty)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn method(m: &MethodSig) -> String {
    let mut out = format!("{}({})", m.name.name, params(&m.params));
    if m.is_async {
        out.push_str(" async");
    }
    if m.throws {
        out.push_str(" throws");
    }
    if let Some(r) = &m.ret {
        out.push_str(&format!(" -> {}", ty(r)));
    }
    out
}

fn args(args: &[Arg]) -> String {
    args.iter()
        .map(|a| match &a.label {
            Some(l) => format!("{}: {}", l.name, expr(&a.value)),
            None => expr(&a.value),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn modifier(m: &Modifier) -> String {
    format!(".{}({})", path_string(&m.path), args(&m.args))
}

/// 表达式按原有结构输出，括号只来自源码中的`Paren`
pub(crate) fn expr(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Int(v) => v.to_string(),
        ExprKind::Float(_, raw) => raw.clone(),
        ExprKind::Str(_, raw) => raw.clone(),
        ExprKind::Bool(b) => b.to_string(),
        ExprKind::Nil => "nil".into(),
        ExprKind::Ident(n) => n.clone(),
        ExprKind::ImplicitMember(i) => format!(".{}", i.name),
        ExprKind::Member(b, n) => format!("{}.{}", expr(b), n.name),
        ExprKind::Call(c, a) => format!("{}({})", expr(c), args(a)),
        ExprKind::Unary(op, inner) => {
            let sym = match op {
                UnaryOp::Not => "!",
                UnaryOp::Neg => "-",
            };
            format!("{sym}{}", expr(inner))
        }
        ExprKind::Binary(op, l, r) => format!("{} {} {}", expr(l), op.symbol(), expr(r)),
        ExprKind::Ternary(c, a, b) => format!("{} ? {} : {}", expr(c), expr(a), expr(b)),
        ExprKind::List(items) => format!(
            "[{}]",
            items.iter().map(expr).collect::<Vec<_>>().join(", ")
        ),
        ExprKind::Paren(inner) => format!("({})", expr(inner)),
        ExprKind::Error => String::new(),
    }
}
