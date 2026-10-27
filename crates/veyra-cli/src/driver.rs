// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 编译流程的编排

use std::collections::{HashMap, HashSet};
use std::path::Path;

use veyra_backend_compose::ComposeBackend;
use veyra_backend_swiftui::SwiftUiBackend;
use veyra_codegen::{Backend, BackendInput, OutputFile};
use veyra_diagnostics::{Diagnostic, Diagnostics, FileId, RenderOptions, SourceMap, render_all};
use veyra_ir::Program;
use veyra_project::{LintLevel, Project};
use veyra_sema::{NativeLint, Target, analyze};
use veyra_syntax::ast;

use crate::BuildArgs;

/// 已注册的后端，新增平台时在此登记
pub(crate) fn registry() -> Vec<Box<dyn Backend>> {
    vec![Box::new(SwiftUiBackend), Box::new(ComposeBackend)]
}

pub(crate) fn backend_names() -> Vec<&'static str> {
    registry().iter().map(|b| b.name()).collect()
}

struct Reporter {
    sources: SourceMap,
    diags: Diagnostics,
    color: bool,
}

impl Reporter {
    fn new(color: bool) -> Self {
        Self {
            sources: SourceMap::new(),
            diags: Diagnostics::new(),
            color,
        }
    }

    fn push(&mut self, d: Diagnostic) {
        self.diags.push(d);
    }

    fn extend(&mut self, d: Diagnostics) {
        self.diags.extend(d);
    }

    fn has_errors(&self) -> bool {
        self.diags.has_errors()
    }

    /// 输出并清空已收集的诊断
    fn flush(&mut self) {
        if self.diags.is_empty() {
            return;
        }
        let diags = std::mem::take(&mut self.diags).sorted();
        eprint!(
            "{}",
            render_all(&diags, &self.sources, RenderOptions { color: self.color })
        );
    }

    /// 输出诊断并返回是否成功
    fn finish(mut self) -> bool {
        let ok = !self.has_errors();
        self.flush();
        ok
    }
}

struct Compiled {
    project: Project,
    programs: Vec<(Box<dyn Backend>, Program)>,
    sync: veyra_l10n::Sync,
}

fn parse_flags(
    args: &BuildArgs,
    project: &Project,
    r: &mut Reporter,
) -> indexmap::IndexMap<String, bool> {
    let mut flags = project.config.flags.clone();
    for raw in &args.flags {
        let (name, value) = match raw.split_once('=') {
            Some((n, v)) => (n, v.parse::<bool>().ok()),
            None => (raw.as_str(), Some(true)),
        };
        match value {
            Some(v) if flags.contains_key(name) => {
                flags.insert(name.to_string(), v);
            }
            Some(_) => r.push(
                Diagnostic::error("V9005", format!("未声明的flag`{name}`"))
                    .with_note("请先在veyra.toml的[flags]中声明"),
            ),
            None => r.push(Diagnostic::error(
                "V9005",
                format!("flag`{raw}`的取值必须是true或false"),
            )),
        }
    }
    flags
}

/// 解析、检查全部源码并同步翻译文件，不写入磁盘
fn compile(dir: &Path, args: &BuildArgs, r: &mut Reporter) -> Option<Compiled> {
    let project = match Project::discover(dir, &backend_names()) {
        Ok(p) => p,
        Err(d) => {
            r.push(d);
            return None;
        }
    };
    let flags = parse_flags(args, &project, r);
    let mut backends: Vec<Box<dyn Backend>> = registry()
        .into_iter()
        .filter(|b| project.config.backends.contains_key(b.name()))
        .collect();
    for name in &args.backend {
        if !backends.iter().any(|b| b.name() == name) {
            r.push(Diagnostic::error(
                "V9006",
                format!("veyra.toml中没有配置后端`{name}`"),
            ));
        }
    }
    if !args.backend.is_empty() {
        backends.retain(|b| args.backend.iter().any(|n| n == b.name()));
    }
    if backends.is_empty() && !r.has_errors() {
        r.push(
            Diagnostic::error("V9006", "没有可用的后端").with_note(format!(
                "在veyra.toml中添加配置段以启用后端：{}",
                backend_names().join("、")
            )),
        );
    }
    let files = match project.source_files() {
        Ok(f) => f,
        Err(d) => {
            r.push(d);
            return None;
        }
    };
    if r.has_errors() {
        return None;
    }

    let mut asts = Vec::new();
    for rel in &files {
        if let Some(file) = load(&project.root, &slash(rel), r) {
            asts.push(file);
        }
    }
    resolve_imports(&project.root, &mut asts, r);
    if r.has_errors() {
        return None;
    }

    let mut programs = Vec::new();
    let mut programs_used = HashSet::new();
    let mut seen = Diagnostics::new();
    for backend in backends {
        let native = match project.config.lint.native {
            LintLevel::Allow => NativeLint::Allow,
            LintLevel::Warn => NativeLint::Warn,
            LintLevel::Deny => NativeLint::Deny,
        };
        let empty = toml::Table::new();
        let options = project
            .config
            .backends
            .get(backend.name())
            .unwrap_or(&empty);
        let apple_oses = match backend.apple_oses(options) {
            Ok(o) => o,
            Err(d) => {
                r.extend(d);
                continue;
            }
        };
        let target = Target {
            platform: backend.platform(),
            flags: flags.clone(),
            native,
            apple_oses,
            os: None,
        };
        let analysis = analyze(&asts, &target);
        // 多个Apple系统时，IR保留系统差异，诊断由逐个系统的检查给出
        let diagnostics = if target.apple_oses.len() > 1 {
            let mut per_os = Diagnostics::new();
            for os in &target.apple_oses {
                let single = Target {
                    os: Some(*os),
                    ..target.clone()
                };
                per_os.extend(analyze(&asts, &single).diagnostics);
            }
            // 条件结构本身的问题只在保留系统差异时才能发现
            // 互斥系统分支中的同名声明在单个系统上合法，但生成的代码只能保留一份
            let mut all = Diagnostics::new();
            for d in analysis.diagnostics {
                if d.code.starts_with("V60") {
                    all.push(d);
                } else if matches!(d.code, "V1001" | "V1003") && !per_os.iter().any(|x| *x == d) {
                    all.push(d.with_note(
                        "启用多个Apple系统时，按系统区分的声明也不能重名，请合并为一个声明并在view中用#if区分",
                    ));
                }
            }
            all.extend(per_os);
            all
        } else {
            analysis.diagnostics
        };
        // 多个平台报告的相同诊断只保留一份
        for d in diagnostics {
            if !seen.iter().any(|x| *x == d) {
                seen.push(d.clone());
                r.push(d);
            }
        }
        programs_used.extend(analysis.used_imports);
        programs.push((backend, analysis.program));
    }
    report_unused_imports(&asts, &programs_used, r);
    if r.has_errors() {
        return None;
    }

    let lists: Vec<_> = programs.iter().map(|(_, p)| p.strings.clone()).collect();
    let strings = match veyra_l10n::merge_strings(&lists) {
        Ok(s) => s,
        Err(d) => {
            r.extend(d);
            return None;
        }
    };
    let sync = veyra_l10n::sync(&strings, &project.config.l10n, &project.root);
    r.extend(sync.diagnostics.clone());
    if r.has_errors() {
        return None;
    }
    Some(Compiled {
        project,
        programs,
        sync,
    })
}

fn slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// 读取并解析一个源文件，`rel`为相对于项目根目录的路径
fn load(root: &Path, rel: &str, r: &mut Reporter) -> Option<ast::File> {
    let text = match std::fs::read_to_string(root.join(rel)) {
        Ok(t) => t,
        Err(e) => {
            r.push(Diagnostic::error("V9004", format!("无法读取{rel}：{e}")));
            return None;
        }
    };
    let id = r.sources.add(rel, text.as_str());
    let parsed = veyra_syntax::parse(id, &text);
    r.extend(parsed.diagnostics);
    let mut file = parsed.file;
    file.path = rel.to_string();
    Some(file)
}

/// 规范化`dir/path`，返回None表示路径越出了项目根目录
fn join_relative(dir: &str, path: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

/// 解析import路径，未在sources中的文件会被一并加载
fn resolve_imports(root: &Path, asts: &mut Vec<ast::File>, r: &mut Reporter) {
    let mut index: HashMap<String, FileId> = asts.iter().map(|f| (f.path.clone(), f.id)).collect();
    let mut i = 0;
    while i < asts.len() {
        let from = asts[i].path.clone();
        let dir = from
            .rsplit_once('/')
            .map(|(d, _)| d.to_string())
            .unwrap_or_default();
        let mut resolved = Vec::new();
        let imports: Vec<(String, veyra_diagnostics::Span)> = asts[i]
            .imports
            .iter()
            .map(|im| (im.path.clone(), im.span))
            .collect();
        for (path, span) in &imports {
            let (path, span) = (path.as_str(), *span);
            if !path.ends_with(".vey") {
                r.push(
                    Diagnostic::error("V1030", "import的路径必须以.vey结尾").with_label(span, ""),
                );
                resolved.push(None);
                continue;
            }
            let Some(target) = join_relative(&dir, path) else {
                r.push(
                    Diagnostic::error("V1030", "import的路径越出了项目目录").with_label(span, ""),
                );
                resolved.push(None);
                continue;
            };
            if target == from {
                r.push(Diagnostic::error("V1031", "文件不能import自身").with_label(span, ""));
                resolved.push(None);
                continue;
            }
            let id = match index.get(&target) {
                Some(id) => Some(*id),
                None if root.join(&target).is_file() => match load(root, &target, r) {
                    Some(file) => {
                        let id = file.id;
                        index.insert(target.clone(), id);
                        asts.push(file);
                        Some(id)
                    }
                    None => None,
                },
                None => {
                    r.push(
                        Diagnostic::error("V1030", format!("找不到import的文件{target}"))
                            .with_label(span, "")
                            .with_note("路径相对于当前文件所在的目录"),
                    );
                    None
                }
            };
            if id.is_some() && resolved.contains(&id) {
                r.push(Diagnostic::warning("V1032", "重复的import").with_label(span, ""));
            }
            resolved.push(id);
        }
        for (import, id) in asts[i].imports.iter_mut().zip(resolved) {
            import.resolved = id;
        }
        i += 1;
    }
}

/// 全部平台都没有用到的import
fn report_unused_imports(asts: &[ast::File], used: &HashSet<(FileId, FileId)>, r: &mut Reporter) {
    for file in asts {
        for import in &file.imports {
            if let Some(target) = import.resolved
                && !used.contains(&(file.id, target))
            {
                r.push(
                    Diagnostic::warning("V1033", format!("未使用的import`{}`", import.path))
                        .with_label(import.span, "")
                        .with_note("当前文件没有用到其中声明的类型"),
                );
            }
        }
    }
}

pub(crate) fn check(dir: &Path, args: &BuildArgs, color: bool) -> bool {
    let mut r = Reporter::new(color);
    let compiled = compile(dir, args, &mut r);
    let ok = r.finish();
    if let Some(c) = compiled
        && ok
    {
        let names: Vec<_> = c.programs.iter().map(|(b, _)| b.name()).collect();
        eprintln!("检查通过：{}", names.join("、"));
    }
    ok
}

pub(crate) fn build(dir: &Path, args: &BuildArgs, color: bool) -> bool {
    let mut r = Reporter::new(color);
    let Some(c) = compile(dir, args, &mut r) else {
        return r.finish();
    };

    let mut outputs: Vec<OutputFile> = Vec::new();
    for (backend, program) in &c.programs {
        let empty = toml::Table::new();
        let options = c
            .project
            .config
            .backends
            .get(backend.name())
            .unwrap_or(&empty);
        match backend.generate(&BackendInput {
            program,
            catalog: &c.sync.catalog,
            options,
        }) {
            Ok(files) => outputs.extend(files),
            Err(d) => r.extend(d),
        }
    }
    if r.has_errors() {
        return r.finish();
    }

    // 翻译文件也是项目的一部分，但归开发者维护，不进入manifest
    for f in &c.sync.files {
        if f.changed {
            let path = c.project.root.join(&f.path);
            let result = path
                .parent()
                .map(std::fs::create_dir_all)
                .unwrap_or(Ok(()))
                .and_then(|_| std::fs::write(&path, &f.contents));
            if let Err(e) = result {
                r.push(Diagnostic::error(
                    "V9007",
                    format!("无法写入{}：{e}", f.path.display()),
                ));
            }
        }
    }
    let report = match veyra_project::write_outputs(&c.project.root, &outputs) {
        Ok(report) => report,
        Err(e) => {
            r.push(Diagnostic::error("V9007", format!("写入生成文件失败：{e}")));
            return r.finish();
        }
    };
    let ok = r.finish();
    for p in &report.created {
        eprintln!("    创建 {}", p.display());
    }
    for p in &report.removed {
        eprintln!("    删除 {}", p.display());
    }
    let updated_locales = c.sync.files.iter().filter(|f| f.changed).count();
    eprintln!(
        "完成：写入{}个文件，{}个未变化，创建{}个骨架，保留{}个已有文件，更新{}个翻译文件",
        report.written.len(),
        report.unchanged.len(),
        report.created.len(),
        report.kept.len(),
        updated_locales
    );
    ok
}

pub(crate) fn l10n_status(dir: &Path, color: bool) -> bool {
    let mut r = Reporter::new(color);
    let Some(c) = compile(dir, &BuildArgs::default(), &mut r) else {
        return r.finish();
    };
    let total = c.sync.catalog.entries.len();
    let ok = r.finish();
    println!("源语言 {}：{total}条", c.sync.catalog.source_locale);
    for f in &c.sync.files {
        let done = total - f.missing.len();
        let percent = (done * 100).checked_div(total).unwrap_or(100);
        println!(
            "{:<10} {done}/{total} ({percent}%)，过时{}条",
            f.locale,
            f.stale.len()
        );
    }
    ok
}

pub(crate) fn l10n_prune(dir: &Path, color: bool) -> bool {
    let mut r = Reporter::new(color);
    let Some(c) = compile(dir, &BuildArgs::default(), &mut r) else {
        return r.finish();
    };
    let mut removed = 0;
    for f in &c.sync.files {
        if f.stale.is_empty() && !f.changed {
            continue;
        }
        removed += f.stale.len();
        let path = c.project.root.join(&f.path);
        if let Err(e) = std::fs::write(&path, veyra_l10n::prune(f)) {
            r.push(Diagnostic::error(
                "V9007",
                format!("无法写入{}：{e}", f.path.display()),
            ));
        }
    }
    let ok = r.finish();
    println!("已删除{removed}条过时的翻译");
    ok
}

/// 格式化文件，`check`为真时只报告需要格式化的文件
pub(crate) fn fmt(dir: &Path, files: &[std::path::PathBuf], check: bool, color: bool) -> bool {
    let mut r = Reporter::new(color);
    let targets: Vec<std::path::PathBuf> = if files.is_empty() {
        let project = match Project::discover(dir, &backend_names()) {
            Ok(p) => p,
            Err(d) => {
                r.push(d);
                return r.finish();
            }
        };
        match project.source_files() {
            Ok(fs) => fs.into_iter().map(|f| project.root.join(f)).collect(),
            Err(d) => {
                r.push(d);
                return r.finish();
            }
        }
    } else {
        files.iter().map(|f| dir.join(f)).collect()
    };
    let mut changed = Vec::new();
    for path in &targets {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                r.push(Diagnostic::error(
                    "V9004",
                    format!("无法读取{}：{e}", path.display()),
                ));
                continue;
            }
        };
        let id = r.sources.add(path, text.as_str());
        let parsed = veyra_syntax::parse(id, &text);
        if parsed.diagnostics.has_errors() {
            r.extend(parsed.diagnostics);
            continue;
        }
        let formatted = veyra_syntax::format(&parsed, &text);
        if formatted == text {
            continue;
        }
        changed.push(path.clone());
        if !check && let Err(e) = std::fs::write(path, formatted) {
            r.push(Diagnostic::error(
                "V9007",
                format!("无法写入{}：{e}", path.display()),
            ));
        }
    }
    let mut ok = r.finish();
    for p in &changed {
        eprintln!(
            "    {} {}",
            if check {
                "需要格式化"
            } else {
                "已格式化"
            },
            p.display()
        );
    }
    if check && !changed.is_empty() {
        ok = false;
    }
    ok
}
