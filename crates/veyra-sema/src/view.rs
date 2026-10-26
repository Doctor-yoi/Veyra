// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 视图树检查：组件参数、子节点、修饰符

use indexmap::IndexMap;
use veyra_diagnostics::{Diagnostic, Span};
use veyra_ir::{self as ir, Builtin, ComponentParamType, ElementKind, ModifierKind, Type, Value};
use veyra_syntax::ast::{self, Arg, ExprKind as A};

use crate::catalog::{self, ArgKind, ArgSpec, Children};
use crate::expr::{Scope, promote};
use crate::{Checker, NativeLint};

impl<'a> Checker<'a> {
    pub(crate) fn view_nodes(
        &mut self,
        sc: &mut Scope,
        nodes: &'a [ast::Node<ast::ViewNode>],
    ) -> Vec<ir::ViewNode> {
        let pruned = self.cfg.prune(nodes, &mut self.diags);
        let mut items: Vec<(Option<bool>, ir::ViewNode)> = Vec::new();
        for p in pruned {
            let node = match p.item {
                ast::ViewNode::Element(el) => self.element(sc, el).map(ir::ViewNode::Element),
                ast::ViewNode::If(node) => Some(self.if_node(sc, node)),
                ast::ViewNode::Native(n) => self.native(sc, n),
            };
            if let Some(node) = node {
                items.push((p.debug, node));
            }
        }
        group_debug(items)
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
            return self.platform_element(sc, el);
        }
        let name = &el.path[0];
        if self.components.contains_key(&name.name) {
            self.lookup(&name.name, name.span);
            return self.component_instance(sc, el);
        }
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

        let modifiers = self.modifiers(sc, &el.modifiers, Some(builtin));
        Some(ir::Element {
            kind: ElementKind::Builtin(builtin),
            args,
            children,
            item,
            modifiers,
        })
    }

    /// 自定义组件的实例，参数一律按名称传递
    fn component_instance(&mut self, sc: &mut Scope, el: &'a ast::Element) -> Option<ir::Element> {
        let name = &el.path[0];
        let params = self.components.get(&name.name).cloned().unwrap_or_default();
        if let Some(c) = &el.closure {
            self.error(
                "V2034",
                format!("组件`{}`不接受子组件", name.name),
                c.span,
                "",
            );
        }
        let mut args: IndexMap<String, Value> = IndexMap::new();
        for arg in &el.args {
            let Some(label) = &arg.label else {
                self.diags.push(
                    Diagnostic::error("V2073", "自定义组件的参数必须带名称")
                        .with_label(arg.span, "")
                        .with_note(format!(
                            "参数：{}",
                            params
                                .iter()
                                .map(|p| p.name.as_str())
                                .collect::<Vec<_>>()
                                .join("、")
                        )),
                );
                continue;
            };
            let Some(p) = params.iter().find(|p| p.name == label.name) else {
                self.error(
                    "V2030",
                    format!("组件`{}`没有参数`{}`", name.name, label.name),
                    label.span,
                    "",
                );
                continue;
            };
            if args.contains_key(&p.name) {
                self.error("V2031", format!("参数`{}`重复", p.name), arg.span, "");
                continue;
            }
            let kind = match &p.ty {
                ComponentParamType::Value(t) if *t == Type::Double => ArgKind::Number,
                ComponentParamType::Value(t) => ArgKind::Expr(t.clone()),
                ComponentParamType::Action => ArgKind::Action,
                ComponentParamType::Binding(t) => ArgKind::Binding(t.clone()),
            };
            let value = self.arg_value(sc, &kind, &arg.value);
            args.insert(p.name.clone(), value);
        }
        let mut ordered = IndexMap::new();
        for p in &params {
            match args.shift_remove(&p.name) {
                Some(v) => {
                    ordered.insert(p.name.clone(), v);
                }
                None if matches!(&p.ty, ComponentParamType::Value(t) if t.is_optional()) => {
                    ordered.insert(
                        p.name.clone(),
                        Value::Expr(ir::Expr::new(
                            ir::ExprKind::Nil,
                            match &p.ty {
                                ComponentParamType::Value(t) => t.clone(),
                                _ => Type::Unknown,
                            },
                        )),
                    );
                }
                None => self.error(
                    "V2032",
                    format!("组件`{}`缺少参数`{}`", name.name, p.name),
                    el.span,
                    "",
                ),
            }
        }
        let modifiers = self.modifiers(sc, &el.modifiers, None);
        Some(ir::Element {
            kind: ElementKind::Component(name.name.clone()),
            args: ordered,
            children: Vec::new(),
            item: None,
            modifiers,
        })
    }

    /// 平台命名空间需与当前目标一致
    fn check_platform(&mut self, ns: &ast::Ident, full: &str, span: Span) -> bool {
        let Some(platform) = ir::Platform::from_cfg_name(&ns.name) else {
            self.error(
                "V1011",
                format!("未知的平台`{}`", ns.name),
                ns.span,
                "可用的平台：ios、android",
            );
            return false;
        };
        if platform != self.target.platform {
            self.diags.push(
                Diagnostic::error("V6004", format!("`{full}`只能用于{}", ns.name))
                    .with_label(span, "")
                    .with_note(format!("请放在`#if {}`条件块中", ns.name)),
            );
            return false;
        }
        true
    }

    fn platform_element(&mut self, sc: &mut Scope, el: &'a ast::Element) -> Option<ir::Element> {
        let full = el.path_string();
        let [ns, name] = el.path.as_slice() else {
            self.error("V1011", format!("未知的组件`{full}`"), el.span, "");
            return None;
        };
        if !self.check_platform(ns, &full, el.span) {
            return None;
        }
        let Some((kind, spec)) = catalog::platform_element(&ns.name, &name.name) else {
            let all = if ns.name == "ios" {
                catalog::IOS_ELEMENTS
            } else {
                catalog::ANDROID_ELEMENTS
            };
            self.diags.push(
                Diagnostic::error("V1011", format!("未知的组件`{full}`"))
                    .with_label(name.span, "")
                    .with_note(format!("{}专属组件：{}", ns.name, all.join("、"))),
            );
            return None;
        };
        let args = self.match_args(
            sc,
            &spec.args,
            &el.args,
            name.span,
            &format!("组件`{full}`"),
        );
        if let Some(c) = &el.closure {
            self.error("V2034", format!("组件`{full}`不接受子组件"), c.span, "");
        }
        let modifiers = self.modifiers(sc, &el.modifiers, None);
        Some(ir::Element {
            kind,
            args,
            children: Vec::new(),
            item: None,
            modifiers,
        })
    }

    fn platform_modifier(&mut self, m: &ast::Modifier) -> Option<ModifierKind> {
        let full = format!(".{}", ast::path_string(&m.path));
        let [ns, name] = m.path.as_slice() else {
            self.error("V1012", format!("未知的修饰符`{full}`"), m.span, "");
            return None;
        };
        if !self.check_platform(ns, &full, m.span) {
            return None;
        }
        let kind = catalog::platform_modifier(&ns.name, &name.name);
        if kind.is_none() {
            let all = if ns.name == "ios" {
                catalog::IOS_MODIFIERS
            } else {
                catalog::ANDROID_MODIFIERS
            };
            self.diags.push(
                Diagnostic::error("V1012", format!("未知的修饰符`{full}`"))
                    .with_label(name.span, "")
                    .with_note(format!("{}专属修饰符：{}", ns.name, all.join("、"))),
            );
        }
        kind
    }

    /// `native ios { """...""" }`，`{{name}}`替换为状态读取或命令入口
    fn native(&mut self, sc: &mut Scope, n: &'a ast::NativeNode) -> Option<ir::ViewNode> {
        if !self.check_platform(&n.platform, &format!("native {}", n.platform.name), n.span) {
            return None;
        }
        match self.target.native {
            NativeLint::Allow => {}
            NativeLint::Warn => self.diags.push(
                Diagnostic::warning("V6010", "native代码块不受Veyra的类型检查与MVVM约束")
                    .with_label(n.span, "")
                    .with_note("可在veyra.toml的[lint]中设置native = \"allow\"或\"deny\""),
            ),
            NativeLint::Deny => {
                self.error(
                    "V6010",
                    "项目禁止使用native代码块",
                    n.span,
                    "veyra.toml中[lint] native = \"deny\"",
                );
                return None;
            }
        }
        let mut segments = Vec::new();
        let mut rest = n.code.as_str();
        while let Some(start) = rest.find("{{") {
            if start > 0 {
                segments.push(ir::NativeSegment::Text(rest[..start].to_string()));
            }
            let after = &rest[start + 2..];
            let Some(end) = after.find("}}") else {
                self.error("V6011", "native代码中的`{{`缺少`}}`", n.code_span, "");
                return None;
            };
            let path: Vec<&str> = after[..end].trim().split('.').collect();
            if let Some(seg) = self.native_ref(sc, &path, n.code_span) {
                segments.push(seg);
            }
            rest = &after[end + 2..];
        }
        if !rest.is_empty() {
            segments.push(ir::NativeSegment::Text(rest.to_string()));
        }
        Some(ir::ViewNode::Native(segments))
    }

    fn native_ref(
        &mut self,
        sc: &mut Scope,
        path: &[&str],
        span: Span,
    ) -> Option<ir::NativeSegment> {
        let valid = |p: &str| {
            p.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        };
        if path.is_empty() || !path.iter().all(|p| valid(p)) {
            self.error(
                "V6011",
                format!("无效的占位符`{{{{{}}}}}`", path.join(".")),
                span,
                "占位符只能是名称或字段路径",
            );
            return None;
        }
        if path.len() == 1 {
            if sc.is_action(path[0]) {
                return Some(ir::NativeSegment::Action(path[0].to_string()));
            }
            if sc.is_action_param(path[0]) {
                return Some(ir::NativeSegment::Callback(path[0].to_string()));
            }
        }
        // 组装成表达式交给常规检查
        let mut e = ast::Expr {
            kind: A::Ident(path[0].to_string()),
            span,
        };
        for p in &path[1..] {
            e = ast::Expr {
                kind: A::Member(
                    Box::new(e),
                    ast::Ident {
                        name: p.to_string(),
                        span,
                    },
                ),
                span,
            };
        }
        let v = self.expr(sc, &e, None);
        if v.ty.is_unknown() {
            return None;
        }
        Some(ir::NativeSegment::Expr(v))
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
        owner: Option<Builtin>,
    ) -> Vec<ir::Modifier> {
        let pruned = self.cfg.prune(nodes, &mut self.diags);
        let mut out = Vec::new();
        for p in pruned {
            let m = p.item;
            let name = m.path.last().unwrap_or(&m.path[0]);
            let kind = if m.path.len() > 1 {
                match self.platform_modifier(m) {
                    Some(k) => Some(k),
                    None => continue,
                }
            } else {
                ModifierKind::from_name(&name.name)
            };
            let Some(kind) = kind else {
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
                && !owner.is_some_and(|o| allowed.contains(&o))
            {
                let owner = owner.map(|o| o.name()).unwrap_or("自定义组件");
                self.diags.push(
                    Diagnostic::error("V2042", format!("`.{}`不能用于`{owner}`", name.name))
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
            if p.debug.is_some() && catalog::is_parameter_modifier(kind) {
                self.diags.push(
                    Diagnostic::error("V6003", format!("`.{}`不能放在`debug`条件中", name.name))
                        .with_label(name.span, "")
                        .with_note("该修饰符在Compose中会生成为组件参数，无法按构建类型切换"),
                );
                continue;
            }
            out.push(ir::Modifier {
                kind,
                args,
                debug: p.debug,
            });
        }
        out
    }

    /// 按参数表匹配实参
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

/// 把连续的debug节点合并为`ViewNode::Debug`
fn group_debug(items: Vec<(Option<bool>, ir::ViewNode)>) -> Vec<ir::ViewNode> {
    let mut out = Vec::new();
    let mut then = Vec::new();
    let mut else_ = Vec::new();
    let flush = |out: &mut Vec<ir::ViewNode>,
                 then: &mut Vec<ir::ViewNode>,
                 else_: &mut Vec<ir::ViewNode>| {
        if !then.is_empty() || !else_.is_empty() {
            out.push(ir::ViewNode::Debug {
                then: std::mem::take(then),
                else_: std::mem::take(else_),
            });
        }
    };
    for (debug, node) in items {
        match debug {
            None => {
                flush(&mut out, &mut then, &mut else_);
                out.push(node);
            }
            Some(true) => {
                if !else_.is_empty() {
                    flush(&mut out, &mut then, &mut else_);
                }
                then.push(node);
            }
            Some(false) => else_.push(node),
        }
    }
    flush(&mut out, &mut then, &mut else_);
    out
}
