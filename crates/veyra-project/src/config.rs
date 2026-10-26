// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Deserialize;
use veyra_diagnostics::Diagnostic;

pub const CONFIG_FILE: &str = "veyra.toml";

/// 已知的顶层配置段，后端段由注册表决定
const KNOWN_SECTIONS: &[&str] = &["project", "l10n", "flags", "lint", "storage"];

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSection {
    pub name: String,
    #[serde(default = "default_sources")]
    pub sources: Vec<PathBuf>,
}

fn default_sources() -> Vec<PathBuf> {
    vec!["ui".into()]
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LintLevel {
    Allow,
    #[default]
    Warn,
    Deny,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Lint {
    /// native代码块
    pub native: LintLevel,
}

/// 解析后的`veyra.toml`
#[derive(Clone, Debug)]
pub struct Config {
    pub project: ProjectSection,
    pub l10n: veyra_l10n::Config,
    pub flags: IndexMap<String, bool>,
    pub lint: Lint,
    /// 后端配置段，键为后端名称
    pub backends: IndexMap<String, toml::Table>,
}

impl Config {
    /// 解析配置，`backend_names`为已注册的后端
    pub fn parse(text: &str, backend_names: &[&str]) -> Result<Self, Diagnostic> {
        let invalid = |e: String| Diagnostic::error("V9002", format!("{CONFIG_FILE}无效：{e}"));
        let table: toml::Table =
            toml::from_str(text).map_err(|e| invalid(e.message().to_string()))?;
        let section = |name: &str| table.get(name).cloned();
        let project: ProjectSection = match section("project") {
            Some(v) => v
                .try_into()
                .map_err(|e: toml::de::Error| invalid(format!("[project] {}", e.message())))?,
            None => return Err(invalid("缺少[project]段".into())),
        };
        let l10n = match section("l10n") {
            Some(v) => v
                .try_into()
                .map_err(|e: toml::de::Error| invalid(format!("[l10n] {}", e.message())))?,
            None => veyra_l10n::Config::default(),
        };
        let flags: IndexMap<String, bool> = match section("flags") {
            Some(v) => v
                .try_into()
                .map_err(|e: toml::de::Error| invalid(format!("[flags] {}", e.message())))?,
            None => IndexMap::new(),
        };
        let lint = match section("lint") {
            Some(v) => v
                .try_into()
                .map_err(|e: toml::de::Error| invalid(format!("[lint] {}", e.message())))?,
            None => Lint::default(),
        };
        for name in flags.keys() {
            if matches!(name.as_str(), "ios" | "android" | "harmony" | "debug") {
                return Err(invalid(format!("[flags]中的`{name}`是保留的条件名称")));
            }
        }
        let mut backends = IndexMap::new();
        for (key, value) in &table {
            if KNOWN_SECTIONS.contains(&key.as_str()) {
                continue;
            }
            if !backend_names.contains(&key.as_str()) {
                let mut all: Vec<&str> = KNOWN_SECTIONS.to_vec();
                all.extend(backend_names);
                return Err(invalid(format!(
                    "未知的配置段[{key}]，可用的配置段：{}",
                    all.join("、")
                )));
            }
            match value {
                toml::Value::Table(t) => {
                    backends.insert(key.clone(), t.clone());
                }
                _ => return Err(invalid(format!("[{key}]必须是表"))),
            }
        }
        Ok(Self {
            project,
            l10n,
            flags,
            lint,
            backends,
        })
    }
}

/// 一个Veyra项目
#[derive(Clone, Debug)]
pub struct Project {
    pub root: PathBuf,
    pub config: Config,
}

impl Project {
    /// 从`dir`向上查找`veyra.toml`
    pub fn discover(dir: &Path, backend_names: &[&str]) -> Result<Self, Diagnostic> {
        let start = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let mut cur = Some(start.as_path());
        while let Some(d) = cur {
            if d.join(CONFIG_FILE).is_file() {
                return Self::load(d, backend_names);
            }
            cur = d.parent();
        }
        Err(Diagnostic::error(
            "V9003",
            format!("在{}及其上级目录中找不到{CONFIG_FILE}", dir.display()),
        )
        .with_note("使用`veyra init`创建项目"))
    }

    pub fn load(root: &Path, backend_names: &[&str]) -> Result<Self, Diagnostic> {
        let path = root.join(CONFIG_FILE);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Diagnostic::error("V9003", format!("无法读取{}：{e}", path.display())))?;
        let config = Config::parse(&text, backend_names)?;
        Ok(Self {
            root: root.to_path_buf(),
            config,
        })
    }

    /// 按路径排序的全部`.vey`文件，返回相对于项目根目录的路径
    pub fn source_files(&self) -> Result<Vec<PathBuf>, Diagnostic> {
        let mut out = Vec::new();
        for dir in &self.config.project.sources {
            let abs = self.root.join(dir);
            if abs.is_file() {
                out.push(dir.clone());
                continue;
            }
            if !abs.is_dir() {
                return Err(Diagnostic::error(
                    "V9004",
                    format!("源码目录{}不存在", dir.display()),
                ));
            }
            collect(&self.root, &abs, &mut out).map_err(|e| {
                Diagnostic::error("V9004", format!("无法读取{}：{e}", abs.display()))
            })?;
        }
        out.sort();
        out.dedup();
        Ok(out)
    }
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(root, &path, out)?;
        } else if path.extension().is_some_and(|e| e == "vey") {
            out.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_config() {
        let c = Config::parse(
            "[project]\nname = \"App\"\n[flags]\npremium = true\n[swiftui]\nout = \"ios\"\n",
            &["swiftui", "compose"],
        )
        .unwrap();
        assert_eq!(c.project.sources, vec![PathBuf::from("ui")]);
        assert!(c.flags["premium"]);
        assert!(c.backends.contains_key("swiftui"));
        assert!(Config::parse("[project]\nname = \"A\"\n[arkui]\n", &["swiftui"]).is_err());
        assert!(Config::parse("[project]\nname = \"A\"\n[flags]\nios = true\n", &[]).is_err());
    }
}
