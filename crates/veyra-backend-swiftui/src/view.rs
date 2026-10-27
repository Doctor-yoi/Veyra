// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 视图树到SwiftUI的映射

use veyra_codegen::CodeWriter;
use veyra_ir::{
    ActionRef, BindingRef, Builtin, Element, ElementKind, Expr, ExprKind, FrameworkCall,
    FrameworkValue, Modifier, ModifierKind, NativeSegment, SwiftConv, Type, Value, ViewNode,
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
            ViewNode::Available {
                versions,
                then,
                else_,
            } => {
                let list: Vec<_> = versions
                    .iter()
                    .map(|(os, v)| format!("{} {v}", os.swift_name()))
                    .collect();
                w.line(format!("if #available({}, *) {{", list.join(", ")));
                w.indent();
                self::nodes(w, cx, then);
                w.dedent();
                if else_.is_empty() {
                    w.line("}");
                } else {
                    w.line("} else {");
                    w.indent();
                    self::nodes(w, cx, else_);
                    w.dedent();
                    w.line("}");
                }
            }
            ViewNode::Cond { cond, then, else_ } => {
                w.line(format!("#if {}", cond.swift()));
                self::nodes(w, cx, then);
                if !else_.is_empty() {
                    w.line("#else");
                    self::nodes(w, cx, else_);
                }
                w.line("#endif");
            }
            ViewNode::Native(segments) => {
                let mut code = String::new();
                for seg in segments {
                    match seg {
                        NativeSegment::Text(t) => code.push_str(t),
                        NativeSegment::Expr(e) => code.push_str(&swift::expr(cx.expr, e)),
                        NativeSegment::Action(a) => {
                            code.push_str(&format!("{}.{}", cx.vm, swift::ident(a)))
                        }
                        NativeSegment::Callback(c) => code.push_str(&swift::ident(c)),
                    }
                }
                w.line(code);
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
    let b = match &el.kind {
        ElementKind::Builtin(b) => b,
        ElementKind::SwiftUi(call) => return framework_element(w, cx, call, el),
        ElementKind::Android(_) => return,
        ElementKind::Component(name) => {
            // 按声明顺序传参，与结构体的成员构造器一致
            let args: Vec<_> = el
                .args
                .iter()
                .map(|(n, v)| {
                    let value = match v {
                        Value::Expr(x) => swift::expr(cx.expr, x),
                        Value::Action(_) => format!("{{ {} }}", action(cx, Some(v))),
                        Value::Binding(_) => binding(cx, Some(v)),
                        Value::KeyPath(_) | Value::Token(_) => String::new(),
                    };
                    format!("{}: {value}", swift::ident(n))
                })
                .collect();
            return leaf(w, cx, format!("{name}({})", args.join(", ")), el);
        }
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
            let horizontal = arg("axis").and_then(Value::as_token) == Some("horizontal");
            let mut params = Vec::new();
            if horizontal {
                params.push(".horizontal".to_string());
            }
            if let Some(i) = arg("indicators").and_then(Value::as_expr) {
                params.push(format!("showsIndicators: {}", swift::expr(cx.expr, i)));
            }
            let head = if params.is_empty() {
                "ScrollView".to_string()
            } else {
                format!("ScrollView({})", params.join(", "))
            };
            w.line(format!("{head} {{"));
            w.indent();
            // 单个容器子节点直接放入，否则按方向包一层栈
            match el.children.as_slice() {
                [ViewNode::Element(child)] if is_stack(child) => nodes(w, cx, &el.children),
                _ => {
                    w.line(if horizontal { "HStack {" } else { "VStack {" });
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
        Builtin::LazyColumn | Builtin::LazyRow | Builtin::LazyGrid => {
            let key = arg("key").and_then(Value::as_key_path).unwrap_or("id");
            let item = el
                .item
                .as_ref()
                .map(|(n, _)| swift::ident(n))
                .unwrap_or_else(|| "item".into());
            let spacing = number(cx, arg("spacing"));
            let (scroll, stack) = match b {
                Builtin::LazyRow => (
                    "ScrollView(.horizontal)",
                    format!(
                        "LazyHStack{}",
                        spacing
                            .as_ref()
                            .map(|s| format!("(spacing: {s})"))
                            .unwrap_or_default()
                    ),
                ),
                Builtin::LazyColumn => (
                    "ScrollView",
                    format!(
                        "LazyVStack{}",
                        spacing
                            .as_ref()
                            .map(|s| format!("(spacing: {s})"))
                            .unwrap_or_default()
                    ),
                ),
                _ => {
                    // 列间距在GridItem上，行间距在LazyVGrid上
                    let item_spacing = spacing
                        .as_ref()
                        .map(|s| format!(", spacing: {s}"))
                        .unwrap_or_default();
                    let columns = match (
                        arg("minWidth").and_then(Value::as_expr),
                        arg("columns").and_then(Value::as_expr),
                    ) {
                        (Some(min), _) => format!(
                            "[GridItem(.adaptive(minimum: {}){item_spacing})]",
                            swift::expr(cx.expr, min)
                        ),
                        (None, Some(n)) => format!(
                            "Array(repeating: GridItem(.flexible(){item_spacing}), count: {})",
                            swift::expr(cx.expr, n)
                        ),
                        (None, None) => format!(
                            "Array(repeating: GridItem(.flexible(){item_spacing}), count: 2)"
                        ),
                    };
                    let spacing = spacing
                        .as_ref()
                        .map(|s| format!(", spacing: {s}"))
                        .unwrap_or_default();
                    (
                        "ScrollView",
                        format!("LazyVGrid(columns: {columns}{spacing})"),
                    )
                }
            };
            w.line(format!("{scroll} {{"));
            w.indent();
            w.line(format!("{stack} {{"));
            w.indent();
            w.line(format!(
                "ForEach({}, id: \\.{}) {{ {item} in",
                e(cx, arg("_")),
                swift::ident(key)
            ));
            w.indent();
            nodes(w, cx, &el.children);
            w.dedent();
            w.line("}");
            w.dedent();
            w.line("}");
            w.dedent();
            w.line("}");
            trailing_modifiers(w, cx, &el.modifiers);
        }
        Builtin::Section => {
            w.line("Section {");
            w.indent();
            nodes(w, cx, &el.children);
            w.dedent();
            let mut tail = "}".to_string();
            for (label, key) in [("header", "header"), ("footer", "footer")] {
                if let Some(Value::Expr(x)) = arg(key) {
                    w.line(format!("{tail} {label}: {{"));
                    w.indent();
                    w.line(format!("Text({})", swift::expr(cx.expr, x)));
                    w.dedent();
                    tail = "}".to_string();
                }
            }
            w.line(tail);
            trailing_modifiers(w, cx, &el.modifiers);
        }
        Builtin::Form => container(w, cx, "Form", None, el),
        Builtin::GroupBox => {
            let head = match arg("_") {
                Some(v) => format!("GroupBox({})", text_arg(cx, Some(v))),
                None => "GroupBox".to_string(),
            };
            container(w, cx, &head, None, el);
        }
        Builtin::DisclosureGroup => {
            let head = match arg("isExpanded") {
                Some(b) => format!(
                    "DisclosureGroup({}, isExpanded: {})",
                    text_arg(cx, arg("_")),
                    binding(cx, Some(b))
                ),
                None => format!("DisclosureGroup({})", text_arg(cx, arg("_"))),
            };
            container(w, cx, &head, None, el);
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
            let role = match arg("role").and_then(Value::as_token) {
                Some(r) => format!(", role: .{r}"),
                None => String::new(),
            };
            w.line(format!("Button({}{role}) {{", text_arg(cx, arg("_"))));
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
            let mut parts = Vec::new();
            if let Some(l) = arg("_") {
                parts.push(text_arg(cx, Some(l)));
            }
            if let Some(v) = number(cx, arg("value")) {
                parts.push(format!("value: {v}"));
                if let Some(t) = number(cx, arg("total")) {
                    parts.push(format!("total: {t}"));
                }
            }
            w.line(format!("ProgressView({})", parts.join(", ")));
            w.indent();
            if let Some(style) = arg("style").and_then(Value::as_token) {
                w.line(format!(".progressViewStyle(.{style})"));
            }
            trailing_modifiers(w, cx, &el.modifiers);
            w.dedent();
        }
        Builtin::Picker => {
            let selection = match arg("selection").and_then(Value::as_binding) {
                Some(BindingRef::State { ty, .. } | BindingRef::Param { ty, .. }) => ty.clone(),
                None => Type::Unknown,
            };
            w.line(format!(
                "Picker({}, selection: {}) {{",
                text_arg(cx, arg("_")),
                binding(cx, arg("selection"))
            ));
            w.indent();
            picker_options(w, cx, &el.children, &selection);
            w.dedent();
            w.line("}");
            match arg("style").and_then(Value::as_token) {
                Some("automatic") | None => {}
                Some(style) => w.line(format!(".pickerStyle(.{style})")),
            }
            trailing_modifiers(w, cx, &el.modifiers);
        }
        // 只会出现在Picker中，由picker_options生成
        Builtin::Option => {}
        Builtin::DatePicker => {
            let components = match arg("components").and_then(Value::as_token) {
                Some("date") => ", displayedComponents: [.date]",
                Some("time") => ", displayedComponents: [.hourAndMinute]",
                _ => "",
            };
            w.line(format!(
                "DatePicker({}, selection: {}{components})",
                text_arg(cx, arg("_")),
                binding(cx, arg("selection"))
            ));
            w.indent();
            match arg("style").and_then(Value::as_token) {
                Some("automatic") | None => {}
                Some(style) => w.line(format!(".datePickerStyle(.{style})")),
            }
            trailing_modifiers(w, cx, &el.modifiers);
            w.dedent();
        }
        Builtin::Stepper => {
            let double = matches!(
                arg("value").and_then(Value::as_binding),
                Some(BindingRef::State { ty, .. } | BindingRef::Param { ty, .. }) if *ty == Type::Double
            );
            let mut text = format!(
                "Stepper({}, value: {}",
                text_arg(cx, arg("_")),
                binding(cx, arg("value"))
            );
            let (min, max) = (number(cx, arg("min")), number(cx, arg("max")));
            if min.is_some() || max.is_some() {
                let (lo, hi) = if double {
                    ("-Double.infinity", "Double.infinity")
                } else {
                    ("Int.min", "Int.max")
                };
                text.push_str(&format!(
                    ", in: {}...{}",
                    min.unwrap_or_else(|| lo.into()),
                    max.unwrap_or_else(|| hi.into())
                ));
            }
            if let Some(step) = number(cx, arg("step")) {
                text.push_str(&format!(", step: {step}"));
            }
            text.push(')');
            leaf(w, cx, text, el);
        }
        Builtin::Menu => {
            let head = format!("Menu({})", text_arg(cx, arg("_")));
            container(w, cx, &head, None, el);
        }
        Builtin::Link => leaf(
            w,
            cx,
            format!(
                "Link({}, destination: URL(string: {})!)",
                text_arg(cx, arg("_")),
                e(cx, arg("url"))
            ),
            el,
        ),
        Builtin::TextEditor => leaf(
            w,
            cx,
            format!("TextEditor(text: {})", binding(cx, arg("text"))),
            el,
        ),
        Builtin::Label => leaf(
            w,
            cx,
            format!(
                "Label({}, systemImage: {})",
                text_arg(cx, arg("_")),
                e(cx, arg("icon"))
            ),
            el,
        ),
        Builtin::Gauge => {
            let min = number(cx, arg("min")).unwrap_or_else(|| double_literal(0.0));
            let max = number(cx, arg("max")).unwrap_or_else(|| double_literal(1.0));
            let label = match arg("label").and_then(Value::as_expr) {
                Some(x) => text_view(cx, x),
                None => "EmptyView()".into(),
            };
            w.line(format!(
                "Gauge(value: {}, in: {min}...{max}) {{ {label} }}",
                e(cx, arg("value"))
            ));
            modifiers_only(w, cx, &el.modifiers);
        }
    }
}

/// `Text`视图，字面量不做本地化查找
fn text_view(cx: ViewCx, x: &Expr) -> String {
    match &x.kind {
        ExprKind::String(s) => format!("Text(verbatim: {})", swift::string_literal(s)),
        _ => format!("Text({})", swift::expr(cx.expr, x)),
    }
}

/// Picker的选项，tag的类型需要与selection完全一致
fn picker_options(w: &mut CodeWriter, cx: ViewCx, nodes: &[ViewNode], selection: &Type) {
    let optional = selection.is_optional();
    for node in nodes {
        let ViewNode::Element(el) = node else {
            continue;
        };
        match el.kind {
            ElementKind::Builtin(Builtin::Option) => {
                let label = match el.arg("_").and_then(Value::as_expr) {
                    Some(x) => text_view(cx, x),
                    None => "Text(verbatim: \"\")".into(),
                };
                w.line(label);
                w.indent();
                let value = match el.arg("value").and_then(Value::as_expr) {
                    Some(Expr {
                        kind: ExprKind::Nil,
                        ..
                    }) => format!("nil as {}", swift::ty(selection)),
                    Some(x) if optional && !x.ty.is_optional() => {
                        format!("Optional({})", swift::expr(cx.expr, x))
                    }
                    Some(x) => swift::expr(cx.expr, x),
                    None => "0".into(),
                };
                w.line(format!(".tag({value})"));
                trailing_modifiers(w, cx, &el.modifiers);
                w.dedent();
            }
            ElementKind::Builtin(Builtin::ForEach) => {
                let key = el.arg("key").and_then(Value::as_key_path).unwrap_or("id");
                let item = el
                    .item
                    .as_ref()
                    .map(|(n, _)| swift::ident(n))
                    .unwrap_or_else(|| "item".into());
                w.line(format!(
                    "ForEach({}, id: \\.{}) {{ {item} in",
                    e(cx, el.arg("_")),
                    swift::ident(key)
                ));
                w.indent();
                picker_options(w, cx, &el.children, selection);
                w.dedent();
                w.line("}");
            }
            _ => {}
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
        let lines = modifier(cx, m);
        if lines.is_empty() {
            continue;
        }
        // 后缀表达式中的#if，需要Swift 5.5及以上
        if let Some(cond) = &m.cond {
            w.line(format!("#if {}", cond.swift()));
        }
        for line in lines {
            w.line(line);
        }
        if m.cond.is_some() {
            w.line("#endif");
        }
    }
}

fn modifiers_only(w: &mut CodeWriter, cx: ViewCx, mods: &[Modifier]) {
    w.indent();
    trailing_modifiers(w, cx, mods);
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
        ModifierKind::Refreshable => {
            let call = match arg("_").and_then(Value::as_action) {
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
                        format!("await {call}")
                    } else {
                        call
                    }
                }
                Some(ActionRef::Param(name)) => format!("{}()", swift::ident(name)),
                None => String::new(),
            };
            vec![format!(".refreshable {{ {call} }}")]
        }
        ModifierKind::Searchable => {
            let mut s = format!(".searchable(text: {}", binding(cx, arg("text")));
            if let Some(p) = arg("prompt") {
                s.push_str(&format!(", prompt: {}", text_arg(cx, Some(p))));
            }
            s.push(')');
            vec![s]
        }
        ModifierKind::Keyboard => {
            let t = match arg("_").and_then(Value::as_token).unwrap_or("default") {
                "email" => "emailAddress",
                "number" => "numberPad",
                "decimal" => "decimalPad",
                "phone" => "phonePad",
                "url" => "URL",
                _ => "default",
            };
            vec![format!(".keyboardType(.{t})")]
        }
        ModifierKind::SubmitLabel => vec![format!(
            ".submitLabel(.{})",
            arg("_").and_then(Value::as_token).unwrap_or("done")
        )],
        ModifierKind::OnSubmit => vec![format!(".onSubmit {{ {} }}", action(cx, arg("_")))],
        ModifierKind::Autocapitalization => vec![format!(
            ".textInputAutocapitalization(.{})",
            arg("_").and_then(Value::as_token).unwrap_or("sentences")
        )],
        ModifierKind::SwiftUi => match &m.call {
            Some(call) => vec![format!(".{}", framework_call(cx, call))],
            None => Vec::new(),
        },
        ModifierKind::Android(_) => Vec::new(),
    }
}

/// 视图闭包的文本，多行内容由CodeWriter逐行缩进
fn closure_text(cx: ViewCx, nodes: &[ViewNode]) -> String {
    let mut w = CodeWriter::new("    ");
    w.line("{");
    w.indent();
    self::nodes(&mut w, cx, nodes);
    w.dedent();
    w.line("}");
    w.finish().trim_end().to_string()
}

fn conv(cx: ViewCx, e: &Expr, c: SwiftConv) -> String {
    match c {
        SwiftConv::None => swift::expr(cx.expr, e),
        SwiftConv::String => plain_string_arg(cx.expr, e),
        SwiftConv::Text => match &e.kind {
            ExprKind::String(s) => format!("Text(verbatim: {})", swift::string_literal(s)),
            _ => format!("Text({})", swift::expr(cx.expr, e)),
        },
        SwiftConv::LocalizedKey => format!("LocalizedStringKey({})", swift::expr(cx.expr, e)),
        SwiftConv::Float => format!("Float({})", swift::expr(cx.expr, e)),
        SwiftConv::Url => format!("URL(string: {})!", swift::expr(cx.expr, e)),
        SwiftConv::OptionalNumber => format!("{}.map {{ .init($0) }}", swift::postfix(cx.expr, e)),
        SwiftConv::OptionalUrl => format!(
            "{}.flatMap {{ URL(string: $0) }}",
            swift::postfix(cx.expr, e)
        ),
    }
}

fn framework_value(cx: ViewCx, v: &FrameworkValue) -> String {
    match v {
        FrameworkValue::Expr(e, c) => conv(cx, e, *c),
        FrameworkValue::Action { action, is_async } => {
            let stmt = match action {
                ActionRef::Dispatch {
                    action,
                    args,
                    is_async: target_async,
                } => {
                    let args: Vec<_> = args
                        .iter()
                        .map(|(n, v)| format!("{}: {}", n, swift::expr(cx.expr, v)))
                        .collect();
                    let call = format!("{}.{}({})", cx.vm, swift::ident(action), args.join(", "));
                    match (is_async, target_async) {
                        (true, true) => format!("await {call}"),
                        (false, true) => format!("Task {{ await {call} }}"),
                        _ => call,
                    }
                }
                ActionRef::Param(name) => format!("{}()", swift::ident(name)),
            };
            format!("{{ {stmt} }}")
        }
        FrameworkValue::Binding(b) => binding(cx, Some(&Value::Binding(b.clone()))),
        FrameworkValue::Token { member, args: None } => format!(".{}", swift::ident(member)),
        FrameworkValue::Token {
            member,
            args: Some(args),
        } => {
            format!(".{}({})", swift::ident(member), framework_args(cx, args))
        }
        FrameworkValue::Content(nodes) => closure_text(cx, nodes),
        FrameworkValue::View(nodes) => {
            let text = closure_text(cx, nodes);
            match nodes.as_slice() {
                [_] if text.lines().count() == 3 => {
                    text.lines().nth(1).unwrap_or_default().trim().to_string()
                }
                _ => format!("Group {text}"),
            }
        }
    }
}

fn framework_args(cx: ViewCx, args: &[veyra_ir::FrameworkArg]) -> String {
    args.iter()
        .map(|a| {
            let v = framework_value(cx, &a.value);
            match &a.label {
                Some(l) => format!("{l}: {v}"),
                None => v,
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// `name(args)`，单个视图内容放在尾随闭包中
fn framework_call(cx: ViewCx, call: &FrameworkCall) -> String {
    let (args, trailing) = match (call.trailing_content, call.args.split_last()) {
        (true, Some((last, rest))) => (rest, Some(&last.value)),
        _ => (call.args.as_slice(), None),
    };
    let head = if args.is_empty() && trailing.is_some() {
        call.name.clone()
    } else {
        format!("{}({})", call.name, framework_args(cx, args))
    };
    match trailing {
        Some(FrameworkValue::Content(nodes)) => format!("{head} {}", closure_text(cx, nodes)),
        _ => head,
    }
}

fn framework_element(w: &mut CodeWriter, cx: ViewCx, call: &FrameworkCall, el: &Element) {
    let text = framework_call(cx, call);
    if text.ends_with('}') {
        w.line(text);
        trailing_modifiers(w, cx, &el.modifiers);
    } else {
        leaf(w, cx, text, el);
    }
}
