// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 选择器、菜单、步进器等控件到Material3的映射

use veyra_codegen::{CodeWriter, pascal_case};
use veyra_ir::{BindingRef, Builtin, Element, ElementKind, Expr, ModifierKind, Value, ViewNode};

use super::{Call, Parent, ViewEmitter};
use crate::kotlin::{self, float, ident};
use crate::symbols;

impl ViewEmitter<'_> {
    fn add_all(&mut self, imports: &[&str]) {
        for i in imports {
            self.im.add(i);
        }
    }

    fn fresh(&mut self, prefix: &str) -> String {
        let name = format!("{prefix}{}", self.temp);
        self.temp += 1;
        name
    }

    /// 本地记住的布尔状态，用于菜单与对话框的展开
    fn local_flag(&mut self, w: &mut CodeWriter, prefix: &str) -> String {
        self.add_all(&[
            "androidx.compose.runtime.getValue",
            "androidx.compose.runtime.setValue",
            "androidx.compose.runtime.mutableStateOf",
            "androidx.compose.runtime.remember",
        ]);
        let name = self.fresh(prefix);
        w.line(format!(
            "var {name} by remember {{ mutableStateOf(false) }}"
        ));
        name
    }

    /// 通过绑定写入新值的语句
    pub(super) fn write_binding(&mut self, v: Option<&Value>, new: &str) -> String {
        match v.and_then(Value::as_binding) {
            Some(BindingRef::State { setter, .. }) => {
                format!("{}.{}({new})", self.actions, ident(setter))
            }
            Some(BindingRef::Param { name, .. }) => {
                format!("on{}Change({new})", pascal_case(name))
            }
            None => String::new(),
        }
    }

    fn label_text(&mut self, w: &mut CodeWriter, label: String, weight: bool) {
        let mut text = self.text_call(label, &[]);
        if weight {
            self.im.add("androidx.compose.ui.Modifier");
            text.arg("modifier", "Modifier.weight(1f)");
        }
        text.render(w, None);
    }

    /// 带标签的一行控件，标签占据剩余宽度
    fn labeled_row(&mut self, w: &mut CodeWriter, el: &Element, base: &str) {
        self.add_all(&[
            "androidx.compose.foundation.layout.Row",
            "androidx.compose.ui.Alignment",
        ]);
        let mut row = Call::new("Row");
        if let Some(m) = self.modifier_chain(w, &el.modifiers, None, base) {
            row.arg("modifier", m);
        }
        row.arg("verticalAlignment", "Alignment.CenterVertically");
        row.render(w, Some("{"));
        w.indent();
        let label = self.ev(el.arg("_"));
        self.label_text(w, label, true);
    }

    fn text_button(
        &mut self,
        w: &mut CodeWriter,
        on_click: &str,
        enabled: Option<&str>,
        text: &str,
    ) {
        self.im.add("androidx.compose.material3.TextButton");
        let mut b = Call::new("TextButton");
        b.arg("onClick", format!("{{ {on_click} }}"));
        if let Some(en) = enabled {
            b.arg("enabled", en);
        }
        b.render(w, Some("{"));
        w.indent();
        self.text_call(text.to_string(), &[]).render(w, None);
        w.dedent();
        w.line("}");
    }

    /// 菜单中的按钮
    pub(super) fn menu_item(&mut self, w: &mut CodeWriter, el: &Element, flag: &str, base: &str) {
        self.add_all(&[
            "androidx.compose.material3.DropdownMenuItem",
            "androidx.compose.material3.Text",
        ]);
        let a = self.action(el.arg("action"));
        let label = self.ev(el.arg("_"));
        let mut call = Call::new("DropdownMenuItem");
        call.arg("text", format!("{{ Text(text = {label}) }}"))
            .arg("onClick", format!("{{ {flag} = false; {a} }}"));
        if el.arg("role").and_then(Value::as_token) == Some("destructive") {
            self.add_all(&[
                "androidx.compose.material3.MenuDefaults",
                "androidx.compose.material3.MaterialTheme",
            ]);
            call.arg(
                "colors",
                "MenuDefaults.itemColors(textColor = MaterialTheme.colorScheme.error)",
            );
        }
        if let Some(en) = self.enabled(&el.modifiers) {
            call.arg("enabled", en);
        }
        if let Some(m) = self.modifier_chain(w, &el.modifiers, None, base) {
            call.arg("modifier", m);
        }
        call.render(w, None);
    }

    pub(super) fn menu(&mut self, w: &mut CodeWriter, el: &Element, base: &str) {
        self.add_all(&[
            "androidx.compose.foundation.layout.Box",
            "androidx.compose.material3.DropdownMenu",
        ]);
        let flag = self.local_flag(w, "menu");
        let mut b = Call::new("Box");
        if let Some(m) = self.modifier_chain(w, &el.modifiers, None, base) {
            b.arg("modifier", m);
        }
        b.render(w, Some("{"));
        w.indent();
        let label = self.ev(el.arg("_"));
        let enabled = self.enabled(&el.modifiers);
        self.text_button(w, &format!("{flag} = true"), enabled.as_deref(), &label);
        w.line(format!(
            "DropdownMenu(expanded = {flag}, onDismissRequest = {{ {flag} = false }}) {{"
        ));
        w.indent();
        let outer = self.menu.replace(flag);
        self.nodes(w, &el.children, Parent::Column);
        self.menu = outer;
        w.dedent();
        w.line("}");
        w.dedent();
        w.line("}");
    }

    /// Picker的选项列表，元素为`值 to 文案`
    fn picker_options(&mut self, nodes: &[ViewNode]) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut fixed: Vec<String> = Vec::new();
        for node in nodes {
            let ViewNode::Element(el) = node else {
                continue;
            };
            match el.kind {
                ElementKind::Builtin(Builtin::Option) => fixed.push(self.option_pair(el)),
                ElementKind::Builtin(Builtin::ForEach) => {
                    if !fixed.is_empty() {
                        parts.push(format!("listOf({})", fixed.join(", ")));
                        fixed.clear();
                    }
                    let list = match el.arg("_").and_then(Value::as_expr) {
                        Some(x) => kotlin::atom(self.cx, x, self.im),
                        None => "emptyList<Nothing>()".into(),
                    };
                    let item = el
                        .item
                        .as_ref()
                        .map(|(n, _)| ident(n))
                        .unwrap_or_else(|| "item".into());
                    let pairs: Vec<String> = el
                        .children
                        .iter()
                        .filter_map(|c| match c {
                            ViewNode::Element(o) => Some(o),
                            _ => None,
                        })
                        .map(|o| self.option_pair(o))
                        .collect();
                    parts.push(match pairs.as_slice() {
                        [one] => format!("{list}.map {{ {item} -> {one} }}"),
                        _ => format!(
                            "{list}.flatMap {{ {item} -> listOf({}) }}",
                            pairs.join(", ")
                        ),
                    });
                }
                _ => {}
            }
        }
        if !fixed.is_empty() {
            parts.push(format!("listOf({})", fixed.join(", ")));
        }
        if parts.is_empty() {
            "emptyList<Pair<Nothing, String>>()".into()
        } else {
            parts.join(" + ")
        }
    }

    fn option_pair(&mut self, el: &Element) -> String {
        let value = self.ev(el.arg("value"));
        let label = self.ev(el.arg("_"));
        format!("{value} to {label}")
    }

    pub(super) fn picker(&mut self, w: &mut CodeWriter, el: &Element, base: &str) {
        let options = self.fresh("options");
        let list = self.picker_options(&el.children);
        let ty = match el.arg("selection").and_then(Value::as_binding) {
            Some(BindingRef::State { ty, .. } | BindingRef::Param { ty, .. }) => {
                kotlin::ty(ty, self.im)
            }
            None => "Any?".into(),
        };
        w.line(format!("val {options}: List<Pair<{ty}, String>> = {list}"));
        let (value, _) = self.binding(el.arg("selection"), "");
        let select = self.write_binding(el.arg("selection"), "option");
        let enabled = self.enabled(&el.modifiers);
        match el.arg("style").and_then(Value::as_token) {
            Some("segmented") => {
                self.add_all(&[
                    "androidx.compose.material3.SingleChoiceSegmentedButtonRow",
                    "androidx.compose.material3.SegmentedButton",
                    "androidx.compose.material3.SegmentedButtonDefaults",
                    "androidx.compose.material3.Text",
                ]);
                self.im.experimental = true;
                let mut row = Call::new("SingleChoiceSegmentedButtonRow");
                if let Some(m) = self.modifier_chain(w, &el.modifiers, None, base) {
                    row.arg("modifier", m);
                }
                row.render(w, Some("{"));
                w.indent();
                w.line(format!(
                    "{options}.forEachIndexed {{ index, (option, label) ->"
                ));
                w.indent();
                let mut b = Call::new("SegmentedButton");
                b.arg("selected", format!("option == {value}"))
                    .arg("onClick", format!("{{ {select} }}"))
                    .arg(
                        "shape",
                        format!(
                            "SegmentedButtonDefaults.itemShape(index = index, count = {options}.size)"
                        ),
                    );
                if let Some(en) = &enabled {
                    b.arg("enabled", en.clone());
                }
                b.render(w, Some("{ Text(text = label) }"));
                w.dedent();
                w.line("}");
                w.dedent();
                w.line("}");
            }
            Some("inline" | "wheel" | "palette") => {
                self.add_all(&[
                    "androidx.compose.foundation.layout.Column",
                    "androidx.compose.foundation.layout.Row",
                    "androidx.compose.foundation.layout.fillMaxWidth",
                    "androidx.compose.foundation.selection.selectable",
                    "androidx.compose.material3.RadioButton",
                    "androidx.compose.material3.MaterialTheme",
                    "androidx.compose.ui.Alignment",
                    "androidx.compose.ui.Modifier",
                ]);
                let mut col = Call::new("Column");
                if let Some(m) = self.modifier_chain(w, &el.modifiers, None, base) {
                    col.arg("modifier", m);
                }
                col.render(w, Some("{"));
                w.indent();
                let label = self.ev(el.arg("_"));
                let mut title = self.text_call(label, &[]);
                title.arg("style", "MaterialTheme.typography.labelLarge");
                title.render(w, None);
                w.line(format!("{options}.forEach {{ (option, label) ->"));
                w.indent();
                let en = enabled
                    .as_ref()
                    .map(|e| format!(", enabled = {e}"))
                    .unwrap_or_default();
                w.line(format!(
                    "Row(modifier = Modifier.fillMaxWidth().selectable(selected = option == {value}{en}, onClick = {{ {select} }}), verticalAlignment = Alignment.CenterVertically) {{"
                ));
                w.indent();
                w.line(format!(
                    "RadioButton(selected = option == {value}, onClick = null{en})"
                ));
                self.text_call("label".into(), &[]).render(w, None);
                w.dedent();
                w.line("}");
                w.dedent();
                w.line("}");
                w.dedent();
                w.line("}");
            }
            _ => {
                self.add_all(&[
                    "androidx.compose.foundation.layout.Box",
                    "androidx.compose.material3.DropdownMenu",
                    "androidx.compose.material3.DropdownMenuItem",
                    "androidx.compose.material3.Text",
                ]);
                let flag = self.local_flag(w, "expanded");
                self.labeled_row(w, el, base);
                w.line("Box {");
                w.indent();
                let current =
                    format!("{options}.firstOrNull {{ it.first == {value} }}?.second ?: \"\"");
                self.text_button(w, &format!("{flag} = true"), enabled.as_deref(), &current);
                w.line(format!(
                    "DropdownMenu(expanded = {flag}, onDismissRequest = {{ {flag} = false }}) {{"
                ));
                w.indent();
                w.line(format!("{options}.forEach {{ (option, label) ->"));
                w.indent();
                w.line(format!(
                    "DropdownMenuItem(text = {{ Text(text = label) }}, onClick = {{ {flag} = false; {select} }})"
                ));
                w.dedent();
                w.line("}");
                w.dedent();
                w.line("}");
                w.dedent();
                w.line("}");
                w.dedent();
                w.line("}");
            }
        }
    }

    /// 日期选择：按钮显示当前值，点按后弹出Material3的日期或时间对话框
    pub(super) fn date_picker(&mut self, w: &mut CodeWriter, el: &Element, base: &str) {
        self.im.experimental = true;
        let (value, _) = self.binding(el.arg("selection"), "");
        let (date, time) = match el.arg("components").and_then(Value::as_token) {
            Some("date") => (true, false),
            Some("time") => (false, true),
            _ => (true, true),
        };
        let enabled = self.enabled(&el.modifiers);
        let date_flag = date.then(|| self.local_flag(w, "pickDate"));
        let time_flag = time.then(|| self.local_flag(w, "pickTime"));
        self.labeled_row(w, el, base);
        if let Some(f) = &date_flag {
            let text = format!("veyraDateLabel({value}, date = true, time = false)");
            self.text_button(w, &format!("{f} = true"), enabled.as_deref(), &text);
        }
        if let Some(f) = &time_flag {
            let text = format!("veyraDateLabel({value}, date = false, time = true)");
            self.text_button(w, &format!("{f} = true"), enabled.as_deref(), &text);
        }
        w.dedent();
        w.line("}");
        self.add_all(&[
            "androidx.compose.material3.TextButton",
            "androidx.compose.material3.Text",
            "androidx.compose.ui.res.stringResource",
        ]);
        let buttons = |w: &mut CodeWriter, flag: &str, confirm: &str| {
            w.line(format!(
                "confirmButton = {{ TextButton(onClick = {{ {confirm}; {flag} = false }}) {{ Text(text = stringResource(android.R.string.ok)) }} }},"
            ));
            w.line(format!(
                "dismissButton = {{ TextButton(onClick = {{ {flag} = false }}) {{ Text(text = stringResource(android.R.string.cancel)) }} }},"
            ));
        };
        if let Some(f) = &date_flag {
            self.add_all(&[
                "androidx.compose.material3.DatePicker",
                "androidx.compose.material3.DatePickerDialog",
                "androidx.compose.material3.rememberDatePickerState",
            ]);
            let set =
                self.write_binding(el.arg("selection"), &format!("veyraWithDate({value}, it)"));
            let st = self.fresh("dateState");
            w.line(format!("if ({f}) {{"));
            w.indent();
            w.line(format!(
                "val {st} = rememberDatePickerState(initialSelectedDateMillis = veyraDateMillis({value}))"
            ));
            w.line("DatePickerDialog(");
            w.indent();
            w.line(format!("onDismissRequest = {{ {f} = false }},"));
            buttons(w, f, &format!("{st}.selectedDateMillis?.let {{ {set} }}"));
            w.dedent();
            w.line(") {");
            w.indent();
            w.line(format!("DatePicker(state = {st})"));
            w.dedent();
            w.line("}");
            w.dedent();
            w.line("}");
        }
        if let Some(f) = &time_flag {
            self.add_all(&[
                "androidx.compose.material3.AlertDialog",
                "androidx.compose.material3.TimePicker",
                "androidx.compose.material3.rememberTimePickerState",
            ]);
            let st = self.fresh("timeState");
            let set = self.write_binding(
                el.arg("selection"),
                &format!("veyraWithTime({value}, {st}.hour, {st}.minute)"),
            );
            w.line(format!("if ({f}) {{"));
            w.indent();
            w.line(format!(
                "val {st} = rememberTimePickerState(initialHour = veyraHour({value}), initialMinute = veyraMinute({value}))"
            ));
            w.line("AlertDialog(");
            w.indent();
            w.line(format!("onDismissRequest = {{ {f} = false }},"));
            buttons(w, f, &set);
            w.line(format!("text = {{ TimePicker(state = {st}) }},"));
            w.dedent();
            w.line(")");
            w.dedent();
            w.line("}");
        }
    }

    pub(super) fn stepper(&mut self, w: &mut CodeWriter, el: &Element, base: &str) {
        let double = matches!(
            el.arg("value").and_then(Value::as_binding),
            Some(BindingRef::State { ty, .. } | BindingRef::Param { ty, .. }) if *ty == veyra_ir::Type::Double
        );
        let (value, _) = self.binding(el.arg("value"), "");
        let bound = |this: &mut Self, n: &str| {
            el.arg(n)
                .and_then(Value::as_expr)
                .map(|x| kotlin::expr(this.cx, x, this.im))
        };
        let step = bound(self, "step").unwrap_or_else(|| if double { "1.0" } else { "1" }.into());
        let (min, max) = (bound(self, "min"), bound(self, "max"));
        let enabled = self.enabled(&el.modifiers);
        let guard = |limit: Option<String>, op: &str| -> Option<String> {
            let mut parts = Vec::new();
            if let Some(en) = &enabled {
                parts.push(en.clone());
            }
            if let Some(l) = limit {
                parts.push(format!("{value} {op} {l}"));
            }
            (!parts.is_empty()).then(|| parts.join(" && "))
        };
        let down = match &min {
            Some(m) => format!("({value} - {step}).coerceAtLeast({m})"),
            None => format!("{value} - {step}"),
        };
        let up = match &max {
            Some(m) => format!("({value} + {step}).coerceAtMost({m})"),
            None => format!("{value} + {step}"),
        };
        let down = self.write_binding(el.arg("value"), &down);
        let up = self.write_binding(el.arg("value"), &up);
        let (down_en, up_en) = (guard(min.clone(), ">"), guard(max.clone(), "<"));
        self.labeled_row(w, el, base);
        self.text_button(w, &down, down_en.as_deref(), "\"−\"");
        self.text_button(w, &up, up_en.as_deref(), "\"+\"");
        w.dedent();
        w.line("}");
    }

    pub(super) fn link(&mut self, w: &mut CodeWriter, el: &Element, base: &str) {
        self.add_all(&[
            "androidx.compose.material3.TextButton",
            "androidx.compose.ui.platform.LocalUriHandler",
        ]);
        let handler = self.fresh("uriHandler");
        w.line(format!("val {handler} = LocalUriHandler.current"));
        let url = self.ev(el.arg("url"));
        let mut b = Call::new("TextButton");
        b.arg("onClick", format!("{{ {handler}.openUri({url}) }}"));
        if let Some(en) = self.enabled(&el.modifiers) {
            b.arg("enabled", en);
        }
        if let Some(m) = self.modifier_chain(w, &el.modifiers, None, base) {
            b.arg("modifier", m);
        }
        b.render(w, Some("{"));
        w.indent();
        let label = self.ev(el.arg("_"));
        self.text_call(label, &[]).render(w, None);
        w.dedent();
        w.line("}");
    }

    pub(super) fn text_editor(&mut self, w: &mut CodeWriter, el: &Element, base: &str) {
        self.im.add("androidx.compose.material3.OutlinedTextField");
        let (value, change) = self.binding(el.arg("text"), "");
        let mut call = Call::new("OutlinedTextField");
        call.arg("value", value)
            .arg("onValueChange", change)
            .arg("minLines", "4");
        if let Some(en) = self.enabled(&el.modifiers) {
            call.arg("enabled", en);
        }
        if let Some(m) = self.modifier_chain(w, &el.modifiers, None, base) {
            call.arg("modifier", m);
        }
        call.render(w, None);
    }

    /// SF Symbols按对照表映射为Material图标，没有对应图标时只显示文字
    pub(super) fn label(&mut self, w: &mut CodeWriter, el: &Element, base: &str) {
        self.add_all(&[
            "androidx.compose.foundation.layout.Row",
            "androidx.compose.foundation.layout.Arrangement",
            "androidx.compose.ui.Alignment",
            "androidx.compose.ui.unit.dp",
        ]);
        let mut row = Call::new("Row");
        if let Some(m) = self.modifier_chain(w, &el.modifiers, None, base) {
            row.arg("modifier", m);
        }
        row.arg("verticalAlignment", "Alignment.CenterVertically")
            .arg("horizontalArrangement", "Arrangement.spacedBy(8.dp)");
        row.render(w, Some("{"));
        w.indent();
        match el.arg("icon").and_then(Value::as_expr) {
            Some(Expr {
                kind: veyra_ir::ExprKind::String(s),
                ..
            }) => {
                if let Some(icon) = symbols::material(s) {
                    self.add_all(&[
                        "androidx.compose.material3.Icon",
                        "androidx.compose.material.icons.Icons",
                        &format!("androidx.compose.material.icons.filled.{icon}"),
                    ]);
                    w.line(format!(
                        "Icon(imageVector = Icons.Default.{icon}, contentDescription = null)"
                    ));
                }
            }
            Some(x) => {
                self.im.add("androidx.compose.material3.Icon");
                let name = kotlin::expr(self.cx, x, self.im);
                w.line(format!(
                    "veyraSymbol({name})?.let {{ Icon(imageVector = it, contentDescription = null) }}"
                ));
            }
            None => {}
        }
        let title = self.ev(el.arg("_"));
        self.text_call(title, &[]).render(w, None);
        w.dedent();
        w.line("}");
    }

    pub(super) fn gauge(&mut self, w: &mut CodeWriter, el: &Element, base: &str) {
        self.add_all(&[
            "androidx.compose.foundation.layout.Column",
            "androidx.compose.foundation.layout.fillMaxWidth",
            "androidx.compose.material3.LinearProgressIndicator",
            "androidx.compose.ui.Modifier",
        ]);
        let mut col = Call::new("Column");
        if let Some(m) = self.modifier_chain(w, &el.modifiers, None, base) {
            col.arg("modifier", m);
        }
        col.render(w, Some("{"));
        w.indent();
        if let Some(l) = el.arg("label").and_then(Value::as_expr) {
            self.im.add("androidx.compose.material3.MaterialTheme");
            let t = self.e(l);
            let mut text = self.text_call(t, &[]);
            text.arg("style", "MaterialTheme.typography.labelMedium");
            text.render(w, None);
        }
        let progress = self.fraction(el);
        w.line(format!(
            "LinearProgressIndicator(progress = {{ {progress} }}, modifier = Modifier.fillMaxWidth())"
        ));
        w.dedent();
        w.line("}");
    }

    /// Gauge的取值换算为0到1之间的Float
    fn fraction(&mut self, el: &Element) -> String {
        let value = el.arg("value").and_then(Value::as_expr);
        let min = el.arg("min").and_then(Value::as_expr);
        let max = el.arg("max").and_then(Value::as_expr);
        let Some(value) = value else {
            return "0f".into();
        };
        if min.is_none() && max.is_none() {
            return float(self.cx, value, self.im);
        }
        let v = kotlin::wrap(self.cx, value, 5, self.im);
        let lo = min
            .map(|m| kotlin::wrap(self.cx, m, 6, self.im))
            .unwrap_or_else(|| "0.0".into());
        let hi = max
            .map(|m| kotlin::wrap(self.cx, m, 6, self.im))
            .unwrap_or_else(|| "1.0".into());
        format!("(({v} - {lo}) / ({hi} - {lo})).toFloat()")
    }

    pub(super) fn progress(&mut self, w: &mut CodeWriter, el: &Element, base: &str) {
        let value = el.arg("value").and_then(Value::as_expr);
        let circular = match el.arg("style").and_then(Value::as_token) {
            Some(s) => s == "circular",
            None => value.is_none(),
        };
        let name = if circular {
            "CircularProgressIndicator"
        } else {
            "LinearProgressIndicator"
        };
        self.im.add(&format!("androidx.compose.material3.{name}"));
        let mut call = Call::new(name);
        if let Some(x) = value {
            let p = match el.arg("total").and_then(Value::as_expr) {
                Some(t) => format!(
                    "({} / {}).toFloat()",
                    kotlin::wrap(self.cx, x, 6, self.im),
                    kotlin::wrap(self.cx, t, 7, self.im)
                ),
                None => float(self.cx, x, self.im),
            };
            call.arg("progress", format!("{{ {p} }}"));
        }
        let chain = self.modifier_chain(w, &el.modifiers, None, base);
        match el.arg("_").and_then(Value::as_expr) {
            Some(l) => {
                self.im.add("androidx.compose.foundation.layout.Column");
                let mut col = Call::new("Column");
                if let Some(m) = chain {
                    col.arg("modifier", m);
                }
                col.render(w, Some("{"));
                w.indent();
                let t = self.e(l);
                self.text_call(t, &[]).render(w, None);
                call.render(w, None);
                w.dedent();
                w.line("}");
            }
            None => {
                if let Some(m) = chain {
                    call.arg("modifier", m);
                }
                call.render(w, None);
            }
        }
    }

    /// 键盘类型、提交按钮与大小写等文本框参数
    pub(super) fn keyboard_args(&mut self, call: &mut Call, el: &Element) {
        let token = |kind: ModifierKind| {
            Self::find(&el.modifiers, kind)
                .and_then(|m| m.arg("_"))
                .and_then(Value::as_token)
        };
        let mut options = Vec::new();
        if let Some(k) = token(ModifierKind::Keyboard) {
            self.im.add("androidx.compose.ui.text.input.KeyboardType");
            let t = match k {
                "email" => "Email",
                "number" => "Number",
                "decimal" => "Decimal",
                "phone" => "Phone",
                "url" => "Uri",
                _ => "Text",
            };
            options.push(format!("keyboardType = KeyboardType.{t}"));
        }
        if let Some(c) = token(ModifierKind::Autocapitalization) {
            self.im
                .add("androidx.compose.ui.text.input.KeyboardCapitalization");
            let t = match c {
                "never" => "None",
                "words" => "Words",
                "characters" => "Characters",
                _ => "Sentences",
            };
            options.push(format!("capitalization = KeyboardCapitalization.{t}"));
        }
        if let Some(s) = token(ModifierKind::SubmitLabel) {
            self.im.add("androidx.compose.ui.text.input.ImeAction");
            let t = match s {
                "done" => "Done",
                "go" => "Go",
                "next" => "Next",
                "search" => "Search",
                "send" => "Send",
                _ => "Default",
            };
            options.push(format!("imeAction = ImeAction.{t}"));
        }
        if !options.is_empty() {
            self.im
                .add("androidx.compose.foundation.text.KeyboardOptions");
            call.arg(
                "keyboardOptions",
                format!("KeyboardOptions({})", options.join(", ")),
            );
        }
        if let Some(m) = Self::find(&el.modifiers, ModifierKind::OnSubmit) {
            self.im
                .add("androidx.compose.foundation.text.KeyboardActions");
            let a = self.action(m.arg("_"));
            call.arg(
                "keyboardActions",
                format!("KeyboardActions(onAny = {{ {a} }})"),
            );
        }
    }
}
