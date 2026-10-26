// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! Veyra翻译文件的同步与校验
//!
//! 翻译的唯一来源是`locales/<locale>.toml`，源语言文案来自`.vey`中的默认值
//! 每次构建时：
//!
//! - 新增的key写入空字符串占位，并附上源文案注释
//! - 已有翻译原样保留，包括开发者手写的注释与顺序
//! - 源码中已不存在的key报告为过时，由`veyra l10n prune`清理
//! - 校验翻译中的占位符与源文案一致
//!
//! 合并后的结果构成[`Catalog`]，交给各平台后端生成资源文件

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Deserialize;
use toml_edit::{DocumentMut, Item, Value, value};
use veyra_diagnostics::{Diagnostic, Diagnostics};
use veyra_ir::{
    Catalog, CatalogEntry, LocString, PLURAL_CATEGORIES, Segment, Translation, parse_template,
};

/// 缺失翻译的处理方式
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Missing {
    #[default]
    Warn,
    Error,
}

/// `[l10n]`配置段
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub source: String,
    pub targets: Vec<String>,
    pub dir: PathBuf,
    pub missing: Missing,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            source: "en".into(),
            targets: Vec::new(),
            dir: "locales".into(),
            missing: Missing::Warn,
        }
    }
}

const FILE_NOTICE: &str = "# Translations for Veyra, fill in the empty strings\n# Each comment above a key shows the source text\n";

/// 一个语言文件的同步结果
#[derive(Clone, Debug)]
pub struct LocaleFile {
    pub locale: String,
    pub path: PathBuf,
    /// 同步后的内容
    pub contents: String,
    /// 与磁盘上的内容是否不同
    pub changed: bool,
    pub stale: Vec<String>,
    pub missing: Vec<String>,
}

/// 同步结果
#[derive(Clone, Debug)]
pub struct Sync {
    pub catalog: Catalog,
    pub files: Vec<LocaleFile>,
    pub diagnostics: Diagnostics,
}

/// 合并各平台收集到的文案，同一key在不同平台的定义必须一致
pub fn merge_strings(lists: &[Vec<LocString>]) -> Result<Vec<LocString>, Diagnostics> {
    let mut map: IndexMap<String, LocString> = IndexMap::new();
    let mut diags = Diagnostics::new();
    for list in lists {
        for s in list {
            match map.get(&s.key) {
                Some(prev) if prev != s => diags.push(
                    Diagnostic::error(
                        "V4001",
                        format!("本地化key`{}`在不同平台的定义不一致", s.key),
                    )
                    .with_note("同一个key在所有平台必须使用相同的源文案与参数"),
                ),
                Some(_) => {}
                None => {
                    map.insert(s.key.clone(), s.clone());
                }
            }
        }
    }
    if diags.has_errors() {
        return Err(diags);
    }
    let mut out: Vec<_> = map.into_values().collect();
    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(out)
}

/// 读取并合并全部语言文件，`root`为项目根目录
pub fn sync(strings: &[LocString], config: &Config, root: &Path) -> Sync {
    let mut diags = Diagnostics::new();
    let mut files = Vec::new();
    let mut translations: IndexMap<String, IndexMap<String, Translation>> = IndexMap::new();
    for locale in &config.targets {
        if *locale == config.source {
            diags.push(Diagnostic::error(
                "V4030",
                format!("目标语言`{locale}`与源语言相同"),
            ));
            continue;
        }
        let rel = config.dir.join(format!("{locale}.toml"));
        let original = std::fs::read_to_string(root.join(&rel)).unwrap_or_default();
        let file = sync_file(
            locale,
            &rel,
            &original,
            strings,
            &mut translations,
            &mut diags,
        );
        if !file.missing.is_empty() {
            let shown = file
                .missing
                .iter()
                .take(5)
                .map(|k| format!("`{k}`"))
                .collect::<Vec<_>>()
                .join("、");
            let more = if file.missing.len() > 5 { "等" } else { "" };
            let message = format!("`{locale}`缺少{}条翻译：{shown}{more}", file.missing.len());
            let d = match config.missing {
                Missing::Warn => Diagnostic::warning("V4024", message),
                Missing::Error => Diagnostic::error("V4024", message),
            };
            diags.push(d.with_note(format!("在{}中填写，未翻译时回退到源语言", rel.display())));
        }
        if !file.stale.is_empty() {
            diags.push(
                Diagnostic::warning(
                    "V4020",
                    format!("`{}`中有{}条过时的翻译", rel.display(), file.stale.len()),
                )
                .with_note("运行`veyra l10n prune`删除源码中已不存在的key"),
            );
        }
        files.push(file);
    }
    let entries = strings
        .iter()
        .map(|s| CatalogEntry {
            string: s.clone(),
            translations: config
                .targets
                .iter()
                .filter_map(|l| {
                    translations
                        .get(l)
                        .and_then(|t| t.get(&s.key))
                        .map(|t| (l.clone(), t.clone()))
                })
                .collect(),
        })
        .collect();
    let catalog = Catalog {
        source_locale: config.source.clone(),
        target_locales: config.targets.clone(),
        entries,
    };
    Sync {
        catalog,
        files,
        diagnostics: diags,
    }
}

fn placeholder_names(text: &str) -> Result<BTreeSet<String>, String> {
    Ok(parse_template(text)?
        .into_iter()
        .filter_map(|s| match s {
            Segment::Arg(n) => Some(n),
            Segment::Text(_) => None,
        })
        .collect())
}

/// 检查一条翻译的占位符
fn check_text(
    locale: &str,
    s: &LocString,
    text: &str,
    path: &Path,
    diags: &mut Diagnostics,
) -> bool {
    let allowed: BTreeSet<String> = s.args.iter().map(|a| a.name.clone()).collect();
    match placeholder_names(text) {
        Err(msg) => {
            diags.push(
                Diagnostic::error("V4021", format!("`{locale}`中`{}`的翻译无效：{msg}", s.key))
                    .with_note(path.display().to_string()),
            );
            false
        }
        Ok(names) => {
            let unknown: Vec<_> = names.difference(&allowed).collect();
            if unknown.is_empty() {
                return true;
            }
            let list = unknown
                .iter()
                .map(|n| format!("{{{n}}}"))
                .collect::<Vec<_>>()
                .join("、");
            diags.push(
                Diagnostic::error(
                    "V4021",
                    format!("`{locale}`中`{}`的翻译包含源文案没有的占位符{list}", s.key),
                )
                .with_note(path.display().to_string()),
            );
            false
        }
    }
}

fn sync_file(
    locale: &str,
    rel: &Path,
    original: &str,
    strings: &[LocString],
    out: &mut IndexMap<String, IndexMap<String, Translation>>,
    diags: &mut Diagnostics,
) -> LocaleFile {
    let mut doc = match original.parse::<DocumentMut>() {
        Ok(d) => d,
        Err(e) => {
            diags.push(Diagnostic::error(
                "V4031",
                format!("无法解析{}：{}", rel.display(), e.message()),
            ));
            return LocaleFile {
                locale: locale.into(),
                path: rel.into(),
                contents: original.into(),
                changed: false,
                stale: Vec::new(),
                missing: Vec::new(),
            };
        }
    };
    if original.trim().is_empty() {
        doc.decor_mut().set_prefix(FILE_NOTICE);
    }
    let table = doc.as_table_mut();
    let mut found = IndexMap::new();
    let mut missing = Vec::new();
    for s in strings {
        let key = s.key.as_str();
        if !table.contains_key(key) {
            let item = match &s.plural {
                Some(_) => {
                    let mut t = toml_edit::InlineTable::new();
                    t.insert("other", "".into());
                    Item::Value(Value::InlineTable(t))
                }
                None => value(""),
            };
            table.insert(key, item);
            if let Some(mut k) = table.key_mut(key) {
                let comment = s.source.replace('\n', "\\n");
                k.leaf_decor_mut().set_prefix(format!("\n# {comment}\n"));
            }
        }
        let translation = match table.get(key) {
            Some(Item::Value(Value::String(v))) => {
                let text = v.value().clone();
                if s.plural.is_some() {
                    diags.push(Diagnostic::error("V4022", format!("`{locale}`中`{key}`是复数文案，需要写成{{ one = \"...\", other = \"...\" }}")));
                    None
                } else if text.is_empty() {
                    None
                } else if check_text(locale, s, &text, rel, diags) {
                    Some(Translation::Text(text))
                } else {
                    None
                }
            }
            Some(item) if item.is_inline_table() || item.is_table() => {
                let mut forms = IndexMap::new();
                if let Some(t) = item.as_table_like() {
                    for (cat, v) in t.iter() {
                        if !PLURAL_CATEGORIES.contains(&cat) {
                            diags.push(Diagnostic::error(
                                "V4022",
                                format!("`{locale}`中`{key}`包含未知的复数类别`{cat}`"),
                            ));
                            continue;
                        }
                        if let Some(text) = v.as_str()
                            && !text.is_empty()
                            && check_text(locale, s, text, rel, diags)
                        {
                            forms.insert(cat.to_string(), text.to_string());
                        }
                    }
                }
                if s.plural.is_none() {
                    diags.push(Diagnostic::error(
                        "V4022",
                        format!("`{locale}`中`{key}`不是复数文案，翻译应为字符串"),
                    ));
                    None
                } else if !forms.contains_key("other") {
                    None
                } else {
                    Some(Translation::Plural(forms))
                }
            }
            _ => {
                diags.push(Diagnostic::error(
                    "V4022",
                    format!("`{locale}`中`{key}`的翻译必须是字符串"),
                ));
                None
            }
        };
        match translation {
            Some(t) => {
                found.insert(key.to_string(), t);
            }
            None => missing.push(key.to_string()),
        }
    }
    let known: BTreeSet<&str> = strings.iter().map(|s| s.key.as_str()).collect();
    let stale: Vec<String> = table
        .iter()
        .map(|(k, _)| k.to_string())
        .filter(|k| !known.contains(k.as_str()))
        .collect();
    out.insert(locale.to_string(), found);
    let contents = doc.to_string();
    LocaleFile {
        locale: locale.into(),
        path: rel.into(),
        changed: contents != original,
        contents,
        stale,
        missing,
    }
}

/// 删除过时的key，返回新内容
pub fn prune(file: &LocaleFile) -> String {
    let Ok(mut doc) = file.contents.parse::<DocumentMut>() else {
        return file.contents.clone();
    };
    for key in &file.stale {
        doc.as_table_mut().remove(key);
    }
    doc.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use veyra_ir::{LocArg, LocArgType};

    fn s(key: &str, source: &str, args: &[&str]) -> LocString {
        LocString {
            key: key.into(),
            source: source.into(),
            args: args
                .iter()
                .map(|a| LocArg {
                    name: a.to_string(),
                    ty: LocArgType::Int,
                })
                .collect(),
            plural: None,
        }
    }

    #[test]
    fn inserts_new_keys_and_keeps_translations() {
        let strings = vec![
            s("home.count", "共{count}项", &["count"]),
            s("home.title", "首页", &[]),
        ];
        let original = "# 手写注释\n\"home.title\" = \"Home\"\n\"old.key\" = \"Old\"\n";
        let mut out = IndexMap::new();
        let mut diags = Diagnostics::new();
        let file = sync_file(
            "en",
            Path::new("locales/en.toml"),
            original,
            &strings,
            &mut out,
            &mut diags,
        );
        assert!(diags.is_empty());
        assert_eq!(
            file.contents,
            "# 手写注释\n\"home.title\" = \"Home\"\n\"old.key\" = \"Old\"\n\n# 共{count}项\n\"home.count\" = \"\"\n"
        );
        assert_eq!(file.missing, vec!["home.count"]);
        assert_eq!(file.stale, vec!["old.key"]);
        assert_eq!(out["en"]["home.title"], Translation::Text("Home".into()));
        assert_eq!(
            prune(&file),
            "# 手写注释\n\"home.title\" = \"Home\"\n\n# 共{count}项\n\"home.count\" = \"\"\n"
        );
    }

    #[test]
    fn rejects_unknown_placeholders() {
        let strings = vec![s("a.b", "{count}", &["count"])];
        let mut out = IndexMap::new();
        let mut diags = Diagnostics::new();
        sync_file(
            "en",
            Path::new("en.toml"),
            "\"a.b\" = \"{n}\"\n",
            &strings,
            &mut out,
            &mut diags,
        );
        assert!(diags.has_errors());
    }
}
