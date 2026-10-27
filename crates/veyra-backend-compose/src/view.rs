// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 视图树到Jetpack Compose的映射
//!
//! SwiftUI的修饰符由内向外包裹，Compose的Modifier链由外向内生效，
//! 因此链式修饰符需要倒序输出

use veyra_codegen::{CodeWriter, pascal_case};
use veyra_ir::{
    ActionRef, AndroidElement, AndroidModifier, BindingRef, Builtin, Element, ElementKind, Expr,
    ExprKind, Modifier, ModifierKind, NativeCond, NativeSegment, UnaryOp, Value, ViewNode,
};

use crate::kotlin::{self, ExprCx, Imports, dp, float, ident};

/// 父布局，决定可用的作用域修饰符
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Parent {
    Column,
    Row,
    Other,
}

pub(crate) struct ViewEmitter<'a> {
    pub cx: ExprCx<'a>,
    /// actions变量名
    pub actions: &'a str,
    pub im: &'a mut Imports,
    /// 提升出的局部变量计数
    pub temp: usize,
}

struct Call {
    name: String,
    args: Vec<String>,
}

impl Call {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            args: Vec::new(),
        }
    }

    fn arg(&mut self, name: &str, value: impl Into<String>) -> &mut Self {
        self.args.push(format!("{name} = {}", value.into()));
        self
    }

    fn render(&self, w: &mut CodeWriter, trailing: Option<&str>) {
        let suffix = trailing.map(|t| format!(" {t}")).unwrap_or_default();
        let single = format!("{}({}){suffix}", self.name, self.args.join(", "));
        if single.len() <= 100 || self.args.len() <= 1 {
            w.line(single);
            return;
        }
        w.line(format!("{}(", self.name));
        w.indent();
        for a in &self.args {
            w.line(format!("{a},"));
        }
        w.dedent();
        w.line(format!("){suffix}"));
    }
}

impl ViewEmitter<'_> {
    fn e(&mut self, x: &Expr) -> String {
        kotlin::expr(self.cx, x, self.im)
    }

    fn ev(&mut self, v: Option<&Value>) -> String {
        match v.and_then(Value::as_expr) {
            Some(x) => self.e(x),
            None => "\"\"".into(),
        }
    }

    pub fn nodes(&mut self, w: &mut CodeWriter, nodes: &[ViewNode], parent: Parent) {
        for node in nodes {
            match node {
                ViewNode::Element(el) => self.element(w, el, parent, None),
                ViewNode::If { cond, then, else_ } => {
                    let c = self.e(cond);
                    w.line(format!("if ({c}) {{"));
                    self.if_tail(w, then, else_, parent);
                }
                // Android上没有系统版本差异，直接使用较新版本的分支
                ViewNode::Available { then, .. } => self.nodes(w, then, parent),
                ViewNode::Cond { cond, then, else_ } => {
                    let c = self.native_cond(cond);
                    w.line(format!("if ({c}) {{"));
                    w.indent();
                    self.nodes(w, then, parent);
                    w.dedent();
                    if else_.is_empty() {
                        w.line("}");
                    } else {
                        w.line("} else {");
                        w.indent();
                        self.nodes(w, else_, parent);
                        w.dedent();
                        w.line("}");
                    }
                }
                ViewNode::Native(segments) => {
                    let mut code = String::new();
                    for seg in segments {
                        match seg {
                            NativeSegment::Text(t) => code.push_str(t),
                            NativeSegment::Expr(x) => {
                                let v = self.e(x);
                                code.push_str(&v);
                            }
                            NativeSegment::Action(a) => {
                                code.push_str(&format!("{}.{}", self.actions, ident(a)))
                            }
                            NativeSegment::Callback(c) => code.push_str(&ident(c)),
                        }
                    }
                    w.line(code);
                }
            }
        }
    }

    /// Android上只会出现debug条件，Apple系统条件在裁剪时已为假
    fn native_cond(&mut self, cond: &NativeCond) -> String {
        let flag = self.build_config();
        cond.render(&|c| match c {
            NativeCond::Debug => flag.clone(),
            _ => "false".into(),
        })
    }

    /// 应用的BuildConfig.DEBUG
    fn build_config(&mut self) -> String {
        let path = format!("{}.BuildConfig", self.im.r_package);
        self.im.add(&path);
        "BuildConfig.DEBUG".into()
    }

    fn if_tail(
        &mut self,
        w: &mut CodeWriter,
        then: &[ViewNode],
        else_: &[ViewNode],
        parent: Parent,
    ) {
        w.indent();
        self.nodes(w, then, parent);
        w.dedent();
        match else_ {
            [] => w.line("}"),
            [ViewNode::If { cond, then, else_ }] => {
                let c = self.e(cond);
                w.line(format!("}} else if ({c}) {{"));
                self.if_tail(w, then, else_, parent);
            }
            _ => {
                w.line("} else {");
                w.indent();
                self.nodes(w, else_, parent);
                w.dedent();
                w.line("}");
            }
        }
    }

    /// 命令分发语句
    pub fn action(&mut self, v: Option<&Value>) -> String {
        match v.and_then(Value::as_action) {
            Some(ActionRef::Dispatch { action, args, .. }) => {
                let parts: Vec<_> = args
                    .iter()
                    .map(|(n, x)| format!("{} = {}", ident(n), self.e(x)))
                    .collect();
                format!("{}.{}({})", self.actions, ident(action), parts.join(", "))
            }
            Some(ActionRef::Param(name)) => format!("{}()", ident(name)),
            None => String::new(),
        }
    }

    /// 返回绑定的当前值与写入回调
    fn binding(&mut self, v: Option<&Value>, convert: &str) -> (String, String) {
        match v.and_then(Value::as_binding) {
            Some(BindingRef::State { field, setter, .. }) => (
                format!("{}{}", self.cx.state, ident(field)),
                format!("{{ {}.{}(it{convert}) }}", self.actions, ident(setter)),
            ),
            Some(BindingRef::Param { name, .. }) => {
                let cb = format!("on{}Change", pascal_case(name));
                (ident(name), format!("{{ {cb}(it{convert}) }}"))
            }
            None => ("TODO()".into(), "{}".into()),
        }
    }

    /// 构建Modifier链，`scope`为父布局附加的作用域修饰符
    fn modifier_chain(
        &mut self,
        w: &mut CodeWriter,
        mods: &[Modifier],
        scope: Option<&str>,
        base: &str,
    ) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        if let Some(s) = scope {
            parts.push(s.to_string());
        }
        for m in mods.iter().rev() {
            let arg = |n: &str| m.arg(n);
            let before = parts.len();
            match m.kind {
                ModifierKind::Padding => {
                    self.im.add("androidx.compose.foundation.layout.padding");
                    let v = match arg("_").and_then(Value::as_expr) {
                        Some(x) => dp(self.cx, x, self.im),
                        None => {
                            self.im.add("androidx.compose.ui.unit.dp");
                            "16.dp".into()
                        }
                    };
                    parts.push(format!("padding({v})"));
                }
                ModifierKind::Background => {
                    self.im.add("androidx.compose.foundation.background");
                    let c = self.color(arg("_").and_then(Value::as_token).unwrap_or("clear"));
                    parts.push(format!("background({c})"));
                }
                ModifierKind::Frame => {
                    let dim = |name: &str, fill: &str, fixed: &str, max: bool, this: &mut Self| {
                        match arg(name) {
                            Some(Value::Token(t)) if t == "infinity" => {
                                this.im
                                    .add(&format!("androidx.compose.foundation.layout.{fill}"));
                                Some(format!("{fill}()"))
                            }
                            Some(Value::Expr(x)) => {
                                let f = if max {
                                    format!("{fixed}In")
                                } else {
                                    fixed.to_string()
                                };
                                this.im
                                    .add(&format!("androidx.compose.foundation.layout.{f}"));
                                let d = dp(this.cx, x, this.im);
                                Some(if max {
                                    format!("{f}(max = {d})")
                                } else {
                                    format!("{f}({d})")
                                })
                            }
                            _ => None,
                        }
                    };
                    let items = [
                        dim("maxWidth", "fillMaxWidth", "width", true, self),
                        dim("maxHeight", "fillMaxHeight", "height", true, self),
                        dim("width", "", "width", false, self),
                        dim("height", "", "height", false, self),
                    ];
                    parts.extend(items.into_iter().flatten());
                }
                ModifierKind::Hidden => {
                    self.im.add("androidx.compose.ui.draw.alpha");
                    let c = self.ev(arg("_"));
                    parts.push(format!("alpha(if ({c}) 0f else 1f)"));
                }
                ModifierKind::OnTap => {
                    self.im.add("androidx.compose.foundation.clickable");
                    let a = self.action(arg("_"));
                    parts.push(format!("clickable {{ {a} }}"));
                }
                ModifierKind::AccessibilityLabel => {
                    self.im.add("androidx.compose.ui.semantics.semantics");
                    self.im
                        .add("androidx.compose.ui.semantics.contentDescription");
                    // 文案可能来自stringResource，需要在组合作用域中先求值
                    let value = self.ev(arg("_"));
                    let name = format!("label{}", self.temp);
                    self.temp += 1;
                    w.line(format!("val {name} = {value}"));
                    parts.push(format!("semantics {{ contentDescription = {name} }}"));
                }
                ModifierKind::Android(a) => {
                    let (name, call) = match a {
                        AndroidModifier::ImePadding => (
                            "androidx.compose.foundation.layout.imePadding",
                            "imePadding()".to_string(),
                        ),
                        AndroidModifier::SystemBarsPadding => (
                            "androidx.compose.foundation.layout.systemBarsPadding",
                            "systemBarsPadding()".to_string(),
                        ),
                        AndroidModifier::TestTag => {
                            let tag = self.ev(arg("_"));
                            (
                                "androidx.compose.ui.platform.testTag",
                                format!("testTag({tag})"),
                            )
                        }
                    };
                    self.im.add(name);
                    parts.push(call);
                }
                ModifierKind::SwiftUi
                | ModifierKind::Font
                | ModifierKind::Foreground
                | ModifierKind::Disabled => {}
            }
            // 只在调试或发布构建中生效的修饰符
            if let Some(native) = &m.cond
                && parts.len() > before
            {
                let added: Vec<String> = parts.drain(before..).collect();
                let cond = self.native_cond(native);
                parts.push(format!(
                    "then(if ({cond}) Modifier.{} else Modifier)",
                    added.join(".")
                ));
            }
        }
        if parts.is_empty() && base == "Modifier" {
            return None;
        }
        self.im.add("androidx.compose.ui.Modifier");
        let mut out = base.to_string();
        for p in parts {
            out.push('.');
            out.push_str(&p);
        }
        Some(out)
    }

    fn color(&mut self, token: &str) -> String {
        let theme = |im: &mut Imports, field: &str| {
            im.add("androidx.compose.material3.MaterialTheme");
            format!("MaterialTheme.colorScheme.{field}")
        };
        match token {
            "primary" => theme(self.im, "onSurface"),
            "secondary" => theme(self.im, "onSurfaceVariant"),
            "accent" => theme(self.im, "primary"),
            other => {
                self.im.add("androidx.compose.ui.graphics.Color");
                match other {
                    "orange" => "Color(0xFFFF9800)".into(),
                    "purple" => "Color(0xFF9C27B0)".into(),
                    "clear" => "Color.Transparent".into(),
                    c => {
                        let mut chars = c.chars();
                        let name: String = chars
                            .next()
                            .map(|f| f.to_uppercase().chain(chars).collect())
                            .unwrap_or_default();
                        format!("Color.{name}")
                    }
                }
            }
        }
    }

    fn find(mods: &[Modifier], kind: ModifierKind) -> Option<&Modifier> {
        mods.iter().rev().find(|m| m.kind == kind)
    }

    fn enabled(&mut self, mods: &[Modifier]) -> Option<String> {
        let m = Self::find(mods, ModifierKind::Disabled)?;
        Some(match m.arg("_").and_then(Value::as_expr) {
            // disabled(!x)直接写成enabled = x
            Some(Expr {
                kind: ExprKind::Unary(UnaryOp::Not, inner),
                ..
            }) => kotlin::expr(self.cx, inner, self.im),
            Some(x) => format!("!{}", kotlin::wrap(self.cx, x, 8, self.im)),
            None => "true".into(),
        })
    }

    fn text_call(&mut self, text: String, mods: &[Modifier]) -> Call {
        self.im.add("androidx.compose.material3.Text");
        let mut call = Call::new("Text");
        call.arg("text", text);
        if let Some(m) = Self::find(mods, ModifierKind::Font) {
            self.im.add("androidx.compose.material3.MaterialTheme");
            let style = match m.arg("_").and_then(Value::as_token).unwrap_or("body") {
                "largeTitle" => "displaySmall",
                "title" => "headlineMedium",
                "title2" => "headlineSmall",
                "title3" => "titleLarge",
                "headline" => "titleMedium",
                "subheadline" => "titleSmall",
                "callout" => "bodyMedium",
                "footnote" => "bodySmall",
                "caption" => "labelSmall",
                _ => "bodyLarge",
            };
            call.arg("style", format!("MaterialTheme.typography.{style}"));
        }
        if let Some(m) = Self::find(mods, ModifierKind::Foreground) {
            let c = self.color(m.arg("_").and_then(Value::as_token).unwrap_or("primary"));
            call.arg("color", c);
        }
        call
    }

    fn scope_modifier(&mut self, parent: Parent, weight: bool) -> Option<String> {
        if weight && parent != Parent::Other {
            Some("weight(1f)".into())
        } else {
            None
        }
    }

    pub fn element(
        &mut self,
        w: &mut CodeWriter,
        el: &Element,
        parent: Parent,
        base: Option<&str>,
    ) {
        let base = base.unwrap_or("Modifier");
        let b = match &el.kind {
            ElementKind::Builtin(b) => b,
            ElementKind::Android(AndroidElement::FloatingActionButton) => {
                self.im
                    .add("androidx.compose.material3.FloatingActionButton");
                let a = self.action(el.arg("action"));
                let mut call = Call::new("FloatingActionButton");
                call.arg("onClick", format!("{{ {a} }}"));
                if let Some(m) = self.modifier_chain(w, &el.modifiers, None, base) {
                    call.arg("modifier", m);
                }
                call.render(w, Some("{"));
                w.indent();
                let text = self.ev(el.arg("_"));
                self.text_call(text, &[]).render(w, None);
                w.dedent();
                w.line("}");
                return;
            }
            ElementKind::SwiftUi(_) => return,
            ElementKind::Component(name) => {
                let mut call = Call::new(name);
                for (n, v) in &el.args {
                    match v {
                        Value::Expr(x) => {
                            let value = self.e(x);
                            call.arg(&ident(n), value);
                        }
                        Value::Action(_) => {
                            let a = self.action(Some(v));
                            call.arg(&ident(n), format!("{{ {a} }}"));
                        }
                        Value::Binding(_) => {
                            let (value, change) = self.binding(Some(v), "");
                            call.arg(&ident(n), value);
                            call.arg(&format!("on{}Change", pascal_case(n)), change);
                        }
                        Value::KeyPath(_) | Value::Token(_) => {}
                    }
                }
                if let Some(m) = self.modifier_chain(w, &el.modifiers, None, base) {
                    call.arg("modifier", m);
                }
                call.render(w, None);
                return;
            }
        };
        let mods = &el.modifiers;
        let arg = |n: &str| el.arg(n);
        match b {
            Builtin::Column | Builtin::Row | Builtin::Stack | Builtin::ScrollView => {
                let (name, child_parent) = match b {
                    Builtin::Column | Builtin::ScrollView => ("Column", Parent::Column),
                    Builtin::Row => ("Row", Parent::Row),
                    _ => ("Box", Parent::Other),
                };
                self.im
                    .add(&format!("androidx.compose.foundation.layout.{name}"));
                let mut call = Call::new(name);
                let scroll = if *b == Builtin::ScrollView {
                    self.im.add("androidx.compose.foundation.verticalScroll");
                    self.im
                        .add("androidx.compose.foundation.rememberScrollState");
                    Some("verticalScroll(rememberScrollState())")
                } else {
                    None
                };
                if let Some(m) = self.modifier_chain(w, mods, None, base) {
                    let m = match scroll {
                        Some(s) => format!("{m}.{s}"),
                        None => m,
                    };
                    call.arg("modifier", m);
                } else if let Some(s) = scroll {
                    self.im.add("androidx.compose.ui.Modifier");
                    call.arg("modifier", format!("Modifier.{s}"));
                }
                if let Some(x) = arg("spacing").and_then(Value::as_expr) {
                    self.im
                        .add("androidx.compose.foundation.layout.Arrangement");
                    let d = dp(self.cx, x, self.im);
                    let key = if name == "Row" {
                        "horizontalArrangement"
                    } else {
                        "verticalArrangement"
                    };
                    call.arg(key, format!("Arrangement.spacedBy({d})"));
                }
                // 对齐方式与SwiftUI默认值保持一致
                self.im.add("androidx.compose.ui.Alignment");
                let token = arg("alignment")
                    .and_then(Value::as_token)
                    .unwrap_or("center");
                match b {
                    Builtin::Row => {
                        let a = match token {
                            "top" => "Top",
                            "bottom" => "Bottom",
                            _ => "CenterVertically",
                        };
                        call.arg("verticalAlignment", format!("Alignment.{a}"));
                    }
                    Builtin::Stack => {
                        let a = match token {
                            "topLeading" => "TopStart",
                            "top" => "TopCenter",
                            "topTrailing" => "TopEnd",
                            "leading" => "CenterStart",
                            "trailing" => "CenterEnd",
                            "bottomLeading" => "BottomStart",
                            "bottom" => "BottomCenter",
                            "bottomTrailing" => "BottomEnd",
                            _ => "Center",
                        };
                        call.arg("contentAlignment", format!("Alignment.{a}"));
                    }
                    _ => {
                        let a = match token {
                            "leading" => "Start",
                            "trailing" => "End",
                            _ => "CenterHorizontally",
                        };
                        call.arg("horizontalAlignment", format!("Alignment.{a}"));
                    }
                }
                call.render(w, Some("{"));
                w.indent();
                self.nodes(w, &el.children, child_parent);
                w.dedent();
                w.line("}");
            }
            Builtin::List => {
                self.im.add("androidx.compose.foundation.lazy.LazyColumn");
                self.im.add("androidx.compose.foundation.lazy.items");
                let mut call = Call::new("LazyColumn");
                if let Some(m) = self.modifier_chain(w, mods, None, base) {
                    call.arg("modifier", m);
                }
                call.render(w, Some("{"));
                w.indent();
                let list = self.ev(arg("_"));
                let key = ident(arg("key").and_then(Value::as_key_path).unwrap_or("id"));
                let item = el
                    .item
                    .as_ref()
                    .map(|(n, _)| ident(n))
                    .unwrap_or_else(|| "item".into());
                w.line(format!(
                    "items(items = {list}, key = {{ it.{key} }}) {{ {item} ->"
                ));
                w.indent();
                // LazyItemScope中没有weight
                self.nodes(w, &el.children, Parent::Other);
                w.dedent();
                w.line("}");
                w.dedent();
                w.line("}");
            }
            Builtin::ForEach => {
                self.im.add("androidx.compose.runtime.key");
                let list = match arg("_").and_then(Value::as_expr) {
                    Some(x) => kotlin::atom(self.cx, x, self.im),
                    None => "emptyList<Nothing>()".into(),
                };
                let key = ident(arg("key").and_then(Value::as_key_path).unwrap_or("id"));
                let item = el
                    .item
                    .as_ref()
                    .map(|(n, _)| ident(n))
                    .unwrap_or_else(|| "item".into());
                w.line(format!("{list}.forEach {{ {item} ->"));
                w.indent();
                w.line(format!("key({item}.{key}) {{"));
                w.indent();
                self.nodes(w, &el.children, parent);
                w.dedent();
                w.line("}");
                w.dedent();
                w.line("}");
            }
            Builtin::Text => {
                let text = self.ev(arg("_"));
                let mut call = self.text_call(text, mods);
                if let Some(m) = self.modifier_chain(w, mods, None, base) {
                    call.arg("modifier", m);
                }
                call.render(w, None);
            }
            Builtin::Button => {
                self.im.add("androidx.compose.material3.Button");
                let a = self.action(arg("action"));
                let mut call = Call::new("Button");
                call.arg("onClick", format!("{{ {a} }}"));
                if let Some(en) = self.enabled(mods) {
                    call.arg("enabled", en);
                }
                if let Some(m) = self.modifier_chain(w, mods, None, base) {
                    call.arg("modifier", m);
                }
                call.render(w, Some("{"));
                w.indent();
                let text = self.ev(arg("_"));
                self.text_call(text, &[]).render(w, None);
                w.dedent();
                w.line("}");
            }
            Builtin::TextField | Builtin::SecureField => {
                self.im.add("androidx.compose.material3.OutlinedTextField");
                let (value, change) = self.binding(arg("text"), "");
                let label = self.ev(arg("_"));
                let mut call = Call::new("OutlinedTextField");
                call.arg("value", value).arg("onValueChange", change);
                self.im.add("androidx.compose.material3.Text");
                call.arg("label", format!("{{ Text(text = {label}) }}"));
                if *b == Builtin::SecureField {
                    self.im
                        .add("androidx.compose.ui.text.input.PasswordVisualTransformation");
                    call.arg("visualTransformation", "PasswordVisualTransformation()");
                }
                if let Some(en) = self.enabled(mods) {
                    call.arg("enabled", en);
                }
                if let Some(m) = self.modifier_chain(w, mods, None, base) {
                    call.arg("modifier", m);
                }
                call.render(w, None);
            }
            Builtin::Toggle => {
                self.im.add("androidx.compose.foundation.layout.Row");
                self.im.add("androidx.compose.material3.Switch");
                self.im.add("androidx.compose.ui.Alignment");
                let mut row = Call::new("Row");
                if let Some(m) = self.modifier_chain(w, mods, None, base) {
                    row.arg("modifier", m);
                }
                row.arg("verticalAlignment", "Alignment.CenterVertically");
                row.render(w, Some("{"));
                w.indent();
                let label = self.ev(arg("_"));
                let mut text = self.text_call(label, &[]);
                self.im.add("androidx.compose.ui.Modifier");
                text.arg("modifier", "Modifier.weight(1f)");
                text.render(w, None);
                let (value, change) = self.binding(arg("isOn"), "");
                let mut sw = Call::new("Switch");
                sw.arg("checked", value).arg("onCheckedChange", change);
                if let Some(en) = self.enabled(mods) {
                    sw.arg("enabled", en);
                }
                sw.render(w, None);
                w.dedent();
                w.line("}");
            }
            Builtin::Slider => {
                self.im.add("androidx.compose.material3.Slider");
                let (value, change) = self.binding(arg("value"), ".toDouble()");
                let value = format!("{value}.toFloat()");
                let min = match arg("min").and_then(Value::as_expr) {
                    Some(x) => float(self.cx, x, self.im),
                    None => "0f".into(),
                };
                let max = match arg("max").and_then(Value::as_expr) {
                    Some(x) => float(self.cx, x, self.im),
                    None => "1f".into(),
                };
                let mut call = Call::new("Slider");
                call.arg("value", value)
                    .arg("onValueChange", change)
                    .arg("valueRange", format!("{min}..{max}"));
                if let Some(en) = self.enabled(mods) {
                    call.arg("enabled", en);
                }
                if let Some(m) = self.modifier_chain(w, mods, None, base) {
                    call.arg("modifier", m);
                }
                call.render(w, None);
            }
            Builtin::Image => {
                self.im.add("androidx.compose.foundation.Image");
                self.im.add("androidx.compose.ui.res.painterResource");
                let id = match arg("_").and_then(Value::as_expr) {
                    Some(Expr {
                        kind: ExprKind::String(s),
                        ..
                    }) => {
                        self.im.add_r();
                        format!("R.drawable.{s}")
                    }
                    other => {
                        self.im.add("androidx.compose.ui.platform.LocalContext");
                        let name = other
                            .map(|x| kotlin::expr(self.cx, x, self.im))
                            .unwrap_or_default();
                        format!(
                            "LocalContext.current.let {{ it.resources.getIdentifier({name}, \"drawable\", it.packageName) }}"
                        )
                    }
                };
                let mut call = Call::new("Image");
                call.arg("painter", format!("painterResource(id = {id})"))
                    .arg("contentDescription", "null");
                if let Some(m) = self.modifier_chain(w, mods, None, base) {
                    call.arg("modifier", m);
                }
                call.render(w, None);
            }
            Builtin::Spacer => {
                self.im.add("androidx.compose.foundation.layout.Spacer");
                let scope = self.scope_modifier(parent, true);
                let mut call = Call::new("Spacer");
                match self.modifier_chain(w, mods, scope.as_deref(), base) {
                    Some(m) => call.arg("modifier", m),
                    None => {
                        self.im.add("androidx.compose.ui.Modifier");
                        call.arg("modifier", "Modifier")
                    }
                };
                call.render(w, None);
            }
            Builtin::Divider => {
                let name = if parent == Parent::Row {
                    "VerticalDivider"
                } else {
                    "HorizontalDivider"
                };
                self.im.add(&format!("androidx.compose.material3.{name}"));
                let mut call = Call::new(name);
                if let Some(m) = self.modifier_chain(w, mods, None, base) {
                    call.arg("modifier", m);
                }
                call.render(w, None);
            }
            Builtin::Progress => {
                let mut call = match arg("value").and_then(Value::as_expr) {
                    Some(x) => {
                        self.im
                            .add("androidx.compose.material3.LinearProgressIndicator");
                        let v = float(self.cx, x, self.im);
                        let mut c = Call::new("LinearProgressIndicator");
                        c.arg("progress", format!("{{ {v} }}"));
                        c
                    }
                    None => {
                        self.im
                            .add("androidx.compose.material3.CircularProgressIndicator");
                        Call::new("CircularProgressIndicator")
                    }
                };
                if let Some(m) = self.modifier_chain(w, mods, None, base) {
                    call.arg("modifier", m);
                }
                call.render(w, None);
            }
        }
    }
}
