// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! Android字符串资源

use std::collections::BTreeMap;
use std::path::Path;

use veyra_codegen::{GENERATED_NOTICE, OutputFile};
use veyra_diagnostics::{Diagnostic, Diagnostics};
use veyra_ir::{Catalog, LocArgType, LocString, Segment, Translation, parse_template};

use crate::kotlin::resource_name;

/// BCP 47语言标识转为资源目录限定符，例如`zh-Hant`转为`b+zh+Hant`
pub(crate) fn qualifier(locale: &str) -> String {
    let parts: Vec<&str> = locale.split(['-', '_']).collect();
    match parts.as_slice() {
        [lang] => lang.to_lowercase(),
        [lang, region] if region.len() == 2 && region.chars().all(|c| c.is_ascii_alphabetic()) => {
            format!("{}-r{}", lang.to_lowercase(), region.to_uppercase())
        }
        _ => format!("b+{}", parts.join("+")),
    }
}

fn escape(text: &str) -> String {
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\'' => out.push_str("\\'"),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '@' | '?' if i == 0 => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}

/// 把`{name}`模板转换为Android的位置格式串
pub(crate) fn format_string(text: &str, s: &LocString) -> String {
    let Ok(segments) = parse_template(text) else {
        return escape(text);
    };
    let mut out = String::new();
    for seg in segments {
        match seg {
            Segment::Text(t) => {
                let t = if s.args.is_empty() {
                    t
                } else {
                    t.replace('%', "%%")
                };
                out.push_str(&escape(&t));
            }
            Segment::Arg(n) => match s.args.iter().position(|a| a.name == n) {
                Some(i) => {
                    let spec = match s.args[i].ty {
                        LocArgType::Int => "d",
                        LocArgType::String => "s",
                    };
                    out.push_str(&format!("%{}${spec}", i + 1));
                }
                None => out.push_str(&escape(&format!("{{{n}}}"))),
            },
        }
    }
    out
}

fn string(s: &LocString, text: &str) -> String {
    format!(
        "<string name=\"{}\">{}</string>",
        resource_name(&s.key),
        format_string(text, s)
    )
}

fn plurals(s: &LocString, forms: &indexmap::IndexMap<String, String>) -> String {
    let mut out = format!("<plurals name=\"{}\">", resource_name(&s.key));
    for (cat, text) in forms {
        out.push_str(&format!(
            "\n        <item quantity=\"{cat}\">{}</item>",
            format_string(text, s)
        ));
    }
    out.push_str("\n    </plurals>");
    out
}

fn document(lines: &[String], ignore_missing: bool) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    out.push_str(&format!("<!-- {GENERATED_NOTICE} -->\n"));
    if ignore_missing {
        out.push_str("<resources xmlns:tools=\"http://schemas.android.com/tools\" tools:ignore=\"MissingTranslation\">\n");
    } else {
        out.push_str("<resources>\n");
    }
    for l in lines {
        out.push_str("    ");
        out.push_str(l);
        out.push('\n');
    }
    out.push_str("</resources>\n");
    out
}

/// 生成默认语言与各目标语言的`veyra_strings.xml`
pub(crate) fn strings(catalog: &Catalog, res: &Path) -> Result<Vec<OutputFile>, Diagnostics> {
    let mut names: BTreeMap<String, &str> = BTreeMap::new();
    let mut diags = Diagnostics::new();
    for e in &catalog.entries {
        let name = resource_name(&e.string.key);
        if let Some(prev) = names.insert(name.clone(), &e.string.key) {
            diags.push(Diagnostic::error(
                "V4010",
                format!(
                    "本地化key`{prev}`与`{}`在Android中都映射为`R.string.{name}`",
                    e.string.key
                ),
            ));
        }
    }
    if diags.has_errors() {
        return Err(diags);
    }
    let file = "veyra_strings.xml";
    let mut out = Vec::new();
    let source: Vec<String> = catalog
        .entries
        .iter()
        .map(|e| match &e.string.plural {
            Some(p) => plurals(&e.string, &p.forms),
            None => string(&e.string, &e.string.source),
        })
        .collect();
    out.push(OutputFile::generated(
        res.join("values").join(file),
        document(&source, true),
    ));
    for locale in &catalog.target_locales {
        let lines: Vec<String> = catalog
            .entries
            .iter()
            .filter_map(|e| match e.translations.get(locale) {
                Some(Translation::Text(t)) => Some(string(&e.string, t)),
                Some(Translation::Plural(forms)) => Some(plurals(&e.string, forms)),
                None => None,
            })
            .collect();
        out.push(OutputFile::generated(
            res.join(format!("values-{}", qualifier(locale))).join(file),
            document(&lines, false),
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_locale_qualifiers() {
        assert_eq!(qualifier("en"), "en");
        assert_eq!(qualifier("pt-BR"), "pt-rBR");
        assert_eq!(qualifier("zh-Hant"), "b+zh+Hant");
        assert_eq!(qualifier("zh-Hans-CN"), "b+zh+Hans+CN");
        assert_eq!(escape("@It's <b>"), "\\@It\\'s &lt;b&gt;");
    }
}
