// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! `swiftui.*`：按SDK目录做重载解析、参数类型检查与系统版本检查

use veyra_catalog::{Availability, OsAvailability, Param, ParamType, Signature, TextKind, Version};
use veyra_diagnostics::{Diagnostic, Span};
use veyra_ir::{self as ir, FrameworkArg, FrameworkCall, FrameworkValue, SwiftConv, Type};
use veyra_syntax::ast::{self, Arg, ExprKind as A};

use crate::Checker;
use crate::expr::Scope;

/// 参数类型的可读写法，用于诊断
fn describe(t: &ParamType) -> String {
    match t {
        ParamType::Bool => "Bool".into(),
        ParamType::Int => "Int".into(),
        ParamType::Double | ParamType::Float => "Double".into(),
        ParamType::String | ParamType::Url => "String".into(),
        ParamType::Date => "Date".into(),
        ParamType::Number => "Int或Double".into(),
        ParamType::Any => "任意值".into(),
        ParamType::Text(_) => "文本".into(),
        ParamType::Binding(inner) => format!("Binding<{}>", describe(inner)),
        ParamType::Action { .. } => "Action".into(),
        ParamType::Content | ParamType::View => "{ 视图 }".into(),
        ParamType::Token(t) | ParamType::Style(t) => format!(".{t}"),
        ParamType::Optional(inner) => format!("{}?", describe(inner)),
    }
}

fn describe_signature(name: &str, sig: &[Param]) -> String {
    let parts: Vec<_> = sig
        .iter()
        .map(|p| {
            let label = p.label.as_deref().unwrap_or("_");
            let default = if p.default { " = 默认值" } else { "" };
            format!("{label}: {}{default}", describe(&p.ty))
        })
        .collect();
    format!("{name}({})", parts.join(", "))
}

/// 实参来源：参数列表中的表达式或尾随闭包
#[derive(Clone, Copy)]
enum Source<'x> {
    Expr(&'x ast::Expr),
    Trailing(&'x ast::Closure),
}

/// 结构匹配的结果：每个参数对应的实参，None表示使用默认值
type Matched<'x> = Vec<(usize, Option<Source<'x>>)>;

/// 按Swift的规则把实参对应到签名：标签顺序一致，有默认值的参数可省略，尾随闭包填入第一个未提供的视图内容参数
fn match_params<'x>(
    params: &[Param],
    args: &'x [Arg],
    trailing: Option<&'x ast::Closure>,
) -> Option<Matched<'x>> {
    let mut out = Vec::new();
    let mut ai = 0;
    let mut trailing = trailing;
    for (pi, p) in params.iter().enumerate() {
        if let Some(arg) = args.get(ai) {
            let same = match (&arg.label, &p.label) {
                (Some(l), Some(pl)) => l.name == *pl,
                (None, None) => true,
                _ => false,
            };
            if same {
                out.push((pi, Some(Source::Expr(&arg.value))));
                ai += 1;
                continue;
            }
        }
        if matches!(p.ty, ParamType::Content)
            && let Some(t) = trailing.take()
        {
            out.push((pi, Some(Source::Trailing(t))));
            continue;
        }
        if p.default {
            out.push((pi, None));
            continue;
        }
        return None;
    }
    (ai == args.len() && trailing.is_none()).then_some(out)
}

fn base_type(t: &ParamType) -> Option<Type> {
    Some(match t {
        ParamType::Bool => Type::Bool,
        ParamType::Int => Type::Int,
        ParamType::Double | ParamType::Float => Type::Double,
        ParamType::String | ParamType::Url | ParamType::Text(_) => Type::String,
        ParamType::Date => Type::Date,
        ParamType::Optional(inner) => Type::optional(base_type(inner)?),
        _ => return None,
    })
}

impl<'a> Checker<'a> {
    /// 检查API在当前系统上的可用性，返回是否可用
    pub(crate) fn check_availability(
        &mut self,
        sc: &Scope,
        what: &str,
        avail: &Availability,
        span: Span,
    ) -> bool {
        let Some(os) = self.cfg.fixed_os() else {
            return true;
        };
        match avail.get(os.cfg_name()) {
            None | Some(OsAvailability::Unavailable) => {
                self.diags.push(
                    Diagnostic::error("V6031", format!("`{what}`在{os}上不可用"))
                        .with_label(span, "")
                        .with_note("请放在只包含可用系统的#if条件中"),
                );
                false
            }
            Some(OsAvailability::Available {
                introduced,
                deprecated,
            }) => {
                let deployment = self.target.deployment.get(&os).copied().unwrap_or_default();
                let effective = sc.min_version(os, deployment);
                if let Some(v) = introduced
                    && *v > effective
                {
                    self.diags.push(
                        Diagnostic::error("V6030", format!("`{what}`需要{os} {v}，当前最低版本为{effective}"))
                            .with_label(span, "")
                            .with_note(format!("写在`if available({os}: {v}) {{ ... }}`中，或提高[swiftui] deployment_targets")),
                    );
                    return false;
                }
                if let Some(d) = deprecated
                    && (d.major == 0 || *d <= effective)
                {
                    let when = if d.major == 0 {
                        String::new()
                    } else {
                        format!("自{os} {d}起")
                    };
                    self.diags.push(
                        Diagnostic::warning("V6032", format!("`{what}`{when}已废弃"))
                            .with_label(span, ""),
                    );
                }
                true
            }
        }
    }

    /// 检查一个实参，错误写入诊断
    fn framework_value(
        &mut self,
        sc: &mut Scope,
        ty: &ParamType,
        e: &'a ast::Expr,
    ) -> Option<FrameworkValue> {
        let before = self.diags.error_count();
        let value = match ty {
            ParamType::Optional(inner) => {
                if matches!(e.kind, A::Nil) {
                    let t = base_type(inner)
                        .map(Type::optional)
                        .unwrap_or(Type::Unknown);
                    return Some(FrameworkValue::Expr(
                        ir::Expr::new(ir::ExprKind::Nil, t),
                        SwiftConv::None,
                    ));
                }
                // 可选参数也接受可选值，文本与其他类型按非可选参数检查
                let Some(t) = base_type(inner).filter(|_| !matches!(**inner, ParamType::Text(_)))
                else {
                    return self.framework_value(sc, inner, e);
                };
                let mut v = self.expr(sc, e, Some(&Type::optional(t.clone())));
                if t == Type::Double {
                    v = crate::expr::promote(v);
                }
                let conv = match (&**inner, v.ty.is_optional()) {
                    (ParamType::Double | ParamType::Float, true) => SwiftConv::OptionalNumber,
                    (ParamType::Url, true) => SwiftConv::OptionalUrl,
                    (ParamType::Float, false) => SwiftConv::Float,
                    (ParamType::Url, false) => SwiftConv::Url,
                    _ => SwiftConv::None,
                };
                FrameworkValue::Expr(v, conv)
            }
            ParamType::Bool | ParamType::Int | ParamType::String | ParamType::Date => {
                let t = base_type(ty).unwrap_or(Type::Unknown);
                FrameworkValue::Expr(self.expr(sc, e, Some(&t)), SwiftConv::None)
            }
            ParamType::Double => FrameworkValue::Expr(
                crate::expr::promote(self.expr(sc, e, Some(&Type::Double))),
                SwiftConv::None,
            ),
            ParamType::Float => FrameworkValue::Expr(
                crate::expr::promote(self.expr(sc, e, Some(&Type::Double))),
                SwiftConv::Float,
            ),
            ParamType::Url => {
                FrameworkValue::Expr(self.expr(sc, e, Some(&Type::String)), SwiftConv::Url)
            }
            ParamType::Number => {
                let v = self.expr(sc, e, None);
                if !v.ty.is_numeric() && !v.ty.is_unknown() {
                    self.mismatch(e.span, &Type::Double, &v.ty);
                }
                FrameworkValue::Expr(v, SwiftConv::None)
            }
            ParamType::Any => FrameworkValue::Expr(self.expr(sc, e, None), SwiftConv::None),
            ParamType::Text(kind) => {
                let conv = match kind {
                    TextKind::String => SwiftConv::String,
                    TextKind::Text => SwiftConv::Text,
                    TextKind::LocalizedKey => SwiftConv::LocalizedKey,
                };
                FrameworkValue::Expr(self.expr(sc, e, Some(&Type::String)), conv)
            }
            ParamType::Binding(inner) => {
                let want = match &**inner {
                    ParamType::Number | ParamType::Any => Type::Unknown,
                    other => base_type(other).unwrap_or(Type::Unknown),
                };
                let b = self.binding_ref(sc, e, &want)?;
                if matches!(**inner, ParamType::Number) {
                    let t = match &b {
                        ir::BindingRef::State { ty, .. } | ir::BindingRef::Param { ty, .. } => {
                            ty.clone()
                        }
                    };
                    if !t.is_numeric() && !t.is_unknown() {
                        self.mismatch(e.span, &Type::Double, &t);
                    }
                }
                FrameworkValue::Binding(b)
            }
            ParamType::Action { is_async } => FrameworkValue::Action {
                action: self.action_ref(sc, e)?,
                is_async: *is_async,
            },
            ParamType::Content | ParamType::View => {
                let A::Closure(c) = &e.kind else {
                    self.error("V2076", "此处需要视图闭包", e.span, "例如`{ Text(\"x\") }`");
                    return None;
                };
                return self.content(sc, ty, c);
            }
            ParamType::Token(owner) | ParamType::Style(owner) => self.token_value(sc, owner, e)?,
        };
        (self.diags.error_count() == before).then_some(value)
    }

    fn content(
        &mut self,
        sc: &mut Scope,
        ty: &ParamType,
        c: &'a ast::Closure,
    ) -> Option<FrameworkValue> {
        if !c.params.is_empty() {
            self.error("V2038", "视图闭包不接受参数", c.params[0].span, "");
            return None;
        }
        let before = self.diags.error_count();
        let nodes = self.view_nodes(sc, &c.body);
        if self.diags.error_count() != before {
            return None;
        }
        Some(if matches!(ty, ParamType::View) {
            FrameworkValue::View(nodes)
        } else {
            FrameworkValue::Content(nodes)
        })
    }

    /// `.member`或`.member(args)`
    fn token_value(
        &mut self,
        sc: &mut Scope,
        owner: &str,
        e: &'a ast::Expr,
    ) -> Option<FrameworkValue> {
        let (id, args) = match &e.kind {
            A::ImplicitMember(id) => (id, None),
            A::Call(callee, args) => match &callee.kind {
                A::ImplicitMember(id) => (id, Some(args.as_slice())),
                _ => return self.not_token(owner, e.span),
            },
            _ => return self.not_token(owner, e.span),
        };
        let catalog = veyra_catalog::swiftui();
        let Some(members) = catalog.members(owner) else {
            return self.not_token(owner, e.span);
        };
        let Some(overloads) = members.get(&id.name) else {
            let mut names: Vec<_> = members.keys().map(|k| format!(".{k}")).collect();
            names.truncate(30);
            self.diags.push(
                Diagnostic::error("V2044", format!("`{owner}`没有成员`.{}`", id.name))
                    .with_label(id.span, "")
                    .with_note(format!("可用取值：{}", names.join("、"))),
            );
            return None;
        };
        let what = format!("{owner}.{}", id.name);
        match args {
            None => {
                let Some(m) = overloads.iter().find(|m| m.params.is_none()) else {
                    self.error("V2032", format!("`.{}`需要参数", id.name), id.span, "");
                    return None;
                };
                self.check_availability(sc, &what, &m.availability, id.span)
                    .then_some(FrameworkValue::Token {
                        member: id.name.clone(),
                        args: None,
                    })
            }
            Some(args) => {
                let sigs: Vec<Signature> = overloads
                    .iter()
                    .filter_map(|m| {
                        m.params.clone().map(|params| Signature {
                            params,
                            availability: m.availability.clone(),
                        })
                    })
                    .collect();
                let (_, call_args, _) =
                    self.resolve(sc, &format!(".{}", id.name), &sigs, args, None, e.span)?;
                Some(FrameworkValue::Token {
                    member: id.name.clone(),
                    args: Some(call_args),
                })
            }
        }
    }

    fn not_token(&mut self, owner: &str, span: Span) -> Option<FrameworkValue> {
        self.error(
            "V2044",
            format!("此处需要`{owner}`的取值"),
            span,
            "例如`.automatic`",
        );
        None
    }

    /// 在重载中选择第一个结构匹配且类型检查通过的签名
    pub(crate) fn resolve(
        &mut self,
        sc: &mut Scope,
        what: &str,
        overloads: &[Signature],
        args: &'a [Arg],
        trailing: Option<&'a ast::Closure>,
        span: Span,
    ) -> Option<(usize, Vec<FrameworkArg>, bool)> {
        // 文本参数优先选择StringProtocol，避免把已本地化的文案再次当作key
        let penalty = |sig: &Signature| -> usize {
            fn of(t: &ParamType) -> usize {
                match t {
                    ParamType::Text(TextKind::LocalizedKey) => 2,
                    ParamType::Text(TextKind::Text) => 1,
                    ParamType::Optional(inner) => of(inner),
                    _ => 0,
                }
            }
            sig.params.iter().map(|p| of(&p.ty)).sum()
        };
        let mut order: Vec<usize> = (0..overloads.len()).collect();
        order.sort_by_key(|i| penalty(&overloads[*i]));
        let mut first_candidate = None;
        for i in order {
            let sig = &overloads[i];
            let Some(matched) = match_params(&sig.params, args, trailing) else {
                continue;
            };
            first_candidate.get_or_insert(i);
            let mark = self.diags.len();
            if let Some(out) = self.check_matched(sc, what, sig, &matched, span) {
                return Some((i, out.0, out.1));
            }
            self.diags.truncate(mark);
        }
        match first_candidate {
            // 用第一个结构匹配的重载报告具体错误
            Some(i) => {
                let sig = &overloads[i];
                if let Some(matched) = match_params(&sig.params, args, trailing) {
                    self.check_matched(sc, what, sig, &matched, span);
                }
            }
            None => {
                let mut list: Vec<_> = overloads
                    .iter()
                    .map(|s| describe_signature(what, &s.params))
                    .collect();
                list.dedup();
                list.truncate(8);
                self.diags.push(
                    Diagnostic::error("V2075", format!("`{what}`没有与实参匹配的重载"))
                        .with_label(span, "")
                        .with_note(format!("可用的重载：\n  {}", list.join("\n  "))),
                );
            }
        }
        None
    }

    fn check_matched(
        &mut self,
        sc: &mut Scope,
        what: &str,
        sig: &Signature,
        matched: &Matched<'a>,
        span: Span,
    ) -> Option<(Vec<FrameworkArg>, bool)> {
        let before = self.diags.error_count();
        let mut out = Vec::new();
        for (pi, expr) in matched {
            let p = &sig.params[*pi];
            let Some(src) = expr else { continue };
            let value = match src {
                Source::Expr(e) => self.framework_value(sc, &p.ty, e),
                Source::Trailing(c) => self.content(sc, &p.ty, c),
            };
            if let Some(value) = value {
                out.push(FrameworkArg {
                    label: p.label.clone(),
                    value,
                });
            }
        }
        self.check_availability(sc, what, &sig.availability, span);
        let trailing = sig
            .params
            .last()
            .is_some_and(|p| matches!(p.ty, ParamType::Content))
            && out
                .last()
                .is_some_and(|a| matches!(a.value, FrameworkValue::Content(_)))
            && out
                .iter()
                .filter(|a| matches!(a.value, FrameworkValue::Content(_)))
                .count()
                == 1;
        (self.diags.error_count() == before).then_some((out, trailing))
    }

    /// `swiftui.Name(...) { ... }`
    pub(crate) fn swiftui_element(
        &mut self,
        sc: &mut Scope,
        el: &'a ast::Element,
        name: &ast::Ident,
    ) -> Option<ir::Element> {
        let catalog = veyra_catalog::swiftui();
        let what = format!("swiftui.{}", name.name);
        let Some(overloads) = catalog.views.get(&name.name) else {
            self.diags.push(
                Diagnostic::error("V1011", format!("SwiftUI中没有可用的组件`{}`", name.name))
                    .with_label(name.span, "")
                    .with_note("目录只收录参数能映射到Veyra类型的API，可以用veyra-catalog --explain查看原因"),
            );
            return None;
        };
        let (_, args, trailing_content) = self.resolve(
            sc,
            &what,
            overloads,
            &el.args,
            el.closure.as_ref(),
            name.span,
        )?;
        let modifiers = self.modifiers(sc, &el.modifiers, None);
        Some(ir::Element {
            kind: ir::ElementKind::SwiftUi(FrameworkCall {
                name: name.name.clone(),
                args,
                trailing_content,
            }),
            args: Default::default(),
            children: Vec::new(),
            item: None,
            modifiers,
        })
    }

    /// `.swiftui.name(...)`
    pub(crate) fn swiftui_modifier(
        &mut self,
        sc: &mut Scope,
        m: &'a ast::Modifier,
        name: &ast::Ident,
    ) -> Option<FrameworkCall> {
        let catalog = veyra_catalog::swiftui();
        let what = format!(".swiftui.{}", name.name);
        let Some(overloads) = catalog.modifiers.get(&name.name) else {
            self.diags.push(
                Diagnostic::error(
                    "V1012",
                    format!("SwiftUI中没有可用的修饰符`.{}`", name.name),
                )
                .with_label(name.span, "")
                .with_note(
                    "目录只收录参数能映射到Veyra类型的API，可以用veyra-catalog --explain查看原因",
                ),
            );
            return None;
        };
        let (_, args, trailing_content) =
            self.resolve(sc, &what, overloads, &m.args, None, name.span)?;
        Some(FrameworkCall {
            name: name.name.clone(),
            args,
            trailing_content,
        })
    }

    /// `if available(ios: 26, macos: 26)`的版本列表
    pub(crate) fn available_versions(
        &mut self,
        args: &[Arg],
        span: Span,
    ) -> Vec<(ir::AppleOs, Version)> {
        let mut out = Vec::new();
        for a in args {
            let Some(label) = &a.label else {
                self.error(
                    "V6033",
                    "available的参数必须写成`系统: 版本`",
                    a.span,
                    "例如`ios: 26`",
                );
                continue;
            };
            let Some(os) = ir::AppleOs::from_cfg_name(&label.name) else {
                self.error(
                    "V6033",
                    format!("未知的系统`{}`", label.name),
                    label.span,
                    "可用的系统：ios、macos、watchos、tvos、visionos",
                );
                continue;
            };
            let v = match &a.value.kind {
                A::Int(n) => Version::parse(&n.to_string()),
                A::Float(_, raw) => Version::parse(raw),
                _ => None,
            };
            match v {
                Some(v) => out.push((os, v)),
                None => self.error(
                    "V6033",
                    "版本必须是数字字面量",
                    a.value.span,
                    "例如`26`或`17.4`",
                ),
            }
        }
        if out.is_empty() {
            self.error("V6033", "available至少需要一个系统版本", span, "");
        }
        out
    }
}
