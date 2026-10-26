// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 本地化中间表示。文案模板使用`{name}`占位，`\{`表示字面量花括号

use indexmap::IndexMap;

/// CLDR复数类别
pub const PLURAL_CATEGORIES: [&str; 6] = ["zero", "one", "two", "few", "many", "other"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LocArgType {
    String,
    Int,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LocArg {
    pub name: String,
    pub ty: LocArgType,
}

/// 复数形式，键为复数类别
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluralForms {
    pub forms: IndexMap<String, String>,
}

/// 源码中声明的一条文案
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocString {
    pub key: String,
    /// 源语言文案；复数文案取`other`形式
    pub source: String,
    pub args: Vec<LocArg>,
    pub plural: Option<PluralForms>,
}

/// 某个语言的翻译
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Translation {
    Text(String),
    Plural(IndexMap<String, String>),
}

/// 合并翻译后的单条文案
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogEntry {
    pub string: LocString,
    /// 键为语言标识，缺失表示未翻译
    pub translations: IndexMap<String, Translation>,
}

/// 全部文案与翻译，是各平台资源文件的唯一来源
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    pub source_locale: String,
    pub target_locales: Vec<String>,
    pub entries: Vec<CatalogEntry>,
}

impl Catalog {
    pub fn get(&self, key: &str) -> Option<&CatalogEntry> {
        self.entries.iter().find(|e| e.string.key == key)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    Text(String),
    Arg(String),
}

/// 拆分文案模板
pub fn parse_template(text: &str) -> Result<Vec<Segment>, String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if matches!(chars.peek(), Some('{') | Some('}')) => {
                buf.push(chars.next().unwrap_or_default());
            }
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(ch) if ch == '_' || ch.is_ascii_alphanumeric() => name.push(ch),
                        Some(ch) => return Err(format!("占位符中不能包含`{ch}`")),
                        None => return Err("占位符缺少`}`".to_string()),
                    }
                }
                if name.is_empty() {
                    return Err("占位符名称为空".to_string());
                }
                if !buf.is_empty() {
                    out.push(Segment::Text(std::mem::take(&mut buf)));
                }
                out.push(Segment::Arg(name));
            }
            '}' => return Err("多余的`}`，字面量请写作`\\}`".to_string()),
            _ => buf.push(c),
        }
    }
    if !buf.is_empty() {
        out.push(Segment::Text(buf));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_template() {
        assert_eq!(
            parse_template("共{count}门\\{课\\}").unwrap(),
            vec![
                Segment::Text("共".into()),
                Segment::Arg("count".into()),
                Segment::Text("门{课}".into())
            ]
        );
        assert!(parse_template("a{b").is_err());
        assert!(parse_template("a}").is_err());
    }
}
