// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 递归下降解析器。语法对换行不敏感，出错时跳到下一行或块尾继续解析

use veyra_diagnostics::{Diagnostic, Diagnostics, FileId, Span};

use crate::ast::*;
use crate::lexer::{dedent_raw, unescape};
use crate::token::{Token, TokenKind as T};

pub(crate) struct Parser<'a> {
    file: FileId,
    src: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    pub(crate) diags: Diagnostics,
}

type ItemFn<'f, 'a, U> = &'f mut dyn FnMut(&mut Parser<'a>) -> Option<U>;

impl<'a> Parser<'a> {
    pub(crate) fn new(file: FileId, src: &'a str, tokens: Vec<Token>) -> Self {
        Self {
            file,
            src,
            tokens,
            pos: 0,
            diags: Diagnostics::new(),
        }
    }

    // ---- 基础操作 ----

    fn peek(&self) -> Token {
        self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_at(&self, n: usize) -> Token {
        self.tokens[(self.pos + n).min(self.tokens.len() - 1)]
    }

    fn at(&self, kind: T) -> bool {
        self.peek().kind == kind
    }

    fn text(&self, token: Token) -> &'a str {
        &self.src[token.span.start as usize..token.span.end as usize]
    }

    fn at_kw(&self, kw: &str) -> bool {
        let t = self.peek();
        t.kind == T::Ident && self.text(t) == kw
    }

    fn bump(&mut self) -> Token {
        let t = self.peek();
        if t.kind != T::Eof {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, kind: T) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.at_kw(kw) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn prev_span(&self) -> Span {
        if self.pos == 0 {
            self.peek().span
        } else {
            self.tokens[self.pos - 1].span
        }
    }

    fn error_here(&mut self, expected: &str) {
        let t = self.peek();
        let found = if t.kind == T::Ident || t.kind == T::Int {
            format!("`{}`", self.text(t))
        } else {
            t.kind.describe().to_string()
        };
        self.diags.push(
            Diagnostic::error("V0010", format!("期望{expected}，实际为{found}"))
                .with_label(t.span, format!("此处期望{expected}")),
        );
    }

    fn expect(&mut self, kind: T) -> Option<Token> {
        if self.at(kind) {
            Some(self.bump())
        } else {
            self.error_here(kind.describe());
            None
        }
    }

    fn ident(&mut self, what: &str) -> Option<Ident> {
        let t = self.peek();
        if t.kind == T::Ident {
            self.bump();
            Some(Ident {
                name: self.text(t).to_string(),
                span: t.span,
            })
        } else {
            self.error_here(what);
            None
        }
    }

    /// 跳过一段内容直到下一行或给定的结束符，括号内的内容整体跳过
    fn recover(&mut self, end: Option<T>) {
        let start = self.pos;
        loop {
            let t = self.peek();
            if t.kind == T::Eof || Some(t.kind) == end {
                break;
            }
            if self.pos > start && t.newline_before {
                break;
            }
            if matches!(
                t.kind,
                T::HashIf | T::HashElseIf | T::HashElse | T::HashEndIf
            ) && self.pos > start
            {
                break;
            }
            match t.kind {
                T::LBrace | T::LParen | T::LBracket => self.skip_group(),
                _ => {
                    self.bump();
                }
            }
        }
    }

    fn skip_group(&mut self) {
        let mut depth = 0usize;
        loop {
            let t = self.bump();
            match t.kind {
                T::LBrace | T::LParen | T::LBracket => depth += 1,
                T::RBrace | T::RParen | T::RBracket => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                T::Eof => break,
                _ => {}
            }
        }
    }

    // ---- 条件编译与节点列表 ----

    /// 解析节点序列，遇到`end`或同层的`#elseif/#else/#endif`时停止
    fn nodes<U>(&mut self, end: Option<T>, f: ItemFn<'_, 'a, U>) -> Vec<Node<U>> {
        let mut out = Vec::new();
        loop {
            while self.eat(T::Semi) || self.eat(T::Comma) {}
            let t = self.peek();
            if t.kind == T::Eof || Some(t.kind) == end {
                break;
            }
            match t.kind {
                T::HashElseIf | T::HashElse | T::HashEndIf => break,
                T::HashIf => {
                    if let Some(group) = self.cfg_group(end, f) {
                        out.push(Node::Cfg(group));
                    }
                }
                _ => {
                    let before = self.pos;
                    let errors = self.diags.error_count();
                    match f(self) {
                        Some(item) => out.push(Node::Item(item)),
                        None => {
                            if self.diags.error_count() == errors {
                                self.error_here("声明");
                            }
                            if self.pos == before {
                                self.bump();
                            }
                            self.recover(end);
                        }
                    }
                }
            }
        }
        out
    }

    fn cfg_group<U>(&mut self, end: Option<T>, f: ItemFn<'_, 'a, U>) -> Option<CfgGroup<U>> {
        let start = self.bump().span;
        let mut branches = Vec::new();
        let cond = self.cfg_expr()?;
        let items = self.nodes(end, f);
        branches.push(CfgBranch {
            cond: Some(cond),
            items,
            span: start,
        });
        let mut seen_else = false;
        loop {
            let t = self.peek();
            match t.kind {
                T::HashElseIf => {
                    self.bump();
                    if seen_else {
                        self.diags.push(
                            Diagnostic::error("V0011", "`#elseif`不能出现在`#else`之后")
                                .with_label(t.span, ""),
                        );
                    }
                    let cond = self.cfg_expr()?;
                    let items = self.nodes(end, f);
                    branches.push(CfgBranch {
                        cond: Some(cond),
                        items,
                        span: t.span,
                    });
                }
                T::HashElse => {
                    self.bump();
                    if seen_else {
                        self.diags.push(
                            Diagnostic::error("V0011", "重复的`#else`").with_label(t.span, ""),
                        );
                    }
                    seen_else = true;
                    let items = self.nodes(end, f);
                    branches.push(CfgBranch {
                        cond: None,
                        items,
                        span: t.span,
                    });
                }
                T::HashEndIf => {
                    let end_span = self.bump().span;
                    return Some(CfgGroup {
                        branches,
                        span: start.to(end_span),
                        end_span,
                    });
                }
                _ => {
                    self.diags.push(
                        Diagnostic::error("V0012", "`#if`缺少对应的`#endif`")
                            .with_label(start, "条件块从这里开始")
                            .with_secondary(t.span, "在此之前应出现`#endif`"),
                    );
                    let end_span = self.prev_span();
                    return Some(CfgGroup {
                        branches,
                        span: start.to(end_span),
                        end_span,
                    });
                }
            }
        }
    }

    fn cfg_expr(&mut self) -> Option<CfgExpr> {
        let mut lhs = self.cfg_and()?;
        while self.eat(T::OrOr) {
            let rhs = self.cfg_and()?;
            lhs = CfgExpr::Or(Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }

    fn cfg_and(&mut self) -> Option<CfgExpr> {
        let mut lhs = self.cfg_atom()?;
        while self.eat(T::AndAnd) {
            let rhs = self.cfg_atom()?;
            lhs = CfgExpr::And(Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }

    fn cfg_atom(&mut self) -> Option<CfgExpr> {
        let t = self.peek();
        match t.kind {
            T::Bang => {
                self.bump();
                let inner = self.cfg_atom()?;
                let span = t.span.to(inner.span());
                Some(CfgExpr::Not(Box::new(inner), span))
            }
            T::LParen => {
                self.bump();
                let inner = self.cfg_expr()?;
                let end = self.expect(T::RParen)?.span;
                Some(CfgExpr::Paren(Box::new(inner), t.span.to(end)))
            }
            _ => self.ident("条件名称").map(CfgExpr::Ident),
        }
    }

    // ---- 顶层声明 ----

    pub(crate) fn file(&mut self) -> File {
        let mut f = |p: &mut Parser<'a>| p.item();
        let items = self.nodes(None, &mut f);
        if !self.at(T::Eof) {
            let t = self.peek();
            self.diags.push(
                Diagnostic::error("V0013", format!("多余的{}", t.kind.describe()))
                    .with_label(t.span, "没有与之匹配的`#if`"),
            );
        }
        File {
            id: self.file,
            items,
        }
    }

    fn item(&mut self) -> Option<Item> {
        let t = self.peek();
        if t.kind != T::Ident {
            self.error_here("声明关键字");
            return None;
        }
        let kw = self.text(t);
        let item = match kw {
            "enum" => Item::Enum(self.enum_decl()?),
            "model" => Item::Model(self.model_decl()?),
            "entity" => Item::Entity(self.entity_decl()?),
            "repository" => Item::Repository(self.repository_decl()?),
            "store" => Item::Store(self.store_decl()?),
            "settings" => Item::Settings(self.fields_decl()?),
            "secure" => Item::Secure(self.fields_decl()?),
            "component" => Item::Component(self.component_decl()?),
            "screen" => Item::Screen(self.screen_decl()?),
            _ => {
                self.diags.push(
                    Diagnostic::error("V0014", format!("未知的声明`{kw}`"))
                        .with_label(t.span, "")
                        .with_note("可用的声明：enum、model、entity、repository、store、settings、secure、component、screen"),
                );
                return None;
            }
        };
        Some(item)
    }

    fn braced<U>(&mut self, f: ItemFn<'_, 'a, U>) -> Option<(Vec<Node<U>>, Span)> {
        self.expect(T::LBrace)?;
        let items = self.nodes(Some(T::RBrace), f);
        let end = self.expect(T::RBrace)?.span;
        Some((items, end))
    }

    fn enum_decl(&mut self) -> Option<EnumDecl> {
        let start = self.bump().span;
        let name = self.ident("枚举名称")?;
        let mut f = |p: &mut Parser<'a>| p.ident("枚举成员");
        let (cases, end) = self.braced(&mut f)?;
        Some(EnumDecl {
            name,
            cases,
            span: start.to(end),
        })
    }

    fn model_decl(&mut self) -> Option<ModelDecl> {
        let start = self.bump().span;
        let name = self.ident("模型名称")?;
        let mut f = |p: &mut Parser<'a>| p.field();
        let (fields, end) = self.braced(&mut f)?;
        Some(ModelDecl {
            name,
            fields,
            span: start.to(end),
        })
    }

    fn entity_decl(&mut self) -> Option<EntityDecl> {
        let start = self.bump().span;
        let name = self.ident("实体名称")?;
        let attrs = self.attrs();
        let mut f = |p: &mut Parser<'a>| p.field();
        let (fields, end) = self.braced(&mut f)?;
        Some(EntityDecl {
            name,
            attrs,
            fields,
            span: start.to(end),
        })
    }

    fn fields_decl(&mut self) -> Option<FieldsDecl> {
        let start = self.bump().span;
        let name = self.ident("名称")?;
        let mut f = |p: &mut Parser<'a>| p.field();
        let (fields, end) = self.braced(&mut f)?;
        Some(FieldsDecl {
            name,
            fields,
            span: start.to(end),
        })
    }

    fn repository_decl(&mut self) -> Option<RepositoryDecl> {
        let start = self.bump().span;
        let name = self.ident("repository名称")?;
        let mut f = |p: &mut Parser<'a>| p.method_sig();
        let (methods, end) = self.braced(&mut f)?;
        Some(RepositoryDecl {
            name,
            methods,
            span: start.to(end),
        })
    }

    fn store_decl(&mut self) -> Option<StoreDecl> {
        let start = self.bump().span;
        let name = self.ident("store名称")?;
        if !self.eat_kw("for") {
            self.error_here("`for`");
            return None;
        }
        let entity = self.ident("实体名称")?;
        let mut f = |p: &mut Parser<'a>| p.query_decl();
        let (queries, end) = self.braced(&mut f)?;
        Some(StoreDecl {
            name,
            entity,
            queries,
            span: start.to(end),
        })
    }

    fn query_decl(&mut self) -> Option<QueryDecl> {
        let start = self.peek().span;
        if !self.eat_kw("query") {
            self.error_here("`query`");
            return None;
        }
        let name = self.ident("查询名称")?;
        let params = self.params()?;
        self.expect(T::Arrow)?;
        let ret = self.type_expr()?;
        self.expect(T::LBrace)?;
        let mut filter = None;
        let mut order = Vec::new();
        let mut limit = None;
        while !self.at(T::RBrace) && !self.at(T::Eof) {
            if self.eat_kw("where") {
                filter = Some(self.expr()?);
            } else if self.eat_kw("orderBy") {
                loop {
                    let field = self.ident("排序字段")?;
                    let mut descending = false;
                    let mut span = field.span;
                    if self.at_kw("asc") || self.at_kw("desc") {
                        let t = self.bump();
                        descending = self.text(t) == "desc";
                        span = span.to(t.span);
                    }
                    order.push(OrderKey {
                        field,
                        descending,
                        span,
                    });
                    if !self.eat(T::Comma) {
                        break;
                    }
                }
            } else if self.eat_kw("limit") {
                limit = Some(self.expr()?);
            } else {
                self.error_here("`where`、`orderBy`或`limit`");
                self.recover(Some(T::RBrace));
            }
        }
        let end = self.expect(T::RBrace)?.span;
        Some(QueryDecl {
            name,
            params,
            ret,
            filter,
            order,
            limit,
            span: start.to(end),
        })
    }

    fn component_decl(&mut self) -> Option<ComponentDecl> {
        let start = self.bump().span;
        let name = self.ident("组件名称")?;
        let params = if self.at(T::LParen) {
            self.params()?
        } else {
            Vec::new()
        };
        let mut f = |p: &mut Parser<'a>| p.view_node();
        let (body, end) = self.braced(&mut f)?;
        Some(ComponentDecl {
            name,
            params,
            body,
            span: start.to(end),
        })
    }

    fn screen_decl(&mut self) -> Option<ScreenDecl> {
        let start = self.bump().span;
        let name = self.ident("screen名称")?;
        let mut f = |p: &mut Parser<'a>| p.screen_member();
        let (members, end) = self.braced(&mut f)?;
        Some(ScreenDecl {
            name,
            members,
            span: start.to(end),
        })
    }

    fn screen_member(&mut self) -> Option<ScreenMember> {
        let t = self.peek();
        if t.kind != T::Ident {
            self.error_here("screen成员");
            return None;
        }
        let kw = self.text(t);
        match kw {
            "inject" => {
                self.bump();
                let name = self.ident("注入名称")?;
                self.expect(T::Colon)?;
                let ty = self.ident("类型名称")?;
                Some(ScreenMember::Inject(InjectDecl {
                    span: t.span.to(ty.span),
                    name,
                    ty,
                }))
            }
            "state" => self.block(|p| p.field()).map(ScreenMember::State),
            "computed" => self.block(|p| p.field()).map(ScreenMember::Computed),
            "actions" => self.block(|p| p.method_sig()).map(ScreenMember::Actions),
            "effects" => self.block(|p| p.method_sig()).map(ScreenMember::Effects),
            "lifecycle" => self
                .block(|p| p.lifecycle_hook())
                .map(ScreenMember::Lifecycle),
            "view" => self.block(|p| p.view_node()).map(ScreenMember::View),
            _ => {
                self.diags.push(
                    Diagnostic::error("V0015", format!("未知的screen成员`{kw}`"))
                        .with_label(t.span, "")
                        .with_note("可用的成员：inject、state、computed、actions、effects、lifecycle、view"),
                );
                None
            }
        }
    }

    fn block<U>(&mut self, mut f: impl FnMut(&mut Parser<'a>) -> Option<U>) -> Option<Block<U>> {
        let keyword = self.bump().span;
        let (items, end) = self.braced(&mut f)?;
        Some(Block {
            keyword,
            items,
            span: keyword.to(end),
        })
    }

    fn lifecycle_hook(&mut self) -> Option<LifecycleHook> {
        let event = self.ident("生命周期事件")?;
        self.expect(T::Colon)?;
        let action = self.expr()?;
        Some(LifecycleHook {
            span: event.span.to(action.span),
            event,
            action,
        })
    }

    fn attrs(&mut self) -> Vec<Attr> {
        let mut attrs = Vec::new();
        while self.at(T::At) {
            let at = self.bump().span;
            if let Some(name) = self.ident("属性名称") {
                attrs.push(Attr {
                    span: at.to(name.span),
                    name,
                });
            }
        }
        attrs
    }

    fn field(&mut self) -> Option<Field> {
        let name = self.ident("字段名称")?;
        self.expect(T::Colon)?;
        let ty = self.type_expr()?;
        let default = if self.eat(T::Eq) {
            Some(self.expr()?)
        } else {
            None
        };
        let attrs = self.attrs();
        let end = self.prev_span();
        Some(Field {
            span: name.span.to(end),
            name,
            ty,
            default,
            attrs,
        })
    }

    fn params(&mut self) -> Option<Vec<Param>> {
        self.expect(T::LParen)?;
        let mut params = Vec::new();
        while !self.at(T::RParen) && !self.at(T::Eof) {
            let name = self.ident("参数名称")?;
            self.expect(T::Colon)?;
            let ty = self.type_expr()?;
            params.push(Param {
                span: name.span.to(ty.span),
                name,
                ty,
            });
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.expect(T::RParen)?;
        Some(params)
    }

    fn method_sig(&mut self) -> Option<MethodSig> {
        let name = self.ident("方法名称")?;
        let params = if self.at(T::LParen) {
            self.params()?
        } else {
            Vec::new()
        };
        let is_async = self.eat_kw("async");
        let throws = self.eat_kw("throws");
        let ret = if self.eat(T::Arrow) {
            Some(self.type_expr()?)
        } else {
            None
        };
        let end = self.prev_span();
        Some(MethodSig {
            span: name.span.to(end),
            name,
            params,
            is_async,
            throws,
            ret,
        })
    }

    fn type_expr(&mut self) -> Option<TypeExpr> {
        let name = self.ident("类型")?;
        let mut args = Vec::new();
        let mut span = name.span;
        if self.eat(T::Lt) {
            loop {
                args.push(self.type_expr()?);
                if !self.eat(T::Comma) {
                    break;
                }
            }
            span = span.to(self.expect(T::Gt)?.span);
        }
        let mut ty = TypeExpr {
            kind: TypeExprKind::Named { name, args },
            span,
        };
        while self.at(T::Question) {
            let q = self.bump().span;
            let span = ty.span.to(q);
            ty = TypeExpr {
                kind: TypeExprKind::Optional(Box::new(ty)),
                span,
            };
        }
        Some(ty)
    }

    // ---- 视图 ----

    fn view_node(&mut self) -> Option<ViewNode> {
        if self.at_kw("if") {
            return self.if_node().map(ViewNode::If);
        }
        if self.at_kw("native") {
            return self.native_node().map(ViewNode::Native);
        }
        if !self.at(T::Ident) {
            self.error_here("组件");
            return None;
        }
        self.element().map(ViewNode::Element)
    }

    fn if_node(&mut self) -> Option<IfNode> {
        let start = self.bump().span;
        let cond = self.expr()?;
        let mut f = |p: &mut Parser<'a>| p.view_node();
        let (then, then_end) = self.braced(&mut f)?;
        let mut end = then_end;
        let else_ = if self.eat_kw("else") {
            if self.at_kw("if") {
                let nested = self.if_node()?;
                end = nested.span;
                Some(ElseBranch::If(Box::new(nested)))
            } else {
                let (items, e) = self.braced(&mut f)?;
                end = e;
                Some(ElseBranch::Block(items, e))
            }
        } else {
            None
        };
        Some(IfNode {
            cond,
            then,
            then_end,
            else_,
            span: start.to(end),
        })
    }

    fn native_node(&mut self) -> Option<NativeNode> {
        let start = self.bump().span;
        let platform = self.ident("平台名称")?;
        self.expect(T::LBrace)?;
        let t = self.expect(T::RawStr)?;
        let raw = self.text(t).to_string();
        let end = self.expect(T::RBrace)?.span;
        Some(NativeNode {
            platform,
            code: dedent_raw(&raw),
            raw,
            code_span: t.span,
            span: start.to(end),
        })
    }

    fn element(&mut self) -> Option<Element> {
        let first = self.ident("组件名称")?;
        let start = first.span;
        let mut path = vec![first];
        while self.at(T::Dot) && self.peek_at(1).kind == T::Ident && !self.peek().newline_before {
            self.bump();
            path.push(self.ident("组件名称")?);
        }
        let mut has_parens = false;
        let mut args = Vec::new();
        if self.at(T::LParen) {
            has_parens = true;
            args = self.args()?;
        }
        let closure = if self.at(T::LBrace) {
            Some(self.closure()?)
        } else {
            None
        };
        let modifiers = self.modifiers();
        let end = self.prev_span();
        Some(Element {
            path,
            args,
            has_parens,
            closure,
            modifiers,
            span: start.to(end),
        })
    }

    fn closure(&mut self) -> Option<Closure> {
        let start = self.expect(T::LBrace)?.span;
        let mut params = Vec::new();
        // 识别`a, b in`形式的参数列表
        let mut n = 0;
        let mut looks_like_params = false;
        while self.peek_at(n).kind == T::Ident {
            if self.text(self.peek_at(n + 1)) == "in" && self.peek_at(n + 1).kind == T::Ident {
                looks_like_params = true;
                break;
            }
            if self.peek_at(n + 1).kind != T::Comma {
                break;
            }
            n += 2;
        }
        if looks_like_params {
            loop {
                params.push(self.ident("参数名称")?);
                if !self.eat(T::Comma) {
                    break;
                }
            }
            self.eat_kw("in");
        }
        let mut f = |p: &mut Parser<'a>| p.view_node();
        let body = self.nodes(Some(T::RBrace), &mut f);
        let end = self.expect(T::RBrace)?.span;
        Some(Closure {
            params,
            body,
            span: start.to(end),
        })
    }

    fn modifiers(&mut self) -> Vec<Node<Modifier>> {
        let mut out = Vec::new();
        loop {
            if self.at(T::Dot) {
                match self.modifier() {
                    Some(m) => out.push(Node::Item(m)),
                    None => {
                        self.recover(None);
                        break;
                    }
                }
            } else if self.at(T::HashIf) && self.cfg_wraps_modifier() {
                let mut f = |p: &mut Parser<'a>| {
                    if p.at(T::Dot) {
                        p.modifier()
                    } else {
                        p.error_here("修饰符");
                        None
                    }
                };
                if let Some(group) = self.cfg_group(None, &mut f) {
                    out.push(Node::Cfg(group));
                }
            } else {
                break;
            }
        }
        out
    }

    /// 预读判断`#if`后是否为修饰符
    fn cfg_wraps_modifier(&mut self) -> bool {
        let saved_pos = self.pos;
        let saved = std::mem::take(&mut self.diags);
        self.bump();
        let ok = self.cfg_expr().is_some() && self.at(T::Dot);
        self.pos = saved_pos;
        self.diags = saved;
        ok
    }

    fn modifier(&mut self) -> Option<Modifier> {
        let start = self.expect(T::Dot)?.span;
        let mut path = vec![self.ident("修饰符名称")?];
        while self.at(T::Dot) {
            self.bump();
            path.push(self.ident("修饰符名称")?);
        }
        let args = if self.at(T::LParen) {
            self.args()?
        } else {
            self.error_here("`(`");
            return None;
        };
        Some(Modifier {
            path,
            args,
            span: start.to(self.prev_span()),
        })
    }

    fn args(&mut self) -> Option<Vec<Arg>> {
        self.expect(T::LParen)?;
        let mut args = Vec::new();
        while !self.at(T::RParen) && !self.at(T::Eof) {
            let label = if self.at(T::Ident) && self.peek_at(1).kind == T::Colon {
                let l = self.ident("参数标签")?;
                self.bump();
                Some(l)
            } else {
                None
            };
            let value = self.expr()?;
            let start = label.as_ref().map(|l| l.span).unwrap_or(value.span);
            args.push(Arg {
                span: start.to(value.span),
                label,
                value,
            });
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.expect(T::RParen)?;
        Some(args)
    }

    // ---- 表达式 ----

    pub(crate) fn expr(&mut self) -> Option<Expr> {
        let cond = self.binary(0)?;
        if !self.at(T::Question) {
            return Some(cond);
        }
        self.bump();
        let then = self.expr()?;
        self.expect(T::Colon)?;
        let else_ = self.expr()?;
        let span = cond.span.to(else_.span);
        Some(Expr {
            kind: ExprKind::Ternary(Box::new(cond), Box::new(then), Box::new(else_)),
            span,
        })
    }

    fn binary_op(&self) -> Option<BinaryOp> {
        Some(match self.peek().kind {
            T::QuestionQuestion => BinaryOp::Coalesce,
            T::OrOr => BinaryOp::Or,
            T::AndAnd => BinaryOp::And,
            T::EqEq => BinaryOp::Eq,
            T::NotEq => BinaryOp::Ne,
            T::Lt => BinaryOp::Lt,
            T::Le => BinaryOp::Le,
            T::Gt => BinaryOp::Gt,
            T::Ge => BinaryOp::Ge,
            T::Plus => BinaryOp::Add,
            T::Minus => BinaryOp::Sub,
            T::Star => BinaryOp::Mul,
            T::Slash => BinaryOp::Div,
            T::Percent => BinaryOp::Rem,
            _ => return None,
        })
    }

    fn binary(&mut self, min_prec: u8) -> Option<Expr> {
        let mut lhs = self.unary()?;
        while let Some(op) = self.binary_op() {
            let prec = op.precedence();
            if prec < min_prec || prec == 0 {
                break;
            }
            self.bump();
            // `??`右结合，其余左结合
            let next = if op == BinaryOp::Coalesce {
                prec
            } else {
                prec + 1
            };
            let rhs = self.binary(next)?;
            let span = lhs.span.to(rhs.span);
            lhs = Expr {
                kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)),
                span,
            };
        }
        Some(lhs)
    }

    fn unary(&mut self) -> Option<Expr> {
        let t = self.peek();
        let op = match t.kind {
            T::Bang => UnaryOp::Not,
            T::Minus => UnaryOp::Neg,
            _ => return self.postfix(),
        };
        self.bump();
        let inner = self.unary()?;
        let span = t.span.to(inner.span);
        Some(Expr {
            kind: ExprKind::Unary(op, Box::new(inner)),
            span,
        })
    }

    fn postfix(&mut self) -> Option<Expr> {
        let mut e = self.primary()?;
        loop {
            if self.at(T::Dot) && self.peek_at(1).kind == T::Ident {
                self.bump();
                let name = self.ident("成员名称")?;
                let span = e.span.to(name.span);
                e = Expr {
                    kind: ExprKind::Member(Box::new(e), name),
                    span,
                };
            } else if self.at(T::LParen) && !self.peek().newline_before {
                let args = self.args()?;
                let span = e.span.to(self.prev_span());
                e = Expr {
                    kind: ExprKind::Call(Box::new(e), args),
                    span,
                };
            } else {
                break;
            }
        }
        Some(e)
    }

    fn primary(&mut self) -> Option<Expr> {
        let t = self.peek();
        let text = self.text(t);
        let kind = match t.kind {
            T::Int => {
                self.bump();
                match text.replace('_', "").parse::<i64>() {
                    Ok(v) => ExprKind::Int(v),
                    Err(_) => {
                        self.diags.push(
                            Diagnostic::error("V0020", "整数超出范围").with_label(t.span, ""),
                        );
                        ExprKind::Error
                    }
                }
            }
            T::Float => {
                self.bump();
                ExprKind::Float(
                    text.replace('_', "").parse().unwrap_or(0.0),
                    text.to_string(),
                )
            }
            T::Str => {
                self.bump();
                match unescape(text) {
                    Ok(s) => ExprKind::Str(s, text.to_string()),
                    Err(msg) => {
                        self.diags
                            .push(Diagnostic::error("V0021", msg).with_label(t.span, ""));
                        ExprKind::Error
                    }
                }
            }
            T::Ident => {
                self.bump();
                match text {
                    "true" => ExprKind::Bool(true),
                    "false" => ExprKind::Bool(false),
                    "nil" => ExprKind::Nil,
                    _ => ExprKind::Ident(text.to_string()),
                }
            }
            T::Dot => {
                self.bump();
                let name = self.ident("成员名称")?;
                return Some(Expr {
                    span: t.span.to(name.span),
                    kind: ExprKind::ImplicitMember(name),
                });
            }
            T::LParen => {
                self.bump();
                let inner = self.expr()?;
                let end = self.expect(T::RParen)?.span;
                return Some(Expr {
                    kind: ExprKind::Paren(Box::new(inner)),
                    span: t.span.to(end),
                });
            }
            T::LBracket => {
                self.bump();
                let mut items = Vec::new();
                while !self.at(T::RBracket) && !self.at(T::Eof) {
                    items.push(self.expr()?);
                    if !self.eat(T::Comma) {
                        break;
                    }
                }
                let end = self.expect(T::RBracket)?.span;
                return Some(Expr {
                    kind: ExprKind::List(items),
                    span: t.span.to(end),
                });
            }
            _ => {
                self.error_here("表达式");
                return None;
            }
        };
        Some(Expr { kind, span: t.span })
    }
}
