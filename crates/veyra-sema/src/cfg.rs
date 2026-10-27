// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 条件编译求值与裁剪
//!
//! 后端平台（apple、android、harmony）与自定义flag在编译期确定
//! `debug`与Apple各系统（ios、macos等）由原生构建决定，保留为原生条件
//! 裁剪时记录每个节点生效的世界集合，见[`veyra_ir::Worlds`]

use veyra_diagnostics::{Diagnostic, Diagnostics, Span};
use veyra_ir::{AppleOs, NativeCond, Platform, Worlds};
use veyra_syntax::ast::{CfgExpr, Node};

use crate::Target;

pub(crate) struct Pruned<'a, T> {
    pub item: &'a T,
    pub worlds: Worlds,
    /// 化简后的原生条件，None表示总是生效
    pub cond: Option<NativeCond>,
    /// 引入原生条件的指令位置
    pub cond_span: Span,
}

impl<T> Pruned<'_, T> {
    pub fn depends_on_debug(&self) -> bool {
        self.worlds.depends_on_debug()
    }

    pub fn depends_on_os(&self) -> bool {
        self.worlds.depends_on_os()
    }
}

pub(crate) struct Cfg<'t> {
    target: &'t Target,
    /// 需要保留为原生条件的系统，为空时系统已经确定
    residual: Vec<AppleOs>,
    /// 系统已确定时的取值
    fixed: Option<AppleOs>,
}

impl<'t> Cfg<'t> {
    pub fn new(target: &'t Target) -> Self {
        let (residual, fixed) = match target.platform {
            Platform::Apple => match (target.os, target.apple_oses.as_slice()) {
                (Some(os), _) => (Vec::new(), Some(os)),
                (None, []) => (Vec::new(), Some(AppleOs::Ios)),
                (None, [only]) => (Vec::new(), Some(*only)),
                (None, many) => (many.to_vec(), None),
            },
            _ => (Vec::new(), None),
        };
        Self {
            target,
            residual,
            fixed,
        }
    }

    fn all(&self) -> Worlds {
        Worlds::all(self.residual.len())
    }

    fn os_at(&self, index: usize) -> Option<AppleOs> {
        self.residual.get(index).copied().or(self.fixed)
    }

    /// 检查条件中的名称是否都已声明
    fn validate(&self, expr: &CfgExpr, diags: &mut Diagnostics) {
        match expr {
            CfgExpr::Ident(id) => {
                let name = id.name.as_str();
                let known = Platform::from_cfg_name(name).is_some()
                    || AppleOs::from_cfg_name(name).is_some()
                    || name == "debug"
                    || self.target.flags.contains_key(name);
                if !known {
                    diags.push(
                        Diagnostic::error("V6001", format!("未声明的条件`{name}`"))
                            .with_label(id.span, "")
                            .with_note("可用的条件：apple、android、harmony、ios、macos、watchos、tvos、visionos、debug，以及veyra.toml中[flags]声明的名称"),
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

    fn eval(&self, expr: &CfgExpr, os: Option<AppleOs>, debug: bool) -> bool {
        match expr {
            CfgExpr::Ident(id) => {
                let name = id.name.as_str();
                if let Some(p) = Platform::from_cfg_name(name) {
                    p == self.target.platform
                } else if let Some(o) = AppleOs::from_cfg_name(name) {
                    self.target.platform == Platform::Apple && os == Some(o)
                } else if name == "debug" {
                    debug
                } else {
                    self.target.flags.get(name).copied().unwrap_or(false)
                }
            }
            CfgExpr::Not(inner, _) => !self.eval(inner, os, debug),
            CfgExpr::Paren(inner, _) => self.eval(inner, os, debug),
            CfgExpr::And(a, b) => self.eval(a, os, debug) && self.eval(b, os, debug),
            CfgExpr::Or(a, b) => self.eval(a, os, debug) || self.eval(b, os, debug),
        }
    }

    /// 条件为真的世界
    fn truth(&self, expr: &CfgExpr) -> Worlds {
        let mut w = Worlds::empty(self.residual.len());
        for i in 0..self.residual.len().max(1) {
            for debug in [false, true] {
                if self.eval(expr, self.os_at(i), debug) {
                    w.insert(i, debug);
                }
            }
        }
        w
    }

    pub fn prune<'a, T>(
        &self,
        nodes: &'a [Node<T>],
        diags: &mut Diagnostics,
    ) -> Vec<Pruned<'a, T>> {
        let mut out = Vec::new();
        self.prune_into(nodes, self.all(), Span::default(), &mut out, diags);
        out
    }

    fn prune_into<'a, T>(
        &self,
        nodes: &'a [Node<T>],
        allowed: Worlds,
        cond_span: Span,
        out: &mut Vec<Pruned<'a, T>>,
        diags: &mut Diagnostics,
    ) {
        for node in nodes {
            match node {
                Node::Item(item) => {
                    if allowed.is_empty() {
                        continue;
                    }
                    let cond = allowed.to_cond(&self.residual);
                    out.push(Pruned {
                        item,
                        worlds: allowed,
                        cond,
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
                            None => self.all(),
                        };
                        let claimed = remaining.and(truth);
                        if claimed.is_empty() {
                            // 未选中的分支也检查条件名称
                            self.validate_nested(&branch.items, diags);
                        } else {
                            let span = if claimed == allowed {
                                cond_span
                            } else {
                                branch.span
                            };
                            self.prune_into(&branch.items, claimed, span, out, diags);
                        }
                        remaining = remaining.minus(claimed);
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

/// 合并外层与内层的原生条件
pub(crate) fn and_cond(a: Option<NativeCond>, b: Option<NativeCond>) -> Option<NativeCond> {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(a), Some(b)) if a == b => Some(a),
        (Some(a), Some(b)) => Some(NativeCond::And(vec![a, b])),
    }
}
