// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! Veyra语义检查
//!
//! 对每个目标平台单独执行一次：
//!
//! 1. 按平台与flag裁剪条件编译分支
//! 2. 收集全局声明并解析类型
//! 3. 检查表达式类型与MVVM规则（只读状态、绑定、命令分发、纯表达式）
//! 4. 收集本地化文案
//! 5. 降级为[`veyra_ir::Program`]
//!
//! 即使存在错误也会返回尽可能完整的IR，调用方应先检查诊断再使用IR

mod catalog;
mod cfg;
mod component;
mod decls;
mod expr;
mod screen;
mod storage;
mod view;

use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

use veyra_diagnostics::{Diagnostic, Diagnostics, FileId, Span};
use veyra_ir::{
    AppleOs, ComponentParam, EntityDef, EnumDef, LocString, ModelDef, NativeCond, Platform,
    Program, RepositoryDef, SecureDef, SettingsDef, StoreDef,
};
use veyra_syntax::ast;

use crate::cfg::Cfg;

/// native代码块的检查级别
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativeLint {
    Allow,
    #[default]
    Warn,
    Deny,
}

/// 一次检查的目标
#[derive(Clone, Debug)]
pub struct Target {
    pub platform: Platform,
    /// veyra.toml中声明的自定义flag及取值
    pub flags: IndexMap<String, bool>,
    pub native: NativeLint,
    /// SwiftUI后端启用的Apple系统
    pub apple_oses: Vec<AppleOs>,
    /// 按单个系统检查时的系统，None表示把系统差异保留为原生条件
    pub os: Option<AppleOs>,
}

impl Target {
    pub fn new(platform: Platform) -> Self {
        Self {
            platform,
            flags: IndexMap::new(),
            native: NativeLint::Warn,
            apple_oses: vec![AppleOs::Ios],
            os: None,
        }
    }
}

/// 检查结果
#[derive(Debug)]
pub struct Analysis {
    pub program: Program,
    pub diagnostics: Diagnostics,
    /// 实际用到的import，用于在全部平台检查完后报告未使用的import
    pub used_imports: HashSet<(FileId, FileId)>,
}

/// 对一组源文件执行语义检查
pub fn analyze(files: &[ast::File], target: &Target) -> Analysis {
    let mut checker = Checker::new(target);
    let program = checker.run(files);
    Analysis {
        program,
        diagnostics: checker.diags.sorted(),
        used_imports: checker.used_imports,
    }
}

/// 全局声明
#[derive(Clone, Copy)]
pub(crate) enum Global<'a> {
    Enum(&'a ast::EnumDecl),
    Model(&'a ast::ModelDecl),
    Repository(&'a ast::RepositoryDecl),
    Screen(&'a ast::ScreenDecl),
    Component(&'a ast::ComponentDecl),
    Entity(&'a ast::EntityDecl),
    Store(&'a ast::StoreDecl),
    Settings(&'a ast::FieldsDecl),
    Secure(&'a ast::FieldsDecl),
}

pub(crate) struct Checker<'a> {
    pub target: &'a Target,
    pub cfg: Cfg<'a>,
    pub diags: Diagnostics,
    pub globals: IndexMap<String, (Global<'a>, Span)>,
    pub enums: IndexMap<String, EnumDef>,
    pub models: IndexMap<String, ModelDef>,
    pub repositories: IndexMap<String, RepositoryDef>,
    pub entities: IndexMap<String, EntityDef>,
    pub stores: IndexMap<String, StoreDef>,
    pub settings: IndexMap<String, SettingsDef>,
    pub secures: IndexMap<String, SecureDef>,
    /// 组件签名
    pub components: IndexMap<String, Vec<ComponentParam>>,
    pub strings: IndexMap<String, (LocString, Span)>,
    /// 正在检查的声明所在的文件
    pub current: FileId,
    /// 每个文件可见的其他文件，即直接import的文件
    pub visible: HashMap<FileId, HashSet<FileId>>,
    pub paths: HashMap<FileId, String>,
    /// 实际用到的import，键为(导入方, 被导入方)
    pub used_imports: HashSet<(FileId, FileId)>,
    /// 只在部分Apple系统上存在的screen与component
    pub item_conds: HashMap<String, NativeCond>,
}

impl<'a> Checker<'a> {
    fn new(target: &'a Target) -> Self {
        Self {
            target,
            cfg: Cfg::new(target),
            diags: Diagnostics::new(),
            globals: IndexMap::new(),
            enums: IndexMap::new(),
            models: IndexMap::new(),
            repositories: IndexMap::new(),
            entities: IndexMap::new(),
            stores: IndexMap::new(),
            settings: IndexMap::new(),
            secures: IndexMap::new(),
            components: IndexMap::new(),
            strings: IndexMap::new(),
            current: FileId::default(),
            visible: HashMap::new(),
            paths: HashMap::new(),
            used_imports: HashSet::new(),
            item_conds: HashMap::new(),
        }
    }

    pub fn error(
        &mut self,
        code: &'static str,
        message: impl Into<String>,
        span: Span,
        label: impl Into<String>,
    ) {
        self.diags
            .push(Diagnostic::error(code, message).with_label(span, label));
    }

    /// 声明层级不允许保留原生条件
    pub fn prune_decls<T>(&mut self, nodes: &'a [ast::Node<T>]) -> Vec<&'a T> {
        self.prune_members(nodes, false)
            .into_iter()
            .map(|(item, _)| item)
            .collect()
    }

    /// 裁剪声明，`allow_os`为真时允许按Apple系统区分，返回各声明的原生条件
    pub fn prune_members<T>(
        &mut self,
        nodes: &'a [ast::Node<T>],
        allow_os: bool,
    ) -> Vec<(&'a T, Option<NativeCond>)> {
        let pruned = self.cfg.prune(nodes, &mut self.diags);
        let mut out = Vec::new();
        for p in pruned {
            if p.depends_on_debug() {
                self.diags.push(
                    Diagnostic::error("V6002", "`debug`条件只能用于view与修饰符")
                        .with_label(p.cond_span, "")
                        .with_note("声明在调试与发布构建中必须一致，平台与自定义flag不受此限制"),
                );
                continue;
            }
            if p.depends_on_os() && !allow_os {
                self.diags.push(
                    Diagnostic::error("V6005", "此处的声明不能按Apple系统区分")
                        .with_label(p.cond_span, "")
                        .with_note("按系统区分只支持screen、component、screen中的成员、view与修饰符；可以改用apple、android条件"),
                );
                continue;
            }
            out.push((p.item, p.cond));
        }
        out
    }

    fn run(&mut self, files: &'a [ast::File]) -> Program {
        for file in files {
            let set = file
                .imports
                .iter()
                .filter_map(|i| i.resolved)
                .filter(|f| *f != file.id)
                .collect();
            self.visible.insert(file.id, set);
            self.paths.insert(file.id, file.path.clone());
        }
        for file in files {
            for (item, cond) in self.prune_members(&file.items, true) {
                if cond.is_some() && !matches!(item, ast::Item::Screen(_) | ast::Item::Component(_))
                {
                    self.diags.push(
                        Diagnostic::error(
                            "V6005",
                            format!("`{}`声明不能按Apple系统区分", item.keyword()),
                        )
                        .with_label(item.name().span, "")
                        .with_note("只有screen与component可以只在部分Apple系统上存在"),
                    );
                    continue;
                }
                if let Some(c) = cond {
                    self.item_conds.insert(item.name().name.clone(), c);
                }
                self.register(item);
            }
        }
        self.check_decls();
        self.check_component_signatures();
        let components = self.check_components();
        let screens = self.check_screens();
        let mut strings: Vec<LocString> = self.strings.values().map(|(s, _)| s.clone()).collect();
        strings.sort_by(|a, b| a.key.cmp(&b.key));
        Program {
            enums: self.enums.values().cloned().collect(),
            models: self.models.values().cloned().collect(),
            repositories: self.repositories.values().cloned().collect(),
            entities: self.entities.values().cloned().collect(),
            stores: self.stores.values().cloned().collect(),
            settings: self.settings.values().cloned().collect(),
            secures: self.secures.values().cloned().collect(),
            components,
            screens,
            strings,
        }
    }

    fn register(&mut self, item: &'a ast::Item) {
        let name = item.name();
        let global = match item {
            ast::Item::Enum(d) => Global::Enum(d),
            ast::Item::Model(d) => Global::Model(d),
            ast::Item::Repository(d) => Global::Repository(d),
            ast::Item::Screen(d) => Global::Screen(d),
            ast::Item::Component(d) => Global::Component(d),
            ast::Item::Entity(d) => Global::Entity(d),
            ast::Item::Store(d) => Global::Store(d),
            ast::Item::Settings(d) => Global::Settings(d),
            ast::Item::Secure(d) => Global::Secure(d),
        };
        if is_builtin_type(&name.name) {
            self.error(
                "V1002",
                format!("`{}`是内置类型名称", name.name),
                name.span,
                "请换一个名称",
            );
            return;
        }
        if PLATFORM_TYPES.contains(&name.name.as_str()) {
            self.diags.push(
                Diagnostic::error("V1002", format!("`{}`与平台SDK中的类型同名", name.name))
                    .with_label(name.span, "请换一个名称")
                    .with_note("生成的Swift或Kotlin代码会直接使用这些平台类型"),
            );
            return;
        }
        if let Some((_, prev)) = self.globals.get(&name.name) {
            self.diags.push(
                Diagnostic::error("V1001", format!("重复声明`{}`", name.name))
                    .with_label(name.span, "重复的声明")
                    .with_secondary(*prev, "首次声明于此"),
            );
            return;
        }
        self.check_type_name(name);
        self.globals.insert(name.name.clone(), (global, name.span));
    }

    /// 按名称查找声明，并检查当前文件是否可见
    pub fn lookup(&mut self, name: &str, span: Span) -> Option<Global<'a>> {
        let (g, decl) = *self.globals.get(name)?;
        let file = decl.file;
        if file == self.current {
            return Some(g);
        }
        if self
            .visible
            .get(&self.current)
            .is_some_and(|v| v.contains(&file))
        {
            self.used_imports.insert((self.current, file));
            return Some(g);
        }
        let target = self.paths.get(&file).cloned().unwrap_or_default();
        let from = self.paths.get(&self.current).cloned().unwrap_or_default();
        self.diags.push(
            Diagnostic::error(
                "V1034",
                format!("`{name}`声明在{target}中，当前文件没有import它"),
            )
            .with_label(span, "")
            .with_secondary(decl, "声明于此")
            .with_note(format!(
                "在文件开头添加`import \"{}\"`",
                relative_path(&from, &target)
            )),
        );
        Some(g)
    }

    pub fn check_type_name(&mut self, name: &ast::Ident) {
        if !name.name.starts_with(|c: char| c.is_ascii_uppercase()) {
            self.error(
                "V1021",
                format!("类型名称`{}`应以大写字母开头", name.name),
                name.span,
                "",
            );
        }
    }

    pub fn check_member_name(&mut self, name: &ast::Ident) {
        if !name.name.starts_with(|c: char| c.is_ascii_lowercase()) {
            self.error(
                "V1022",
                format!("成员名称`{}`应以小写字母开头", name.name),
                name.span,
                "",
            );
        }
    }
}

/// 生成代码会用到的平台类型，用户声明不能与之同名
const PLATFORM_TYPES: &[&str] = &[
    "Any",
    "Array",
    "Binding",
    "Box",
    "Button",
    "Color",
    "Column",
    "Composable",
    "Container",
    "Context",
    "Data",
    "DataStore",
    "Dictionary",
    "Divider",
    "Environment",
    "Error",
    "Factory",
    "Flow",
    "Font",
    "ForEach",
    "Group",
    "Image",
    "Instant",
    "Label",
    "Modifier",
    "Never",
    "Nothing",
    "Observable",
    "Optional",
    "Pair",
    "Preferences",
    "Result",
    "Row",
    "Schema",
    "ScrollView",
    "Set",
    "Slider",
    "Spacer",
    "State",
    "Task",
    "Text",
    "Toggle",
    "Triple",
    "UUID",
    "Unit",
    "URL",
    "View",
    "ViewModel",
];

pub(crate) fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "Bool" | "Int" | "Double" | "String" | "Date" | "List" | "Stream" | "Action" | "Binding"
    )
}

/// 计算从`from`所在目录到`to`的相对路径，路径分隔符为`/`
pub(crate) fn relative_path(from: &str, to: &str) -> String {
    let from_dir: Vec<&str> = from.split('/').filter(|p| !p.is_empty()).collect();
    let from_dir = &from_dir[..from_dir.len().saturating_sub(1)];
    let target: Vec<&str> = to.split('/').filter(|p| !p.is_empty()).collect();
    let common = from_dir
        .iter()
        .zip(&target)
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<&str> = vec![".."; from_dir.len() - common];
    parts.extend(&target[common..]);
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::relative_path;

    #[test]
    fn computes_relative_paths() {
        assert_eq!(relative_path("ui/a.vey", "ui/b.vey"), "b.vey");
        assert_eq!(
            relative_path("ui/screens/a.vey", "ui/stores/b.vey"),
            "../stores/b.vey"
        );
        assert_eq!(relative_path("a.vey", "ui/b.vey"), "ui/b.vey");
    }
}
