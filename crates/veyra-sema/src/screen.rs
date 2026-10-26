// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! screen检查：依赖、状态、派生状态、命令与视图

use indexmap::{IndexMap, IndexSet};
use veyra_diagnostics::{Diagnostic, Span};
use veyra_ir::{self as ir, ExprKind, InjectDef, InjectKind};
use veyra_syntax::ast::{self, ScreenMember};

use crate::decls::TypePos;
use crate::expr::{ActionInfo, Owner, Scope, ScreenInfo, StateInfo};
use crate::{Checker, Global};

impl<'a> Checker<'a> {
    pub(crate) fn check_screens(&mut self) -> Vec<ir::ScreenDef> {
        let screens: Vec<_> = self
            .globals
            .values()
            .filter_map(|(g, _)| match g {
                Global::Screen(d) => Some(*d),
                _ => None,
            })
            .collect();
        screens.into_iter().map(|d| self.check_screen(d)).collect()
    }

    fn check_screen(&mut self, d: &'a ast::ScreenDecl) -> ir::ScreenDef {
        let members = self.prune_decls(&d.members);
        let mut info = ScreenInfo::default();
        let mut names: IndexMap<String, Span> = IndexMap::new();
        let mut injects = Vec::new();
        let mut state = Vec::new();
        let mut computed_src = Vec::new();
        let mut actions = Vec::new();
        let mut effects: Vec<ir::EffectDef> = Vec::new();
        let mut hooks = Vec::new();
        let mut views = Vec::new();

        let mut claim = |c: &mut Self, id: &ast::Ident| -> bool {
            c.check_member_name(id);
            if let Some(prev) = names.get(&id.name) {
                c.diags.push(
                    Diagnostic::error("V1003", format!("screen成员`{}`重复", id.name))
                        .with_label(id.span, "")
                        .with_secondary(*prev, "首次声明于此"),
                );
                return false;
            }
            names.insert(id.name.clone(), id.span);
            true
        };

        // 先收集签名，view与computed可以引用之后声明的成员
        for m in &members {
            match m {
                ScreenMember::Inject(inj) => {
                    if !claim(self, &inj.name) {
                        continue;
                    }
                    match self.globals.get(&inj.ty.name).map(|(g, _)| *g) {
                        Some(Global::Repository(_)) => {
                            info.injects.insert(inj.name.name.clone());
                            injects.push(InjectDef {
                                name: inj.name.name.clone(),
                                ty: inj.ty.name.clone(),
                                kind: InjectKind::Repository,
                            });
                        }
                        Some(Global::Store(_)) => {
                            info.injects.insert(inj.name.name.clone());
                            injects.push(InjectDef {
                                name: inj.name.name.clone(),
                                ty: inj.ty.name.clone(),
                                kind: InjectKind::Store,
                            });
                        }
                        Some(Global::Settings(_)) => {
                            info.injects.insert(inj.name.name.clone());
                            injects.push(InjectDef {
                                name: inj.name.name.clone(),
                                ty: inj.ty.name.clone(),
                                kind: InjectKind::Settings,
                            });
                        }
                        Some(Global::Secure(_)) => {
                            info.injects.insert(inj.name.name.clone());
                            injects.push(InjectDef {
                                name: inj.name.name.clone(),
                                ty: inj.ty.name.clone(),
                                kind: InjectKind::Secure,
                            });
                        }
                        Some(_) => self.diags.push(
                            Diagnostic::error("V3008", format!("`{}`不能被注入", inj.ty.name))
                                .with_label(inj.ty.span, "")
                                .with_note(
                                    "只有repository、store、settings与secure可以被注入到ViewModel",
                                ),
                        ),
                        None => self.error(
                            "V1010",
                            format!("未定义的类型`{}`", inj.ty.name),
                            inj.ty.span,
                            "",
                        ),
                    }
                }
                ScreenMember::State(block) => {
                    for f in self.prune_decls(&block.items) {
                        if !claim(self, &f.name) {
                            continue;
                        }
                        let ty = self.resolve_type(&f.ty, TypePos::Value);
                        let mut writable = false;
                        for attr in &f.attrs {
                            match attr.name.name.as_str() {
                                "writable" => writable = true,
                                other => self.error(
                                    "V2040",
                                    format!("state字段不支持`@{other}`"),
                                    attr.span,
                                    "",
                                ),
                            }
                        }
                        info.state.insert(
                            f.name.name.clone(),
                            StateInfo {
                                ty: ty.clone(),
                                writable,
                                span: f.name.span,
                            },
                        );
                        state.push((f, ty, writable));
                    }
                }
                ScreenMember::Computed(block) => {
                    for f in self.prune_decls(&block.items) {
                        if !claim(self, &f.name) {
                            continue;
                        }
                        for attr in &f.attrs {
                            self.error(
                                "V2040",
                                format!("computed字段不支持`@{}`", attr.name.name),
                                attr.span,
                                "",
                            );
                        }
                        let ty = self.resolve_type(&f.ty, TypePos::Value);
                        info.computed.insert(f.name.name.clone(), ty.clone());
                        computed_src.push((f, ty));
                    }
                }
                ScreenMember::Actions(block) => {
                    for a in self.prune_decls(&block.items) {
                        if !claim(self, &a.name) {
                            continue;
                        }
                        if let Some(ret) = &a.ret {
                            self.diags.push(
                                Diagnostic::error("V3018", "action不能有返回值")
                                    .with_label(ret.span, "")
                                    .with_note("action的结果应写入state"),
                            );
                        }
                        if a.throws {
                            self.error(
                                "V3019",
                                "action不能声明throws",
                                a.span,
                                "错误应在ViewModel内部处理并写入state",
                            );
                        }
                        let params = self.params(&a.params);
                        info.actions.insert(
                            a.name.name.clone(),
                            ActionInfo {
                                params: params.clone(),
                                is_async: a.is_async,
                            },
                        );
                        actions.push(ir::ActionDef {
                            name: a.name.name.clone(),
                            params,
                            is_async: a.is_async,
                        });
                    }
                }
                ScreenMember::Effects(block) => {
                    for e in self.prune_decls(&block.items) {
                        if !claim(self, &e.name) {
                            continue;
                        }
                        if e.is_async || e.throws || e.ret.is_some() {
                            self.diags.push(
                                Diagnostic::error("V3032", "effect只声明名称与参数")
                                    .with_label(e.span, "")
                                    .with_note("effect由ViewModel发出、由组装层处理，不能有async、throws或返回值"),
                            );
                        }
                        info.effects.insert(e.name.name.clone());
                        let params = self.params(&e.params);
                        effects.push(ir::EffectDef {
                            name: e.name.name.clone(),
                            params,
                        });
                    }
                }
                ScreenMember::Lifecycle(block) => hooks.extend(self.prune_decls(&block.items)),
                ScreenMember::View(block) => views.push(block),
            }
        }

        // setter不能与action重名
        for (name, s) in &info.state {
            if s.writable {
                let setter = ir::setter_name(name);
                if info.actions.contains_key(&setter) {
                    self.diags.push(
                        Diagnostic::error(
                            "V3006",
                            format!("action`{setter}`与state`{name}`的setter重名"),
                        )
                        .with_label(s.span, "")
                        .with_note("@writable字段会自动生成同名setter"),
                    );
                }
            }
        }

        let state = state
            .into_iter()
            .map(|(f, ty, writable)| {
                let default = match &f.default {
                    Some(e) => self.expr(&Scope::new(Owner::Constant, false), e, Some(&ty)),
                    None if ty.is_optional() => ir::Expr::new(ExprKind::Nil, ty.clone()),
                    None => {
                        if !ty.is_unknown() {
                            self.error(
                                "V2010",
                                format!("state`{}`需要默认值", f.name.name),
                                f.name.span,
                                "非可选类型必须提供默认值",
                            );
                        }
                        ir::Expr::unknown()
                    }
                };
                ir::StateField {
                    name: f.name.name.clone(),
                    ty,
                    default,
                    writable,
                }
            })
            .collect();

        let computed: Vec<ir::ComputedField> = computed_src
            .into_iter()
            .map(|(f, ty)| {
                let sc = Scope::new(Owner::Screen(&info), false);
                let expr = match &f.default {
                    Some(e) => self.expr(&sc, e, Some(&ty)),
                    None => {
                        self.error(
                            "V2013",
                            format!("computed`{}`需要表达式", f.name.name),
                            f.name.span,
                            "",
                        );
                        ir::Expr::unknown()
                    }
                };
                ir::ComputedField {
                    name: f.name.name.clone(),
                    ty,
                    expr,
                }
            })
            .collect();
        self.check_computed_cycles(&computed, &names);

        let mut lifecycle: Vec<ir::LifecycleHook> = Vec::new();
        for h in hooks {
            let Some(event) = ir::LifecycleEvent::from_name(&h.event.name) else {
                self.diags.push(
                    Diagnostic::error("V3030", format!("未知的生命周期事件`{}`", h.event.name))
                        .with_label(h.event.span, "")
                        .with_note("可用的事件：onAppear、onDisappear"),
                );
                continue;
            };
            if lifecycle.iter().any(|x| x.event == event) {
                self.error(
                    "V3031",
                    format!("生命周期事件`{}`重复", h.event.name),
                    h.event.span,
                    "",
                );
                continue;
            }
            let sc = Scope::new(Owner::Screen(&info), false);
            if let Some(action) = self.action_ref(&sc, &h.action) {
                lifecycle.push(ir::LifecycleHook { event, action });
            }
        }

        let mut view = Vec::new();
        match views.as_slice() {
            [] => self.error(
                "V3020",
                format!("screen`{}`缺少view", d.name.name),
                d.name.span,
                "",
            ),
            [block, rest @ ..] => {
                for extra in rest {
                    self.error("V3020", "screen只能有一个view", extra.keyword, "");
                }
                let mut sc = Scope::new(Owner::Screen(&info), true);
                view = self.view_nodes(&mut sc, &block.items);
                if view.is_empty() && block.items.is_empty() {
                    self.error("V3021", "view不能为空", block.span, "");
                }
            }
        }

        ir::ScreenDef {
            name: d.name.name.clone(),
            injects,
            state,
            computed,
            actions,
            effects,
            lifecycle,
            view,
        }
    }

    fn check_computed_cycles(
        &mut self,
        computed: &[ir::ComputedField],
        names: &IndexMap<String, Span>,
    ) {
        fn refs(e: &ir::Expr, out: &mut IndexSet<String>) {
            match &e.kind {
                ExprKind::Computed(n) => {
                    out.insert(n.clone());
                }
                ExprKind::List(items) => items.iter().for_each(|i| refs(i, out)),
                ExprKind::Field(b, _)
                | ExprKind::Count(b)
                | ExprKind::IsEmpty(b)
                | ExprKind::First(b)
                | ExprKind::Unary(_, b) => refs(b, out),
                ExprKind::Contains(a, b) | ExprKind::Binary(_, a, b) => {
                    refs(a, out);
                    refs(b, out);
                }
                ExprKind::Conditional(c, a, b) => {
                    refs(c, out);
                    refs(a, out);
                    refs(b, out);
                }
                ExprKind::Loc { args, .. } => args.iter().for_each(|(_, v)| refs(v, out)),
                ExprKind::Plural { count, .. } => refs(count, out),
                ExprKind::Format { value, .. } => refs(value, out),
                _ => {}
            }
        }
        let graph: IndexMap<&str, IndexSet<String>> = computed
            .iter()
            .map(|c| {
                let mut out = IndexSet::new();
                refs(&c.expr, &mut out);
                (c.name.as_str(), out)
            })
            .collect();
        for c in computed {
            let mut stack: Vec<String> = graph[c.name.as_str()].iter().cloned().collect();
            let mut seen = IndexSet::new();
            while let Some(n) = stack.pop() {
                if n == c.name {
                    let span = names.get(&c.name).copied().unwrap_or_default();
                    self.error(
                        "V3007",
                        format!("computed`{}`存在循环依赖", c.name),
                        span,
                        "",
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
