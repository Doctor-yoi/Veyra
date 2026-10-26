// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 视图树到SwiftUI的映射

use veyra_codegen::CodeWriter;
use veyra_ir::{
    ActionRef, BindingRef, Builtin, Element, ElementKind, Expr, ExprKind, Modifier, ModifierKind,
    Value, ViewNode,
};

use crate::swift::{self, ExprCx, double_literal, plain_string_arg};

/// 视图生成上下文
#[derive(Clone, Copy)]
pub(crate) struct ViewCx<'a> {
    pub expr: ExprCx<'a>,
    /// ViewModel变量名
    pub vm: &'a str,
}

pub(crate) fn nodes(w: &mut CodeWriter, cx: ViewCx, nodes: &[ViewNode]) {
    for node in nodes {
        match node {
            ViewNode::Element(el) => element(w, cx, el),
            ViewNode::If { cond, then, else_ } => {
                w.line(format!("if {} {{", swift::expr(cx.expr, cond)));
                if_tail(w, cx, then, else_);
            }
        }
    }
}

/// 输出if分支体，并把单个嵌套if展开为else if
fn if_tail(w: &mut CodeWriter, cx: ViewCx, then: &[ViewNode], else_: &[ViewNode]) {
    w.indent();
    nodes(w, cx, then);
    w.dedent();
    match else_ {
        [] => w.line("}"),
        [ViewNode::If { cond, then, else_ }] => {
            w.line(format!("}} else if {} {{", swift::expr(cx.expr, cond)));
            if_tail(w, cx, then, else_);
        }
        _ => {
            w.line("} else {");
            w.indent();
            nodes(w, cx, else_);
            w.dedent();
            w.line("}");
        }
    }
}

fn e(cx: ViewCx, v: Option<&Value>) -> String {
    match v.and_then(Value::as_expr) {
        Some(x) => swift::expr(cx.expr, x),
        None => String::new(),
    }
}

fn text_arg(cx: ViewCx, v: Option<&Value>) -> String {
    match v.and_then(Value::as_expr) {
        Some(x) => plain_string_arg(cx.expr, x),
        None => "\"\"".into(),
    }
}

/// 命令分发语句
pub(crate) fn action(cx: ViewCx, a: Option<&Value>) -> String {
    match a.and_then(Value::as_action) {
        Some(ActionRef::Dispatch {
            action,
            args,
            is_async,
        }) => {
            let args: Vec<_> = args
                .iter()
                .map(|(n, v)| format!("{}: {}", n, swift::expr(cx.expr, v)))
                .collect();
            let call = format!("{}.{}({})", cx.vm, swift::ident(action), args.join(", "));
            if *is_async {
                format!("Task {{ await {call} }}")
            } else {
                call
            }
        }
        Some(ActionRef::Param(name)) => format!("{}()", swift::ident(name)),
        None => String::new(),
    }
}

/// 经过setter写入的Binding
fn binding(cx: ViewCx, b: Option<&Value>) -> String {
    match b.and_then(Value::as_binding) {
        Some(BindingRef::State { field, setter, .. }) => format!(
            "Binding(get: {{ {}{} }}, set: {{ {}.{}($0) }})",
            cx.expr.state,
            swift::ident(field),
            cx.vm,
            swift::ident(setter)
        ),
        Some(BindingRef::Param { name, .. }) => format!("${name}"),
        None => ".constant(nil)".into(),
    }
}

fn number(cx: ViewCx, v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_expr).map(|x| swift::expr(cx.expr, x))
}

fn element(w: &mut CodeWriter, cx: ViewCx, el: &Element) {
    let ElementKind::Builtin(b) = &el.kind else {
        return;
    };
    let arg = |n: &str| el.arg(n);
    match b {
        Builtin::Column | Builtin::Row | Builtin::Stack => {
            let name = match b {
                Builtin::Column => "VStack",
                Builtin::Row => "HStack",
                _ => "ZStack",
            };
            let mut params = Vec::new();
            if let Some(a) = arg("alignment").and_then(Value::as_token) {
                params.push(format!("alignment: .{a}"));
            }
            if let Some(s) = number(cx, arg("spacing")) {
                params.push(format!("spacing: {s}"));
            }
            let head = if params.is_empty() {
                name.to_string()
            } else {
                format!("{name}({})", params.join(", "))
            };
            container(w, cx, &head, None, el);
        }
        Builtin::ScrollView => {
            w.line("ScrollView {");
            w.indent();
            // 单个容器子节点直接放入，否则包一层VStack
            match el.children.as_slice() {
                [ViewNode::Element(child)] if is_stack(child) => nodes(w, cx, &el.children),
                _ => {
                    w.line("VStack {");
                    w.indent();
                    nodes(w, cx, &el.children);
                    w.dedent();
                    w.line("}");
                }
            }
            w.dedent();
            w.line("}");
            trailing_modifiers(w, cx, &el.modifiers);
        }
        Builtin::List | Builtin::ForEach => {
            let name = if *b == Builtin::List {
                "List"
            } else {
                "ForEach"
            };
            let key = arg("key").and_then(Value::as_key_path).unwrap_or("id");
            let item = el
                .item
                .as_ref()
                .map(|(n, _)| swift::ident(n))
                .unwrap_or_else(|| "item".into());
            let head = format!("{name}({}, id: \\.{})", e(cx, arg("_")), swift::ident(key));
            container(w, cx, &head, Some(&item), el);
        }
        Builtin::Text => {
            let text = match arg("_").and_then(Value::as_expr) {
                Some(Expr {
                    kind: ExprKind::String(s),
                    ..
                }) => format!("Text(verbatim: {})", swift::string_literal(s)),
                _ => format!("Text({})", e(cx, arg("_"))),
            };
            leaf(w, cx, text, el);
        }
        Builtin::Button => {
            w.line(format!("Button({}) {{", text_arg(cx, arg("_"))));
            w.indent();
            w.line(action(cx, arg("action")));
            w.dedent();
            w.line("}");
            trailing_modifiers(w, cx, &el.modifiers);
        }
        Builtin::TextField | Builtin::SecureField => {
            let name = if *b == Builtin::TextField {
                "TextField"
            } else {
                "SecureField"
            };
            leaf(
                w,
                cx,
                format!(
                    "{name}({}, text: {})",
                    text_arg(cx, arg("_")),
                    binding(cx, arg("text"))
                ),
                el,
            );
        }
        Builtin::Toggle => {
            leaf(
                w,
                cx,
                format!(
                    "Toggle({}, isOn: {})",
                    text_arg(cx, arg("_")),
                    binding(cx, arg("isOn"))
                ),
                el,
            );
        }
        Builtin::Slider => {
            let min = number(cx, arg("min")).unwrap_or_else(|| double_literal(0.0));
            let max = number(cx, arg("max")).unwrap_or_else(|| double_literal(1.0));
            leaf(
                w,
                cx,
                format!(
                    "Slider(value: {}, in: {min}...{max})",
                    binding(cx, arg("value"))
                ),
                el,
            );
        }
        Builtin::Image => leaf(w, cx, format!("Image({})", e(cx, arg("_"))), el),
        Builtin::Spacer => leaf(w, cx, "Spacer()".into(), el),
        Builtin::Divider => leaf(w, cx, "Divider()".into(), el),
        Builtin::Progress => {
            let text = match number(cx, arg("value")) {
                Some(v) => format!("ProgressView(value: {v})"),
                None => "ProgressView()".into(),
            };
            leaf(w, cx, text, el);
        }
    }
}

fn leaf(w: &mut CodeWriter, cx: ViewCx, text: String, el: &Element) {
    w.line(text);
    modifiers_only(w, cx, &el.modifiers);
}

fn container(w: &mut CodeWriter, cx: ViewCx, head: &str, item: Option<&str>, el: &Element) {
    match item {
        Some(i) => w.line(format!("{head} {{ {i} in")),
        None => w.line(format!("{head} {{")),
    }
    w.indent();
    nodes(w, cx, &el.children);
    w.dedent();
    w.line("}");
    trailing_modifiers(w, cx, &el.modifiers);
}

fn is_stack(el: &Element) -> bool {
    matches!(
        el.kind,
        ElementKind::Builtin(Builtin::Column | Builtin::Row | Builtin::Stack | Builtin::List)
    )
}

/// 闭包之后的修饰符与右花括号对齐
fn trailing_modifiers(w: &mut CodeWriter, cx: ViewCx, mods: &[Modifier]) {
    for m in mods {
        for line in modifier(cx, m) {
            w.line(line);
        }
    }
}

fn modifiers_only(w: &mut CodeWriter, cx: ViewCx, mods: &[Modifier]) {
    w.indent();
    for m in mods {
        for line in modifier(cx, m) {
            w.line(line);
        }
    }
    w.dedent();
}

fn color(token: &str, shape: bool) -> String {
    match token {
        "primary" | "secondary" if shape => format!(".{token}"),
        "primary" | "secondary" => format!("Color.{token}"),
        "accent" if shape => ".tint".into(),
        "accent" => "Color.accentColor".into(),
        other => format!("Color.{other}"),
    }
}

fn modifier(cx: ViewCx, m: &Modifier) -> Vec<String> {
    let arg = |n: &str| m.arg(n);
    match m.kind {
        ModifierKind::Padding => match number(cx, arg("_")) {
            Some(v) => vec![format!(".padding({v})")],
            None => vec![".padding()".into()],
        },
        ModifierKind::Font => vec![format!(
            ".font(.{})",
            arg("_").and_then(Value::as_token).unwrap_or("body")
        )],
        ModifierKind::Foreground => {
            vec![format!(
                ".foregroundStyle({})",
                color(
                    arg("_").and_then(Value::as_token).unwrap_or("primary"),
                    true
                )
            )]
        }
        ModifierKind::Background => {
            vec![format!(
                ".background({})",
                color(arg("_").and_then(Value::as_token).unwrap_or("clear"), false)
            )]
        }
        ModifierKind::Frame => {
            let dim = |n: &str| match arg(n) {
                Some(Value::Token(t)) if t == "infinity" => Some(".infinity".to_string()),
                other => number(cx, other),
            };
            let mut out = Vec::new();
            let fixed: Vec<_> = ["width", "height"]
                .iter()
                .filter_map(|n| dim(n).map(|v| format!("{n}: {v}")))
                .collect();
            if !fixed.is_empty() {
                out.push(format!(".frame({})", fixed.join(", ")));
            }
            let max: Vec<_> = ["maxWidth", "maxHeight"]
                .iter()
                .filter_map(|n| dim(n).map(|v| format!("{n}: {v}")))
                .collect();
            if !max.is_empty() {
                out.push(format!(".frame({})", max.join(", ")));
            }
            out
        }
        ModifierKind::Disabled => vec![format!(".disabled({})", e(cx, arg("_")))],
        ModifierKind::Hidden => vec![format!(".opacity({} ? 0 : 1)", e(cx, arg("_")))],
        ModifierKind::OnTap => vec![format!(".onTapGesture {{ {} }}", action(cx, arg("_")))],
        ModifierKind::AccessibilityLabel => {
            vec![format!(".accessibilityLabel({})", text_arg(cx, arg("_")))]
        }
    }
}
