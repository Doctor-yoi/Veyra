// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! Veyra的Jetpack Compose后端
//!
//! 采用状态提升与单向数据流，每个screen生成：
//!
//! - `XxxUiState`：不可变的data class，computed映射为getter
//! - `XxxActions`：View可调用的命令接口，含@writable字段的setter
//! - `XxxViewModelBase`：持有`StateFlow`，提供setter默认实现
//! - `XxxContent`：无状态Composable，只接收state与actions
//! - `XxxRoute`：组装层，创建ViewModel并订阅状态
//! - `XxxViewModel`：开发者自有的实现骨架，仅首次生成
//!
//! 本地化文案生成到`res/values*/veyra_strings.xml`

mod kotlin;
mod resources;
mod screen;
mod storage;
mod view;

use std::path::PathBuf;

use serde::Deserialize;
use veyra_codegen::{Backend, BackendInput, CodeWriter, GENERATED_NOTICE, OutputFile, camel_case};
use veyra_diagnostics::{Diagnostic, Diagnostics};
use veyra_ir::{ComponentParamType, Platform, Program};

use crate::kotlin::{Imports, ident};

/// 依赖注入方式
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Di {
    /// 通过CompositionLocal提供依赖，构造器注入ViewModel
    #[default]
    Manual,
    /// 使用Hilt注入ViewModel
    Hilt,
}

/// `[compose]`配置段
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    /// Kotlin源码根目录
    pub out: PathBuf,
    /// 资源目录
    pub res: PathBuf,
    /// ViewModel骨架所在的源码根目录
    pub user_dir: PathBuf,
    /// 生成代码的包名
    pub package: String,
    /// R类所在的包名，默认与package相同
    pub r_package: Option<String>,
    pub di: Di,
    /// Room数据库文件名
    pub database: String,
    /// Room数据库版本，表结构变化时递增
    pub database_version: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            out: "android/app/src/main/java".into(),
            res: "android/app/src/main/res".into(),
            user_dir: "android/app/src/main/java".into(),
            package: "com.example.app.ui".into(),
            r_package: None,
            di: Di::Manual,
            database: "veyra.db".into(),
            database_version: 1,
        }
    }
}

impl Options {
    fn r_package(&self) -> &str {
        self.r_package.as_deref().unwrap_or(&self.package)
    }

    fn package_dir(&self, root: &std::path::Path) -> PathBuf {
        let mut dir = root.to_path_buf();
        for part in self.package.split('.') {
            dir.push(part);
        }
        dir
    }
}

pub struct ComposeBackend;

impl Backend for ComposeBackend {
    fn name(&self) -> &'static str {
        "compose"
    }

    fn platform(&self) -> Platform {
        Platform::Android
    }

    fn generate(&self, input: &BackendInput<'_>) -> Result<Vec<OutputFile>, Diagnostics> {
        let options: Options = input
            .options
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| {
                Diagnostics::from(Diagnostic::error(
                    "V9001",
                    format!("[compose]配置无效：{}", e.message()),
                ))
            })?;
        if !options
            .package
            .split('.')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        {
            return Err(Diagnostic::error(
                "V9001",
                format!("[compose]的package`{}`不是合法的包名", options.package),
            )
            .into());
        }
        let program = input.program;
        let dir = options.package_dir(&options.out);
        let user_dir = options.package_dir(&options.user_dir);
        let mut files = Vec::new();
        let file = |name: &str| dir.join(format!("{name}.generated.kt"));

        if !program.enums.is_empty() || !program.models.is_empty() {
            files.push(OutputFile::generated(
                file("Models"),
                models(program, &options),
            ));
        }
        if !program.repositories.is_empty() {
            files.push(OutputFile::generated(
                file("Repositories"),
                repositories(program, &options),
            ));
        }
        if options.di == Di::Manual && program.screens.iter().any(|s| !s.injects.is_empty()) {
            files.push(OutputFile::generated(
                file("VeyraDependencies"),
                dependencies(program, &options),
            ));
        }
        if program.any_expr(|e| matches!(e.kind, veyra_ir::ExprKind::Format { .. })) {
            files.push(OutputFile::generated(
                file("VeyraFormat"),
                format_helpers(&options),
            ));
        }
        files.extend(resources::strings(input.catalog, &options.res)?);

        if !program.entities.is_empty() {
            for e in &program.entities {
                files.push(OutputFile::generated(
                    file(&format!("{}Entity", e.name)),
                    storage::entity(e, &options),
                ));
            }
            for st in &program.stores {
                files.push(OutputFile::generated(
                    file(&st.name),
                    storage::store(st, program, &options),
                ));
            }
            files.push(OutputFile::generated(
                file("VeyraDatabase"),
                storage::database(program, &options),
            ));
            files.push(OutputFile::user_owned(
                user_dir.join("VeyraMigrations.kt"),
                storage::migrations(&options),
            ));
        }
        if !program.components.is_empty() {
            files.push(OutputFile::generated(
                file("Components"),
                components(program, &options),
            ));
        }
        for s in &program.screens {
            files.push(OutputFile::generated(
                file(&format!("{}UiState", s.name)),
                screen::ui_state(s, &options),
            ));
            files.push(OutputFile::generated(
                file(&format!("{}ViewModelBase", s.name)),
                screen::base(s, &options),
            ));
            files.push(OutputFile::generated(
                file(&format!("{}Content", s.name)),
                screen::content(s, &options),
            ));
            files.push(OutputFile::generated(
                file(&format!("{}Route", s.name)),
                screen::route(s, &options),
            ));
            files.push(OutputFile::user_owned(
                user_dir.join(format!("{}ViewModel.kt", s.name)),
                screen::view_model(s, &options),
            ));
        }
        Ok(files)
    }
}

/// 输出文件头、包名与import，再拼接正文
pub(crate) fn assemble(package: &str, im: &Imports, body: &str, notice: &str) -> String {
    let mut w = CodeWriter::new("    ");
    w.line(format!("// {notice}"));
    w.blank();
    w.line(format!("package {package}"));
    w.blank();
    let mut any = false;
    for i in im.iter() {
        // 同包内的类型无需导入
        if i.rsplit_once('.').is_some_and(|(p, _)| p == package) {
            continue;
        }
        w.line(format!("import {i}"));
        any = true;
    }
    if any {
        w.blank();
    }
    let mut out = w.finish();
    out.push('\n');
    out.push_str(body);
    out
}

pub(crate) fn generated(package: &str, im: &Imports, body: &str) -> String {
    assemble(package, im, body, GENERATED_NOTICE)
}

fn models(program: &Program, options: &Options) -> String {
    let mut im = Imports::new(options.r_package());
    let mut w = CodeWriter::new("    ");
    for e in &program.enums {
        let cases: Vec<_> = e.cases.iter().map(|c| ident(c)).collect();
        w.line(format!("enum class {} {{ {} }}", e.name, cases.join(", ")));
        w.blank();
    }
    for m in &program.models {
        w.block(format!("data class {}(", m.name), ")", |w| {
            for f in &m.fields {
                w.line(format!(
                    "val {}: {},",
                    ident(&f.name),
                    kotlin::ty(&f.ty, &mut im)
                ));
            }
        });
        w.blank();
    }
    generated(&options.package, &im, &w.finish())
}

pub(crate) fn params(ps: &[veyra_ir::ParamDef], im: &mut Imports) -> String {
    ps.iter()
        .map(|p| format!("{}: {}", ident(&p.name), kotlin::ty(&p.ty, im)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn repositories(program: &Program, options: &Options) -> String {
    let mut im = Imports::new(options.r_package());
    let mut w = CodeWriter::new("    ");
    for r in &program.repositories {
        w.block(format!("interface {} {{", r.name), "}", |w| {
            for m in &r.methods {
                let prefix = if m.is_async { "suspend fun" } else { "fun" };
                let mut sig = format!(
                    "{prefix} {}({})",
                    ident(&m.name),
                    params(&m.params, &mut im)
                );
                if let Some(ret) = &m.ret {
                    sig.push_str(&format!(": {}", kotlin::ty(ret, &mut im)));
                }
                w.line(sig);
            }
        });
        w.blank();
    }
    generated(&options.package, &im, &w.finish())
}

/// 依赖类型在`VeyraDependencies`中的属性名
pub(crate) fn dependency_name(ty: &str) -> String {
    ident(&camel_case(ty))
}

fn dependencies(program: &Program, options: &Options) -> String {
    let mut types: Vec<&str> = program
        .screens
        .iter()
        .flat_map(|s| s.injects.iter().map(|i| i.ty.as_str()))
        .collect();
    types.sort();
    types.dedup();
    let mut im = Imports::new(options.r_package());
    im.add("androidx.compose.runtime.staticCompositionLocalOf");
    let mut w = CodeWriter::new("    ");
    w.line("/** Dependencies provided by the app and passed to view models by the routes */");
    w.block("interface VeyraDependencies {", "}", |w| {
        for t in &types {
            w.line(format!("val {}: {t}", dependency_name(t)));
        }
    });
    w.blank();
    w.block(
        "val LocalVeyraDependencies = staticCompositionLocalOf<VeyraDependencies> {",
        "}",
        |w| {
            w.line(
                "error(\"VeyraDependencies is missing, provide it with CompositionLocalProvider\")",
            );
        },
    );
    generated(&options.package, &im, &w.finish())
}

/// `format()`使用的格式化函数，跟随系统区域设置
fn format_helpers(options: &Options) -> String {
    let mut im = Imports::new(options.r_package());
    for i in [
        "java.text.NumberFormat",
        "java.time.Instant",
        "java.time.ZoneId",
        "java.time.format.DateTimeFormatter",
        "java.time.format.FormatStyle",
        "java.util.Currency",
    ] {
        im.add(i);
    }
    let body = r#"internal fun veyraFormatNumber(value: Number): String = NumberFormat.getInstance().format(value)

internal fun veyraFormatPercent(value: Double): String = NumberFormat.getPercentInstance().format(value)

internal fun veyraFormatCurrency(value: Number, code: String): String =
    NumberFormat.getCurrencyInstance().apply { currency = Currency.getInstance(code) }.format(value)

internal fun veyraFormatDate(value: Instant, date: FormatStyle? = null, time: FormatStyle? = null): String {
    val formatter = when {
        date != null && time != null -> DateTimeFormatter.ofLocalizedDateTime(date, time)
        date != null -> DateTimeFormatter.ofLocalizedDate(date)
        else -> DateTimeFormatter.ofLocalizedTime(time ?: FormatStyle.SHORT)
    }
    return formatter.withZone(ZoneId.systemDefault()).format(value)
}
"#;
    generated(&options.package, &im, body)
}

fn components(program: &Program, options: &Options) -> String {
    let mut im = Imports::new(options.r_package());
    im.add("androidx.compose.runtime.Composable");
    im.add("androidx.compose.ui.Modifier");
    let mut w = CodeWriter::new("    ");
    for c in &program.components {
        w.line("@Composable");
        w.line(format!("fun {}(", c.name));
        w.indent();
        for p in &c.params {
            let name = ident(&p.name);
            match &p.ty {
                ComponentParamType::Value(t) => {
                    w.line(format!("{name}: {},", kotlin::ty(t, &mut im)))
                }
                ComponentParamType::Action => w.line(format!("{name}: () -> Unit,")),
                ComponentParamType::Binding(t) => {
                    let t = kotlin::ty(t, &mut im);
                    w.line(format!("{name}: {t},"));
                    w.line(format!(
                        "on{}Change: ({t}) -> Unit,",
                        veyra_codegen::pascal_case(&p.name)
                    ));
                }
            }
        }
        w.line("modifier: Modifier = Modifier,");
        w.dedent();
        w.line(") {");
        w.indent();
        let mut em = view::ViewEmitter {
            cx: kotlin::ExprCx { state: "" },
            actions: "",
            im: &mut im,
            temp: 0,
        };
        match c.body.as_slice() {
            [veyra_ir::ViewNode::Element(root)] => {
                em.element(&mut w, root, view::Parent::Other, Some("modifier"))
            }
            nodes => em.nodes(&mut w, nodes, view::Parent::Other),
        }
        w.dedent();
        w.line("}");
        w.blank();
    }
    generated(&options.package, &im, &w.finish())
}
