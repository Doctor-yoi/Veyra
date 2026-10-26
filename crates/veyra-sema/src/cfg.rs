// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 条件编译求值与裁剪。
//!
//! 平台与自定义flag在编译期确定；`debug`无法在编译期确定，
//! 因此每个保留下来的节点都带一个`debug`约束：
//! `None`表示无约束，`Some(true)`表示仅调试构建，`Some(false)`表示仅发布构建。

use veyra_diagnostics::{Diagnostic, Diagnostics, Span};
use veyra_ir::Platform;
use veyra_syntax::ast::{CfgExpr, Node};

use crate::Target;

pub(crate) struct Pruned<'a, T> {
    pub item: &'a T,
    pub debug: Option<bool>,
    /// 引入`debug`约束的指令位置
    pub cond_span: Span,
}

pub(crate) struct Cfg<'t> {
    target: &'t Target,
}

impl<'t> Cfg<'t> {
    pub fn new(target: &'t Target) -> Self {
        Self { target }
    }

    /// 检查条件中的名称是否都已声明。
    fn validate(&self, expr: &CfgExpr, diags: &mut Diagnostics) {
        match expr {
            CfgExpr::Ident(id) => {
                let name = id.name.as_str();
                if Platform::from_cfg_name(name).is_none()
                    && name != "debug"
                    && !self.target.flags.contains_key(name)
                {
                    diags.push(
                        Diagnostic::error("V6001", format!("未声明的条件`{name}`"))
                            .with_label(id.span, "")
                            .with_note("可用的条件：ios、android、harmony、debug，以及veyra.toml中[flags]声明的名称"),
                    );
                }
            }
            CfgExpr::Not(inner, _) | CfgExpr::Paren(inner, _) => self.validate(inner, diags),
            CfgExpr::And(a, b) | CfgExpr::Or(a, b) => {
                self.validate(a, diags);
                self.validate(b, diags);
            }
        }
    }

    fn eval(&self, expr: &CfgExpr, debug: bool) -> bool {
        match expr {
            CfgExpr::Ident(id) => {
                let name = id.name.as_str();
                if let Some(p) = Platform::from_cfg_name(name) {
                    p == self.target.platform
                } else if name == "debug" {
                    debug
                } else {
                    self.target.flags.get(name).copied().unwrap_or(false)
                }
            }
            CfgExpr::Not(inner, _) => !self.eval(inner, debug),
            CfgExpr::Paren(inner, _) => self.eval(inner, debug),
            CfgExpr::And(a, b) => self.eval(a, debug) && self.eval(b, debug),
            CfgExpr::Or(a, b) => self.eval(a, debug) || self.eval(b, debug),
        }
    }

    /// 返回条件在`debug=false`与`debug=true`下的取值。
    fn truth(&self, expr: &CfgExpr) -> [bool; 2] {
        [self.eval(expr, false), self.eval(expr, true)]
    }

    pub fn prune<'a, T>(
        &self,
        nodes: &'a [Node<T>],
        diags: &mut Diagnostics,
    ) -> Vec<Pruned<'a, T>> {
        let mut out = Vec::new();
        self.prune_into(nodes, [true, true], Span::default(), &mut out, diags);
        out
    }

    /// `allowed[i]`表示`debug == (i == 1)`时该层是否生效。
    fn prune_into<'a, T>(
        &self,
        nodes: &'a [Node<T>],
        allowed: [bool; 2],
        cond_span: Span,
        out: &mut Vec<Pruned<'a, T>>,
        diags: &mut Diagnostics,
    ) {
        for node in nodes {
            match node {
                Node::Item(item) => {
                    let debug = match allowed {
                        [true, true] => None,
                        [false, true] => Some(true),
                        [true, false] => Some(false),
                        [false, false] => continue,
                    };
                    out.push(Pruned {
                        item,
                        debug,
                        cond_span,
                    });
                }
                Node::Cfg(group) => {
                    let mut remaining = allowed;
                    for branch in &group.branches {
                        let truth = match &branch.cond {
                            Some(cond) => {
                                self.validate(cond, diags);
                                self.truth(cond)
                            }
                            None => [true, true],
                        };
                        let claimed = [remaining[0] && truth[0], remaining[1] && truth[1]];
                        if claimed != [false, false] {
                            let span = if claimed == allowed {
                                cond_span
                            } else {
                                branch.span
                            };
                            self.prune_into(&branch.items, claimed, span, out, diags);
                        } else {
                            // 未选中的分支也检查条件名称
                            self.validate_nested(&branch.items, diags);
                        }
                        remaining = [remaining[0] && !claimed[0], remaining[1] && !claimed[1]];
                    }
                }
            }
        }
    }

    fn validate_nested<T>(&self, nodes: &[Node<T>], diags: &mut Diagnostics) {
        for node in nodes {
            if let Node::Cfg(group) = node {
                for branch in &group.branches {
                    if let Some(cond) = &branch.cond {
                        self.validate(cond, diags);
                    }
                    self.validate_nested(&branch.items, diags);
                }
            }
        }
    }
}
