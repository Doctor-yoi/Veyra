// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 视图树检查：组件参数、子节点、修饰符。

use indexmap::IndexMap;
use veyra_diagnostics::{Diagnostic, Span};
use veyra_ir::{self as ir, Builtin, ElementKind, ModifierKind, Type, Value};
use veyra_syntax::ast::{self, Arg, ExprKind as A};

use crate::Checker;
use crate::catalog::{self, ArgKind, ArgSpec, Children};
use crate::expr::{Scope, promote};

impl<'a> Checker<'a> {
    pub(crate) fn view_nodes(
        &mut self,
        sc: &mut Scope,
        nodes: &'a [ast::Node<ast::ViewNode>],
    ) -> Vec<ir::ViewNode> {
        let pruned = self.cfg.prune(nodes, &mut self.diags);
        let mut out = Vec::new();
        for p in pruned {
            if p.debug.is_some() {
                self.unsupported("view中的`debug`条件", p.cond_span);
                continue;
            }
            match p.item {
                ast::ViewNode::Element(el) => {
                    if let Some(e) = self.element(sc, el) {
                        out.push(ir::ViewNode::Element(e));
                    }
                }
                ast::ViewNode::If(node) => out.push(self.if_node(sc, node)),
                ast::ViewNode::Native(n) => self.unsupported("native代码块", n.span),
            }
        }
        out
    }

    fn if_node(&mut self, sc: &mut Scope, node: &'a ast::IfNode) -> ir::ViewNode {
        let cond = self.expr(sc, &node.cond, Some(&Type::Bool));
        let then = self.view_nodes(sc, &node.then);
        let else_ = match &node.else_ {
            None => Vec::new(),
            Some(ast::ElseBranch::Block(items, _)) => self.view_nodes(sc, items),
            Some(ast::ElseBranch::If(nested)) => vec![self.if_node(sc, nested)],
        };
        ir::ViewNode::If { cond, then, else_ }
    }

    fn element(&mut self, sc: &mut Scope, el: &'a ast::Element) -> Option<ir::Element> {
        if el.path.len() > 1 {
            self.unsupported("平台专属组件", el.span);
            return None;
        }
        let name = &el.path[0];
        let Some(builtin) = Builtin::from_name(&name.name) else {
            self.diags.push(
                Diagnostic::error("V1011", format!("未知的组件`{}`", name.name))
                    .with_label(name.span, "")
                    .with_note(format!(
                        "内置组件：{}",
                        Builtin::ALL
                            .iter()
                            .map(|b| b.name())
                            .collect::<Vec<_>>()
                            .join("、")
                    )),
            );
            return None;
        };
        let spec = catalog::element(builtin);
        let what = format!("组件`{}`", name.name);
        let args = self.match_args(sc, &spec.args, &el.args, name.span, &what);

        // 列表元素类型与key
        let mut item = None;
        if spec.children == Children::Item {
            let elem = args
                .get("_")
                .and_then(Value::as_expr)
                .and_then(|e| e.ty.element().cloned());
            if let (Some(elem), Some(key)) = (&elem, args.get("key").and_then(Value::as_key_path)) {
                self.check_key_path(elem, key, el);
            }
            let params = el
                .closure
                .as_ref()
                .map(|c| c.params.as_slice())
                .unwrap_or_default();
            match params {
                [p] => item = Some((p.name.clone(), elem.unwrap_or(Type::Unknown))),
                _ => {
                    let span = el.closure.as_ref().map(|c| c.span).unwrap_or(el.span);
                    self.error(
                        "V2039",
                        format!("{what}的闭包需要一个元素参数"),
                        span,
                        "例如`{ course in ... }`",
                    );
                }
            }
        }

        let children = match (&el.closure, spec.children) {
            (Some(c), Children::None) => {
                self.error("V2034", format!("{what}不接受子组件"), c.span, "");
                Vec::new()
            }
            (None, Children::Required | Children::Item) => {
                self.error(
                    "V2033",
                    format!("{what}需要子组件"),
                    el.span,
                    "请添加`{ ... }`",
                );
                Vec::new()
            }
            (Some(c), Children::Required) => {
                if !c.params.is_empty() {
                    self.error(
                        "V2038",
                        format!("{what}的闭包不接受参数"),
                        c.params[0].span,
                        "",
                    );
                }
                self.view_nodes(sc, &c.body)
            }
            (Some(c), Children::Item) => {
                let pushed = item.is_some();
                if let Some((n, t)) = &item {
                    sc.locals.push((n.clone(), t.clone()));
                }
                let nodes = self.view_nodes(sc, &c.body);
                if pushed {
                    sc.locals.pop();
                }
                nodes
            }
            (None, Children::None) => Vec::new(),
        };

        let modifiers = self.modifiers(sc, &el.modifiers, builtin);
        Some(ir::Element {
            kind: ElementKind::Builtin(builtin),
            args,
            children,
            item,
            modifiers,
        })
    }

    fn check_key_path(&mut self, elem: &Type, key: &str, el: &ast::Element) {
        let span = el
            .args
            .iter()
            .find(|a| a.label.as_ref().is_some_and(|l| l.name == "key"))
            .map(|a| a.value.span);
        let span = span.unwrap_or(el.span);
        match elem {
            Type::Model(m) => {
                let ok = self
                    .models
                    .get(m)
                    .is_some_and(|d| d.fields.iter().any(|f| f.name == key));
                if !ok {
                    self.error("V2037", format!("`{m}`没有字段`{key}`"), span, "");
                }
            }
            Type::Unknown => {}
            other => self.error(
                "V2037",
                format!("列表元素`{other}`不是model，无法指定key"),
                span,
                "",
            ),
        }
    }

    fn modifiers(
        &mut self,
        sc: &mut Scope,
        nodes: &'a [ast::Node<ast::Modifier>],
        owner: Builtin,
    ) -> Vec<ir::Modifier> {
        let pruned = self.cfg.prune(nodes, &mut self.diags);
        let mut out = Vec::new();
        for p in pruned {
            let m = p.item;
            if p.debug.is_some() {
                self.unsupported("修饰符上的`debug`条件", p.cond_span);
                continue;
            }
            if m.path.len() > 1 {
                self.unsupported("平台专属修饰符", m.span);
                continue;
            }
            let name = &m.path[0];
            let Some(kind) = ModifierKind::from_name(&name.name) else {
                self.diags.push(
                    Diagnostic::error("V1012", format!("未知的修饰符`.{}`", name.name))
                        .with_label(name.span, "")
                        .with_note(format!(
                            "内置修饰符：{}",
                            ModifierKind::ALL
                                .iter()
                                .map(|k| k.name())
                                .collect::<Vec<_>>()
                                .join("、")
                        )),
                );
                continue;
            };
            let spec = catalog::modifier(kind);
            if let Some(allowed) = spec.applies_to
                && !allowed.contains(&owner)
            {
                self.diags.push(
                    Diagnostic::error(
                        "V2042",
                        format!("`.{}`不能用于`{}`", name.name, owner.name()),
                    )
                    .with_label(name.span, "")
                    .with_note(format!(
                        "可用于：{}",
                        allowed
                            .iter()
                            .map(|b| b.name())
                            .collect::<Vec<_>>()
                            .join("、")
                    )),
                );
                continue;
            }
            let args = self.match_args(
                sc,
                &spec.args,
                &m.args,
                name.span,
                &format!("修饰符`.{}`", name.name),
            );
            out.push(ir::Modifier { kind, args });
        }
        out
    }

    /// 按参数表匹配实参。
    pub(crate) fn match_args(
        &mut self,
        sc: &Scope,
        specs: &[ArgSpec],
        args: &[Arg],
        span: Span,
        what: &str,
    ) -> IndexMap<String, Value> {
        let mut filled: IndexMap<String, Value> = IndexMap::new();
        let mut positional = specs.iter().filter(|s| s.label == "_");
        for arg in args {
            let spec = match &arg.label {
                Some(l) => match specs.iter().find(|s| s.label == l.name) {
                    Some(s) => s,
                    None => {
                        let labels: Vec<_> = specs
                            .iter()
                            .filter(|s| s.label != "_")
                            .map(|s| s.label)
                            .collect();
                        let mut d =
                            Diagnostic::error("V2030", format!("{what}没有参数`{}`", l.name))
                                .with_label(l.span, "");
                        if !labels.is_empty() {
                            d = d.with_note(format!("可用参数：{}", labels.join("、")));
                        }
                        self.diags.push(d);
                        continue;
                    }
                },
                None => match positional.next() {
                    Some(s) => s,
                    None => {
                        self.error("V2035", format!("{what}有多余的位置参数"), arg.span, "");
                        continue;
                    }
                },
            };
            if filled.contains_key(spec.label) {
                self.error("V2031", format!("参数`{}`重复", spec.label), arg.span, "");
                continue;
            }
            let value = self.arg_value(sc, &spec.kind, &arg.value);
            filled.insert(spec.label.to_string(), value);
        }
        for spec in specs {
            if spec.required && !filled.contains_key(spec.label) {
                let name = if spec.label == "_" {
                    "第1个位置参数".to_string()
                } else {
                    format!("参数`{}`", spec.label)
                };
                self.error("V2032", format!("{what}缺少{name}"), span, "");
            }
        }
        // 按参数表顺序输出，便于后端生成稳定代码
        let mut ordered = IndexMap::new();
        for spec in specs {
            if let Some(v) = filled.shift_remove(spec.label) {
                ordered.insert(spec.label.to_string(), v);
            }
        }
        ordered
    }

    fn arg_value(&mut self, sc: &Scope, kind: &ArgKind, e: &ast::Expr) -> Value {
        match kind {
            ArgKind::Text => Value::Expr(self.expr(sc, e, Some(&Type::String))),
            ArgKind::Expr(t) => Value::Expr(self.expr(sc, e, Some(t))),
            ArgKind::Number => Value::Expr(promote(self.expr(sc, e, Some(&Type::Double)))),
            ArgKind::List => {
                let v = self.expr(sc, e, None);
                if !matches!(v.ty, Type::List(_) | Type::Unknown) {
                    self.error(
                        "V2001",
                        format!("类型不匹配：期望列表，实际为`{}`", v.ty),
                        e.span,
                        "",
                    );
                }
                Value::Expr(v)
            }
            ArgKind::Action => match self.action_ref(sc, e) {
                Some(a) => Value::Action(a),
                None => Value::Expr(ir::Expr::unknown()),
            },
            ArgKind::Binding(t) => match self.binding_ref(sc, e, t) {
                Some(b) => Value::Binding(b),
                None => Value::Expr(ir::Expr::unknown()),
            },
            ArgKind::KeyPath => match &e.kind {
                A::ImplicitMember(id) => Value::KeyPath(id.name.clone()),
                _ => {
                    self.error("V2043", "此处需要字段路径", e.span, "例如`.id`");
                    Value::Expr(ir::Expr::unknown())
                }
            },
            ArgKind::Token(allowed) => match &e.kind {
                A::ImplicitMember(id) if allowed.contains(&id.name.as_str()) => {
                    Value::Token(id.name.clone())
                }
                _ => {
                    self.diags.push(
                        Diagnostic::error("V2044", "无效的取值")
                            .with_label(e.span, "")
                            .with_note(format!(
                                "可用取值：{}",
                                allowed
                                    .iter()
                                    .map(|t| format!(".{t}"))
                                    .collect::<Vec<_>>()
                                    .join("、")
                            )),
                    );
                    Value::Expr(ir::Expr::unknown())
                }
            },
            ArgKind::Dimension => match &e.kind {
                A::ImplicitMember(id) if id.name == "infinity" => {
                    Value::Token("infinity".to_string())
                }
                _ => Value::Expr(promote(self.expr(sc, e, Some(&Type::Double)))),
            },
        }
    }
}
