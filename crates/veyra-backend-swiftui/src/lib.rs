// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! Veyra的SwiftUI后端
//!
//! 目标为iOS 17+，状态使用Observation框架。每个screen生成：
//!
//! - `XxxState`：`@Observable`状态，computed映射为计算属性
//! - `XxxViewModelContract`：ViewModel契约，含@writable字段的默认setter
//! - `XxxView`：只依赖契约的泛型View，绑定一律经过setter
//! - `XxxRoot`：组装层，负责创建ViewModel并注入依赖
//! - `XxxViewModel`：开发者自有的实现骨架，仅首次生成
//!
//! 本地化文案生成类型安全的`L10n`访问器与String Catalog

mod l10n;
mod screen;
mod swift;
mod view;

use std::path::PathBuf;

use serde::Deserialize;
use veyra_codegen::{Backend, BackendInput, CodeWriter, GENERATED_NOTICE, OutputFile, camel_case};
use veyra_diagnostics::{Diagnostic, Diagnostics};
use veyra_ir::{ComponentParamType, Platform, Program};

use crate::swift::ident;

/// 依赖注入方式
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Di {
    /// 构造器注入，依赖从Environment读取
    #[default]
    Manual,
    /// 通过FactoryKit的`@Injected`注入
    Factory,
}

/// `[swiftui]`配置段
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    /// 生成文件目录
    pub out: PathBuf,
    /// ViewModel骨架目录
    pub user_dir: PathBuf,
    pub di: Di,
    /// String Catalog的表名
    pub table: String,
    /// FactoryKit的模块名，旧版本为`Factory`
    pub factory_module: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            out: "ios/Generated".into(),
            user_dir: "ios/ViewModels".into(),
            di: Di::Manual,
            table: "Veyra".into(),
            factory_module: "FactoryKit".into(),
        }
    }
}

pub struct SwiftUiBackend;

impl Backend for SwiftUiBackend {
    fn name(&self) -> &'static str {
        "swiftui"
    }

    fn platform(&self) -> Platform {
        Platform::Ios
    }

    fn generate(&self, input: &BackendInput<'_>) -> Result<Vec<OutputFile>, Diagnostics> {
        let options: Options = input
            .options
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| {
                Diagnostics::from(Diagnostic::error(
                    "V9001",
                    format!("[swiftui]配置无效：{}", e.message()),
                ))
            })?;
        let mut files = Vec::new();
        let out = &options.out;
        let program = input.program;

        if !program.enums.is_empty() || !program.models.is_empty() {
            files.push(OutputFile::generated(
                out.join("Models.generated.swift"),
                models(program),
            ));
        }
        if !program.repositories.is_empty() {
            files.push(OutputFile::generated(
                out.join("Repositories.generated.swift"),
                repositories(program),
            ));
        }
        let has_injects = program.screens.iter().any(|s| !s.injects.is_empty());
        if has_injects && options.di == Di::Factory {
            files.push(OutputFile::generated(
                out.join("Container+Veyra.generated.swift"),
                factory_container(program, &options),
            ));
        } else if has_injects {
            files.push(OutputFile::generated(
                out.join("VeyraDependencies.generated.swift"),
                dependencies(program),
            ));
        }
        let names = l10n::check_names(input.catalog)?;
        files.push(OutputFile::generated(
            out.join("L10n.generated.swift"),
            l10n::accessors(input.catalog, &names, &options.table),
        ));
        files.push(OutputFile::generated(
            out.join(format!("{}.xcstrings", options.table)),
            l10n::xcstrings(input.catalog),
        ));

        if !program.components.is_empty() {
            files.push(OutputFile::generated(
                out.join("Components.generated.swift"),
                components(program),
            ));
        }
        for s in &program.screens {
            let dir = out.join("Screens").join(&s.name);
            let file = |suffix: &str| dir.join(format!("{}{suffix}.generated.swift", s.name));
            files.push(OutputFile::generated(file("State"), screen::state(s)));
            files.push(OutputFile::generated(file("Contract"), screen::contract(s)));
            files.push(OutputFile::generated(file("View"), screen::view(s)));
            files.push(OutputFile::generated(
                file("Root"),
                screen::root(s, &options),
            ));
            files.push(OutputFile::user_owned(
                options.user_dir.join(format!("{}ViewModel.swift", s.name)),
                screen::view_model(s, &options),
            ));
        }
        Ok(files)
    }
}

pub(crate) fn header(w: &mut CodeWriter, imports: &[&str]) {
    w.line(format!("// {GENERATED_NOTICE}"));
    w.blank();
    for i in imports {
        w.line(format!("import {i}"));
    }
    w.blank();
}

fn models(program: &Program) -> String {
    let mut w = CodeWriter::new("    ");
    header(&mut w, &["Foundation"]);
    for e in &program.enums {
        w.block(
            format!(
                "enum {}: String, Codable, CaseIterable, Hashable, Sendable {{",
                e.name
            ),
            "}",
            |w| {
                for c in &e.cases {
                    w.line(format!("case {}", ident(c)));
                }
            },
        );
        w.blank();
    }
    for m in &program.models {
        let mut conforms = vec!["Codable", "Hashable", "Sendable"];
        if m.id_field().is_some() {
            conforms.push("Identifiable");
        }
        w.block(
            format!("struct {}: {} {{", m.name, conforms.join(", ")),
            "}",
            |w| {
                for f in &m.fields {
                    w.line(format!("var {}: {}", ident(&f.name), swift::ty(&f.ty)));
                }
            },
        );
        w.blank();
    }
    w.finish()
}

fn params(ps: &[veyra_ir::ParamDef]) -> String {
    ps.iter()
        .map(|p| format!("{}: {}", ident(&p.name), swift::ty(&p.ty)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn repositories(program: &Program) -> String {
    let mut w = CodeWriter::new("    ");
    header(&mut w, &["Foundation"]);
    for r in &program.repositories {
        w.line("@MainActor");
        w.block(format!("protocol {}: AnyObject {{", r.name), "}", |w| {
            for m in &r.methods {
                let mut sig = format!("func {}({})", ident(&m.name), params(&m.params));
                if m.is_async {
                    sig.push_str(" async");
                }
                if m.throws {
                    sig.push_str(" throws");
                }
                if let Some(ret) = &m.ret {
                    sig.push_str(&format!(" -> {}", swift::ty(ret)));
                }
                w.line(sig);
            }
        });
        w.blank();
    }
    w.finish()
}

/// 依赖类型在`VeyraDependencies`中的属性名
pub(crate) fn dependency_name(ty: &str) -> String {
    ident(&camel_case(ty))
}

fn dependencies(program: &Program) -> String {
    let mut types: Vec<&str> = program
        .screens
        .iter()
        .flat_map(|s| s.injects.iter().map(|i| i.ty.as_str()))
        .collect();
    types.sort();
    types.dedup();
    let mut w = CodeWriter::new("    ");
    header(&mut w, &["SwiftUI"]);
    w.line("/// Dependencies provided by the app and passed to view models by the root views");
    w.line("@MainActor");
    w.block("protocol VeyraDependencies: Sendable {", "}", |w| {
        for t in &types {
            w.line(format!("var {}: any {t} {{ get }}", dependency_name(t)));
        }
    });
    w.blank();
    w.block("extension EnvironmentValues {", "}", |w| {
        w.line("@Entry var veyraDependencies: (any VeyraDependencies)? = nil");
    });
    w.blank();
    w.block("extension View {", "}", |w| {
        w.line("/// Provides dependencies to descendant views");
        w.block(
            "func veyraDependencies(_ dependencies: any VeyraDependencies) -> some View {",
            "}",
            |w| {
                w.line("environment(\\.veyraDependencies, dependencies)");
            },
        );
    });
    w.finish()
}

/// 字段默认值
pub(crate) fn default_value(e: &veyra_ir::Expr) -> String {
    swift::expr(swift::ExprCx { state: "" }, e)
}

fn components(program: &Program) -> String {
    let mut w = CodeWriter::new("    ");
    header(&mut w, &["SwiftUI"]);
    for c in &program.components {
        w.block(format!("struct {}: View {{", c.name), "}", |w| {
            for p in &c.params {
                match &p.ty {
                    ComponentParamType::Value(t) => {
                        w.line(format!("let {}: {}", ident(&p.name), swift::ty(t)))
                    }
                    ComponentParamType::Action => {
                        w.line(format!("let {}: () -> Void", ident(&p.name)))
                    }
                    ComponentParamType::Binding(t) => {
                        w.line(format!("@Binding var {}: {}", ident(&p.name), swift::ty(t)))
                    }
                }
            }
            if !c.params.is_empty() {
                w.blank();
            }
            w.block("var body: some View {", "}", |w| {
                let cx = view::ViewCx {
                    expr: swift::ExprCx { state: "" },
                    vm: "",
                };
                view::nodes(w, cx, &c.body);
            });
        });
        w.blank();
    }
    w.finish()
}

/// FactoryKit的注册入口，默认实现需由应用注册
fn factory_container(program: &Program, options: &Options) -> String {
    let mut w = CodeWriter::new("    ");
    header(&mut w, &[options.factory_module.as_str()]);
    w.line(
        "/// Register implementations at launch, for example in `AutoRegistering.autoRegister()`",
    );
    w.block("extension Container {", "}", |w| {
        for (i, t) in injected_types(program).iter().enumerate() {
            if i > 0 {
                w.blank();
            }
            w.block(
                format!("var {}: Factory<any {t}> {{", dependency_name(t)),
                "}",
                |w| {
                    w.line(format!(
                        "self {{ @MainActor in fatalError(\"{t} is not registered\") }}"
                    ));
                },
            );
        }
    });
    w.finish()
}

fn injected_types(program: &Program) -> Vec<&str> {
    let mut types: Vec<&str> = program
        .screens
        .iter()
        .flat_map(|s| s.injects.iter().map(|i| i.ty.as_str()))
        .collect();
    types.sort();
    types.dedup();
    types
}
