// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 单个screen的Swift文件

use veyra_codegen::{CodeWriter, USER_OWNED_NOTICE};
use veyra_ir::ScreenDef;

use crate::swift::{self, ExprCx, ident};
use crate::view::{self, ViewCx};
use crate::{Options, default_value, dependency_name, header, params};

pub(crate) fn state(s: &ScreenDef) -> String {
    let mut w = CodeWriter::new("    ");
    header(&mut w, &["Foundation", "Observation"]);
    w.line(format!(
        "/// Observable state of {}, written only by the view model",
        s.name
    ));
    w.line("@MainActor");
    w.line("@Observable");
    w.block(format!("final class {}State {{", s.name), "}", |w| {
        for f in &s.state {
            w.line(format!(
                "var {}: {} = {}",
                ident(&f.name),
                swift::ty(&f.ty),
                default_value(&f.default)
            ));
        }
        if !s.computed.is_empty() {
            w.blank();
            for c in &s.computed {
                let body = swift::expr(ExprCx { state: "" }, &c.expr);
                w.line(format!(
                    "var {}: {} {{ {body} }}",
                    ident(&c.name),
                    swift::ty(&c.ty)
                ));
            }
        }
        w.blank();
        w.line("init() {}");
    });
    w.finish()
}

fn action_sig(a: &veyra_ir::ActionDef) -> String {
    let mut sig = format!("func {}({})", ident(&a.name), params(&a.params));
    if a.is_async {
        sig.push_str(" async");
    }
    sig
}

pub(crate) fn contract(s: &ScreenDef) -> String {
    let mut w = CodeWriter::new("    ");
    header(&mut w, &["Foundation"]);
    w.line(format!(
        "/// View model contract of {}, the only type the view depends on",
        s.name
    ));
    w.line("@MainActor");
    w.block(
        format!("protocol {}ViewModelContract: AnyObject {{", s.name),
        "}",
        |w| {
            w.line(format!("var state: {}State {{ get }}", s.name));
            let setters: Vec<_> = s.writable_fields().collect();
            if !setters.is_empty() {
                w.blank();
                for f in &setters {
                    w.line(format!(
                        "func {}(_ value: {})",
                        ident(&f.setter_name().unwrap_or_default()),
                        swift::ty(&f.ty)
                    ));
                }
            }
            if !s.actions.is_empty() {
                w.blank();
                for a in &s.actions {
                    w.line(action_sig(a));
                }
            }
        },
    );
    let setters: Vec<_> = s.writable_fields().collect();
    if !setters.is_empty() {
        w.blank();
        w.line("/// Default setters write state directly, override them in the view model to validate input");
        w.block(
            format!("extension {}ViewModelContract {{", s.name),
            "}",
            |w| {
                for (i, f) in setters.iter().enumerate() {
                    if i > 0 {
                        w.blank();
                    }
                    let setter = ident(&f.setter_name().unwrap_or_default());
                    w.block(
                        format!("func {setter}(_ value: {}) {{", swift::ty(&f.ty)),
                        "}",
                        |w| {
                            w.line(format!("state.{} = value", ident(&f.name)));
                        },
                    );
                }
            },
        );
    }
    w.finish()
}

pub(crate) fn view(s: &ScreenDef) -> String {
    let mut w = CodeWriter::new("    ");
    header(&mut w, &["SwiftUI"]);
    w.block(
        format!(
            "struct {0}View<ViewModel: {0}ViewModelContract>: View {{",
            s.name
        ),
        "}",
        |w| {
            w.line("let viewModel: ViewModel");
            w.blank();
            w.block("var body: some View {", "}", |w| {
                let cx = ViewCx {
                    expr: ExprCx {
                        state: "viewModel.state.",
                    },
                    vm: "viewModel",
                };
                view::nodes(w, cx, &s.view);
            });
        },
    );
    w.finish()
}

fn construct(s: &ScreenDef, deps: &str) -> String {
    let args: Vec<_> = s
        .injects
        .iter()
        .map(|i| format!("{}: {deps}.{}", ident(&i.name), dependency_name(&i.ty)))
        .collect();
    format!("{}ViewModel({})", s.name, args.join(", "))
}

pub(crate) fn root(s: &ScreenDef, _options: &Options) -> String {
    let mut w = CodeWriter::new("    ");
    header(&mut w, &["SwiftUI"]);
    w.line(format!(
        "/// Entry point of {}, creates the view model and injects dependencies",
        s.name
    ));
    w.block(format!("struct {}Root: View {{", s.name), "}", |w| {
        if s.injects.is_empty() {
            w.line(format!("@State private var viewModel = {}", construct(s, "")));
            w.blank();
            w.block("var body: some View {", "}", |w| {
                w.line(format!("{}View(viewModel: viewModel)", s.name));
            });
            return;
        }
        w.line("@Environment(\\.veyraDependencies) private var dependencies");
        w.line(format!("@State private var viewModel: {}ViewModel?", s.name));
        w.blank();
        w.block("var body: some View {", "}", |w| {
            w.block("if let viewModel {", "} else {", |w| {
                w.line(format!("{}View(viewModel: viewModel)", s.name));
            });
            w.indent();
            w.block("Color.clear.onAppear {", "}", |w| {
                w.block("guard let dependencies else {", "}", |w| {
                    w.line("assertionFailure(\"VeyraDependencies is missing, call veyraDependencies(_:) on an ancestor view\")");
                    w.line("return");
                });
                w.line(format!("viewModel = {}", construct(s, "dependencies")));
            });
            w.dedent();
            w.line("}");
        });
    });
    w.finish()
}

pub(crate) fn view_model(s: &ScreenDef, _options: &Options) -> String {
    let mut w = CodeWriter::new("    ");
    w.line(format!("// {USER_OWNED_NOTICE}"));
    w.blank();
    w.line("import Foundation");
    w.blank();
    w.line("@MainActor");
    w.block(
        format!("final class {0}ViewModel: {0}ViewModelContract {{", s.name),
        "}",
        |w| {
            w.line(format!("let state = {}State()", s.name));
            if !s.injects.is_empty() {
                w.blank();
                for i in &s.injects {
                    w.line(format!("private let {}: any {}", ident(&i.name), i.ty));
                }
                w.blank();
                let ps: Vec<_> = s
                    .injects
                    .iter()
                    .map(|i| format!("{}: any {}", ident(&i.name), i.ty))
                    .collect();
                w.block(format!("init({}) {{", ps.join(", ")), "}", |w| {
                    for i in &s.injects {
                        w.line(format!("self.{0} = {0}", ident(&i.name)));
                    }
                });
            }
            for a in &s.actions {
                w.blank();
                w.block(format!("{} {{", action_sig(a)), "}", |w| {
                    w.line("// Implement business logic here");
                });
            }
        },
    );
    w.finish()
}
