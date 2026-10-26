// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! `L10n`访问器与String Catalog。

use indexmap::IndexMap;
use serde_json::{Map, Value, json};
use veyra_codegen::CodeWriter;
use veyra_diagnostics::{Diagnostic, Diagnostics};
use veyra_ir::{
    Catalog, CatalogEntry, LocArgType, LocString, Segment, Translation, parse_template,
};

use crate::header;
use crate::swift::{self, l10n_name};

/// 检查key转换后的访问器名称是否冲突。
pub(crate) fn check_names(catalog: &Catalog) -> Result<IndexMap<String, String>, Diagnostics> {
    let mut seen: IndexMap<String, String> = IndexMap::new();
    let mut diags = Diagnostics::new();
    for e in &catalog.entries {
        let name = l10n_name(&e.string.key);
        if let Some(prev) = seen.get(&name) {
            diags.push(Diagnostic::error(
                "V4010",
                format!(
                    "本地化key`{prev}`与`{}`在Swift中都映射为`L10n.{name}`",
                    e.string.key
                ),
            ));
        }
        seen.insert(name, e.string.key.clone());
    }
    if diags.has_errors() {
        Err(diags)
    } else {
        Ok(seen.into_iter().map(|(k, v)| (v, k)).collect())
    }
}

fn arg_type(t: LocArgType) -> &'static str {
    match t {
        LocArgType::Int => "Int",
        LocArgType::String => "String",
    }
}

/// 生成`String(localized:defaultValue:)`的插值字面量。
fn interpolation(s: &LocString) -> String {
    let segments =
        parse_template(&s.source).unwrap_or_else(|_| vec![Segment::Text(s.source.clone())]);
    let mut out = String::from("\"");
    for seg in segments {
        match seg {
            Segment::Text(t) => {
                let lit = swift::string_literal(&t);
                out.push_str(&lit[1..lit.len() - 1]);
            }
            Segment::Arg(n) => out.push_str(&format!("\\({})", swift::ident(&n))),
        }
    }
    out.push('"');
    out
}

pub(crate) fn accessors(
    catalog: &Catalog,
    names: &IndexMap<String, String>,
    table: &str,
) -> String {
    let mut w = CodeWriter::new("    ");
    header(&mut w, &["Foundation"]);
    w.line("/// 类型安全的本地化文案，翻译存放在String Catalog中。");
    w.block("enum L10n {", "}", |w| {
        for (i, e) in catalog.entries.iter().enumerate() {
            if i > 0 {
                w.blank();
            }
            let s = &e.string;
            let name = &names[&s.key];
            w.line(format!("/// {}", s.source.replace('\n', " ")));
            let body = format!(
                "String(localized: {}, defaultValue: {}, table: {})",
                swift::string_literal(&s.key),
                interpolation(s),
                swift::string_literal(table)
            );
            if s.args.is_empty() {
                w.block(format!("static var {name}: String {{"), "}", |w| {
                    w.line(&body)
                });
            } else {
                let ps: Vec<_> = s
                    .args
                    .iter()
                    .map(|a| format!("{}: {}", swift::ident(&a.name), arg_type(a.ty)))
                    .collect();
                w.block(
                    format!("static func {name}({}) -> String {{", ps.join(", ")),
                    "}",
                    |w| w.line(&body),
                );
            }
        }
    });
    w.finish()
}

/// 把`{name}`模板转换为printf风格的格式串。
pub(crate) fn format_string(text: &str, s: &LocString) -> String {
    let Ok(segments) = parse_template(text) else {
        return text.to_string();
    };
    let positional = s.args.len() > 1;
    let mut out = String::new();
    for seg in segments {
        match seg {
            Segment::Text(t) => {
                if s.args.is_empty() {
                    out.push_str(&t);
                } else {
                    out.push_str(&t.replace('%', "%%"));
                }
            }
            Segment::Arg(n) => {
                let Some(index) = s.args.iter().position(|a| a.name == n) else {
                    out.push_str(&format!("{{{n}}}"));
                    continue;
                };
                let spec = match s.args[index].ty {
                    LocArgType::Int => "lld",
                    LocArgType::String => "@",
                };
                if positional {
                    out.push_str(&format!("%{}${spec}", index + 1));
                } else {
                    out.push_str(&format!("%{spec}"));
                }
            }
        }
    }
    out
}

fn unit(state: &str, value: String) -> Value {
    json!({ "stringUnit": { "state": state, "value": value } })
}

fn entry(catalog: &Catalog, e: &CatalogEntry) -> Value {
    let s = &e.string;
    let mut locs = Map::new();
    locs.insert(
        catalog.source_locale.clone(),
        unit("translated", format_string(&s.source, s)),
    );
    for locale in &catalog.target_locales {
        let value = match e.translations.get(locale) {
            Some(Translation::Text(t)) => unit("translated", format_string(t, s)),
            _ => unit("new", format_string(&s.source, s)),
        };
        locs.insert(locale.clone(), value);
    }
    json!({ "extractionState": "manual", "localizations": locs })
}

pub(crate) fn xcstrings(catalog: &Catalog) -> String {
    let mut strings = Map::new();
    for e in &catalog.entries {
        strings.insert(e.string.key.clone(), entry(catalog, e));
    }
    let root =
        json!({ "sourceLanguage": catalog.source_locale, "strings": strings, "version": "1.0" });
    let mut out = serde_json::to_string_pretty(&root).unwrap_or_default();
    out.push('\n');
    out
}
