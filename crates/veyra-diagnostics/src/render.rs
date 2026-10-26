// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

use std::fmt::Write;

use unicode_width::UnicodeWidthStr;

use crate::{Diagnostic, Diagnostics, Severity, SourceMap};

/// 渲染选项。
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderOptions {
    pub color: bool,
}

const RED: &str = "\x1b[1;31m";
const YELLOW: &str = "\x1b[1;33m";
const BLUE: &str = "\x1b[1;34m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

struct Palette {
    on: bool,
}

impl Palette {
    fn paint(&self, style: &str, text: &str) -> String {
        if self.on { format!("{style}{text}{RESET}") } else { text.to_string() }
    }
}

/// 渲染单条诊断。
pub fn render(diagnostic: &Diagnostic, sources: &SourceMap, options: RenderOptions) -> String {
    let p = Palette { on: options.color };
    let mut out = String::new();
    let (title, style) = match diagnostic.severity {
        Severity::Error => ("error", RED),
        Severity::Warning => ("warning", YELLOW),
        Severity::Note => ("note", BLUE),
    };
    let _ = writeln!(
        out,
        "{}{}",
        p.paint(style, &format!("{title}[{}]", diagnostic.code)),
        p.paint(BOLD, &format!(": {}", diagnostic.message))
    );

    // 计算行号栏宽度
    let mut gutter = 1;
    for label in &diagnostic.labels {
        let file = sources.get(label.span.file);
        let (line, _) = file.line_of(label.span.start as usize);
        gutter = gutter.max((line + 1).to_string().len());
    }
    let pad = " ".repeat(gutter);

    let mut labels: Vec<_> = diagnostic.labels.iter().collect();
    labels.sort_by_key(|l| (!l.primary, l.span.file.0, l.span.start));
    let mut last_file = None;
    for label in labels {
        let file = sources.get(label.span.file);
        let start = label.span.start as usize;
        let end = (label.span.end as usize).max(start);
        let (line, line_start) = file.line_of(start);
        let text = file.line_text(line);
        let col = start - line_start;
        if last_file != Some(label.span.file) {
            let col_chars = text.get(..col).map(|s| s.chars().count()).unwrap_or(col);
            let _ = writeln!(
                out,
                "{pad}{} {}:{}:{}",
                p.paint(BLUE, "-->"),
                file.path.display(),
                line + 1,
                col_chars + 1
            );
            let _ = writeln!(out, "{pad} {}", p.paint(BLUE, "|"));
            last_file = Some(label.span.file);
        }
        let line_no = format!("{:>gutter$}", line + 1);
        let _ = writeln!(out, "{} {}", p.paint(BLUE, &format!("{line_no} |")), text);

        // 标注只画在第一行内
        let prefix_width = text.get(..col).map(UnicodeWidthStr::width).unwrap_or(col);
        let line_end = line_start + text.len();
        let mark_end = end.min(line_end).max(start + 1);
        let marked = file.text.get(start..mark_end.min(file.text.len())).unwrap_or(" ");
        let mark_width = UnicodeWidthStr::width(marked).max(1);
        let mark = if label.primary { "^" } else { "-" }.repeat(mark_width);
        let style = if label.primary { style } else { BLUE };
        let mut annotation = mark;
        if !label.message.is_empty() {
            annotation.push(' ');
            annotation.push_str(&label.message);
        }
        let _ = writeln!(
            out,
            "{pad} {} {}{}",
            p.paint(BLUE, "|"),
            " ".repeat(prefix_width),
            p.paint(style, &annotation)
        );
    }
    if !diagnostic.labels.is_empty() && !diagnostic.notes.is_empty() {
        let _ = writeln!(out, "{pad} {}", p.paint(BLUE, "|"));
    }
    for note in &diagnostic.notes {
        let _ = writeln!(out, "{pad} {} note: {note}", p.paint(BLUE, "="));
    }
    out
}

/// 渲染全部诊断，并附加汇总行。
pub fn render_all(diagnostics: &Diagnostics, sources: &SourceMap, options: RenderOptions) -> String {
    let mut out = String::new();
    for diagnostic in diagnostics {
        out.push_str(&render(diagnostic, sources, options));
        out.push('\n');
    }
    let errors = diagnostics.error_count();
    let warnings = diagnostics.warning_count();
    let p = Palette { on: options.color };
    if errors > 0 {
        let _ = writeln!(out, "{}", p.paint(RED, &format!("error: 共{errors}个错误，{warnings}个警告")));
    } else if warnings > 0 {
        let _ = writeln!(out, "{}", p.paint(YELLOW, &format!("warning: 共{warnings}个警告")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Span;

    #[test]
    fn renders_label_under_cjk_text() {
        let mut map = SourceMap::new();
        let id = map.add("a.vey", "Text(\"你好\") bind(x)\n");
        let d = Diagnostic::error("V2001", "类型不匹配")
            .with_label(Span::new(id, 20, 21), "这里")
            .with_note("提示");
        let text = render(&d, &map, RenderOptions::default());
        let expected = "error[V2001]: 类型不匹配\n --> a.vey:1:17\n  |\n1 | Text(\"你好\") bind(x)\n  |                   ^ 这里\n  |\n  = note: 提示\n";
        assert_eq!(text, expected);
    }
}
