// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 自定义组件：无业务状态的可复用视图片段

use indexmap::{IndexMap, IndexSet};
use veyra_diagnostics::Diagnostic;
use veyra_ir::{self as ir, ComponentParam, ComponentParamType, ElementKind, ViewNode};
use veyra_syntax::ast::{self, TypeExprKind};

use crate::decls::TypePos;
use crate::expr::{Owner, Scope};
use crate::{Checker, Global};

impl<'a> Checker<'a> {
    fn component_decls(&self) -> Vec<&'a ast::ComponentDecl> {
        self.globals
            .values()
            .filter_map(|(g, _)| match g {
                Global::Component(d) => Some(*d),
                _ => None,
            })
            .collect()
    }

    /// 先登记全部签名，组件之间可以互相引用
    pub(crate) fn check_component_signatures(&mut self) {
        for d in self.component_decls() {
            self.current = d.span.file;
            let mut params: Vec<ComponentParam> = Vec::new();
            for p in &d.params {
                self.check_member_name(&p.name);
                if params.iter().any(|x| x.name == p.name.name) {
                    self.error(
                        "V1003",
                        format!("参数`{}`重复", p.name.name),
                        p.name.span,
                        "",
                    );
                    continue;
                }
                let ty = match &p.ty.kind {
                    TypeExprKind::Named { name, args }
                        if name.name == "Action" && args.is_empty() =>
                    {
                        ComponentParamType::Action
                    }
                    TypeExprKind::Named { name, args } if name.name == "Binding" => {
                        match args.as_slice() {
                            [inner] => ComponentParamType::Binding(
                                self.resolve_type(inner, TypePos::Value),
                            ),
                            _ => {
                                self.error("V2004", "`Binding`需要1个类型参数", p.ty.span, "");
                                continue;
                            }
                        }
                    }
                    _ => ComponentParamType::Value(self.resolve_type(&p.ty, TypePos::Value)),
                };
                params.push(ComponentParam {
                    name: p.name.name.clone(),
                    ty,
                });
            }
            self.components.insert(d.name.name.clone(), params);
        }
    }

    pub(crate) fn check_components(&mut self) -> Vec<ir::ComponentDef> {
        let mut out = Vec::new();
        for d in self.component_decls() {
            self.current = d.span.file;
            let params = self
                .components
                .get(&d.name.name)
                .cloned()
                .unwrap_or_default();
            let mut sc = Scope::new(Owner::Component(&params), true);
            let body = self.view_nodes(&mut sc, &d.body);
            if d.body.is_empty() {
                self.error("V3021", "组件不能为空", d.span, "");
            }
            out.push(ir::ComponentDef {
                cond: self.item_conds.get(&d.name.name).cloned(),
                name: d.name.name.clone(),
                params,
                body,
            });
        }
        self.check_component_cycles(&out);
        out
    }

    /// 组件不能直接或间接引用自身
    fn check_component_cycles(&mut self, components: &[ir::ComponentDef]) {
        fn uses(nodes: &[ViewNode], out: &mut IndexSet<String>) {
            for n in nodes {
                match n {
                    ViewNode::Element(el) => {
                        if let ElementKind::Component(c) = &el.kind {
                            out.insert(c.clone());
                        }
                        uses(&el.children, out);
                    }
                    ViewNode::If { then, else_, .. }
                    | ViewNode::Cond { then, else_, .. }
                    | ViewNode::Available { then, else_, .. } => {
                        uses(then, out);
                        uses(else_, out);
                    }
                    ViewNode::Native(_) => {}
                }
            }
        }
        let graph: IndexMap<&str, IndexSet<String>> = components
            .iter()
            .map(|c| {
                let mut set = IndexSet::new();
                uses(&c.body, &mut set);
                (c.name.as_str(), set)
            })
            .collect();
        for c in components {
            let mut stack: Vec<String> = graph[c.name.as_str()].iter().cloned().collect();
            let mut seen = IndexSet::new();
            while let Some(n) = stack.pop() {
                if n == c.name {
                    let span = self
                        .globals
                        .get(&c.name)
                        .map(|(_, s)| *s)
                        .unwrap_or_default();
                    self.diags.push(
                        Diagnostic::error("V3040", format!("组件`{}`递归引用了自身", c.name))
                            .with_label(span, "")
                            .with_note("递归的视图结构请用List或ForEach表达"),
                    );
                    break;
                }
                if seen.insert(n.clone())
                    && let Some(next) = graph.get(n.as_str())
                {
                    stack.extend(next.iter().cloned());
                }
            }
        }
    }
}
