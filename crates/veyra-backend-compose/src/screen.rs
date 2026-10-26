// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 单个screen的Kotlin文件

use veyra_codegen::{CodeWriter, USER_OWNED_NOTICE};
use veyra_ir::{ScreenDef, ViewNode};

use crate::kotlin::{self, ExprCx, Imports, ident};
use crate::view::{Parent, ViewEmitter};
use crate::{Options, assemble, dependency_name, generated, params};

pub(crate) fn ui_state(s: &ScreenDef, o: &Options) -> String {
    let mut im = Imports::new(o.r_package());
    let mut w = CodeWriter::new("    ");
    w.line(format!(
        "/** UI state of {}, updated only by the view model */",
        s.name
    ));
    let head = if s.state.is_empty() {
        format!("class {}UiState", s.name)
    } else {
        format!("data class {}UiState(", s.name)
    };
    if !s.state.is_empty() {
        w.line(head);
        w.indent();
        for f in &s.state {
            let ty = kotlin::ty(&f.ty, &mut im);
            let default = kotlin::expr(ExprCx { state: "" }, &f.default, &mut im);
            w.line(format!("val {}: {ty} = {default},", ident(&f.name)));
        }
        w.dedent();
        if s.computed.is_empty() {
            w.line(")");
        } else {
            w.line(") {");
        }
    } else if s.computed.is_empty() {
        w.line(head);
    } else {
        w.line(format!("{head} {{"));
    }
    if !s.computed.is_empty() {
        w.indent();
        for (i, c) in s.computed.iter().enumerate() {
            if i > 0 {
                w.blank();
            }
            let ty = kotlin::ty(&c.ty, &mut im);
            let body = kotlin::expr(ExprCx { state: "" }, &c.expr, &mut im);
            w.line(format!("val {}: {ty}", ident(&c.name)));
            w.indent();
            w.line(format!("get() = {body}"));
            w.dedent();
        }
        w.dedent();
        w.line("}");
    }
    w.blank();
    w.line(format!(
        "/** Commands that the {} view can dispatch */",
        s.name
    ));
    let has_members = s.writable_fields().next().is_some() || !s.actions.is_empty();
    if !has_members {
        w.line(format!("interface {}Actions", s.name));
    } else {
        w.block(format!("interface {}Actions {{", s.name), "}", |w| {
            for f in s.writable_fields() {
                let setter = ident(&f.setter_name().unwrap_or_default());
                w.line(format!(
                    "fun {setter}(value: {})",
                    kotlin::ty(&f.ty, &mut im)
                ));
            }
            for a in &s.actions {
                w.line(format!(
                    "fun {}({})",
                    ident(&a.name),
                    params(&a.params, &mut im)
                ));
            }
        });
    }
    generated(&o.package, &im, &w.finish())
}

pub(crate) fn base(s: &ScreenDef, o: &Options) -> String {
    let mut im = Imports::new(o.r_package());
    for i in [
        "androidx.lifecycle.ViewModel",
        "kotlinx.coroutines.flow.MutableStateFlow",
        "kotlinx.coroutines.flow.StateFlow",
        "kotlinx.coroutines.flow.asStateFlow",
    ] {
        im.add(i);
    }
    let writable: Vec<_> = s.writable_fields().collect();
    if !writable.is_empty() {
        im.add("kotlinx.coroutines.flow.update");
    }
    let mut w = CodeWriter::new("    ");
    w.line(format!(
        "/** Base view model of {}, implement the actions in a subclass */",
        s.name
    ));
    w.block(
        format!(
            "abstract class {0}ViewModelBase : ViewModel(), {0}Actions {{",
            s.name
        ),
        "}",
        |w| {
            w.line(format!(
                "protected val mutableState = MutableStateFlow({}UiState())",
                s.name
            ));
            w.line(format!(
                "val state: StateFlow<{}UiState> = mutableState.asStateFlow()",
                s.name
            ));
            for f in &writable {
                w.blank();
                let setter = ident(&f.setter_name().unwrap_or_default());
                w.block(
                    format!(
                        "override fun {setter}(value: {}) {{",
                        kotlin::ty(&f.ty, &mut im)
                    ),
                    "}",
                    |w| {
                        w.line(format!(
                            "mutableState.update {{ it.copy({} = value) }}",
                            ident(&f.name)
                        ));
                    },
                );
            }
        },
    );
    generated(&o.package, &im, &w.finish())
}

pub(crate) fn content(s: &ScreenDef, o: &Options) -> String {
    let mut im = Imports::new(o.r_package());
    im.add("androidx.compose.runtime.Composable");
    im.add("androidx.compose.ui.Modifier");
    let mut w = CodeWriter::new("    ");
    w.line("@Composable");
    w.line(format!("fun {}Content(", s.name));
    w.indent();
    w.line(format!("state: {}UiState,", s.name));
    w.line(format!("actions: {}Actions,", s.name));
    w.line("modifier: Modifier = Modifier,");
    w.dedent();
    w.line(") {");
    w.indent();
    let mut em = ViewEmitter {
        cx: ExprCx { state: "state." },
        actions: "actions",
        im: &mut im,
        temp: 0,
    };
    // 外部传入的modifier作用于唯一的根节点
    match s.view.as_slice() {
        [ViewNode::Element(root)] => em.element(&mut w, root, Parent::Other, Some("modifier")),
        nodes => em.nodes(&mut w, nodes, Parent::Other),
    }
    w.dedent();
    w.line("}");
    generated(&o.package, &im, &w.finish())
}

fn construct(s: &ScreenDef) -> String {
    let args: Vec<_> = s
        .injects
        .iter()
        .map(|i| {
            format!(
                "{} = dependencies.{}",
                ident(&i.name),
                dependency_name(&i.ty)
            )
        })
        .collect();
    format!("{}ViewModel({})", s.name, args.join(", "))
}

pub(crate) fn route(s: &ScreenDef, o: &Options) -> String {
    let mut im = Imports::new(o.r_package());
    for i in [
        "androidx.compose.runtime.Composable",
        "androidx.compose.runtime.getValue",
        "androidx.compose.ui.Modifier",
        "androidx.lifecycle.compose.collectAsStateWithLifecycle",
        "androidx.lifecycle.viewmodel.compose.viewModel",
    ] {
        im.add(i);
    }
    let mut w = CodeWriter::new("    ");
    w.line(format!(
        "/** Entry point of {}, creates the view model and collects its state */",
        s.name
    ));
    w.line("@Composable");
    w.block(
        format!("fun {}Route(modifier: Modifier = Modifier) {{", s.name),
        "}",
        |w| {
            if !s.injects.is_empty() {
                w.line("val dependencies = LocalVeyraDependencies.current");
            }
            w.line(format!(
                "val viewModel: {0}ViewModel = viewModel {{ {1} }}",
                s.name,
                construct(s)
            ));
            w.line("val state by viewModel.state.collectAsStateWithLifecycle()");
            w.line(format!(
                "{}Content(state = state, actions = viewModel, modifier = modifier)",
                s.name
            ));
        },
    );
    generated(&o.package, &im, &w.finish())
}

pub(crate) fn view_model(s: &ScreenDef, o: &Options) -> String {
    let mut im = Imports::new(o.r_package());
    let has_async = s.actions.iter().any(|a| a.is_async);
    if has_async {
        im.add("androidx.lifecycle.viewModelScope");
        im.add("kotlinx.coroutines.launch");
    }
    let mut w = CodeWriter::new("    ");
    if s.injects.is_empty() {
        w.line(format!(
            "class {0}ViewModel : {0}ViewModelBase() {{",
            s.name
        ));
    } else {
        w.line(format!("class {}ViewModel(", s.name));
        w.indent();
        for i in &s.injects {
            w.line(format!("private val {}: {},", ident(&i.name), i.ty));
        }
        w.dedent();
        w.line(format!(") : {}ViewModelBase() {{", s.name));
    }
    w.indent();
    for (i, a) in s.actions.iter().enumerate() {
        if i > 0 {
            w.blank();
        }
        w.block(
            format!(
                "override fun {}({}) {{",
                ident(&a.name),
                params(&a.params, &mut im)
            ),
            "}",
            |w| {
                if a.is_async {
                    w.block("viewModelScope.launch {", "}", |w| {
                        w.line("// Implement business logic here")
                    });
                } else {
                    w.line("// Implement business logic here");
                }
            },
        );
    }
    w.dedent();
    w.line("}");
    assemble(&o.package, &im, &w.finish(), USER_OWNED_NOTICE)
}
