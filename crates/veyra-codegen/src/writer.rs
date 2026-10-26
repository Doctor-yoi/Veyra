// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

/// 按行写入并自动缩进的代码缓冲区
#[derive(Clone, Debug)]
pub struct CodeWriter {
    buf: String,
    level: usize,
    unit: &'static str,
}

impl CodeWriter {
    pub fn new(unit: &'static str) -> Self {
        Self {
            buf: String::new(),
            level: 0,
            unit,
        }
    }

    /// 写入一行，多行文本会逐行缩进
    pub fn line(&mut self, text: impl AsRef<str>) {
        for l in text.as_ref().split('\n') {
            if l.is_empty() {
                self.buf.push('\n');
                continue;
            }
            for _ in 0..self.level {
                self.buf.push_str(self.unit);
            }
            self.buf.push_str(l);
            self.buf.push('\n');
        }
    }

    /// 写入空行，连续空行只保留一个
    pub fn blank(&mut self) {
        if !self.buf.is_empty() && !self.buf.ends_with("\n\n") {
            self.buf.push('\n');
        }
    }

    pub fn indent(&mut self) {
        self.level += 1;
    }

    pub fn dedent(&mut self) {
        self.level = self.level.saturating_sub(1);
    }

    /// 写入`open`，在缩进内执行`body`，再写入`close`
    pub fn block(
        &mut self,
        open: impl AsRef<str>,
        close: impl AsRef<str>,
        body: impl FnOnce(&mut Self),
    ) {
        self.line(open);
        self.indent();
        body(self);
        self.dedent();
        self.line(close);
    }

    pub fn finish(mut self) -> String {
        while self.buf.ends_with("\n\n") {
            self.buf.pop();
        }
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indents_blocks() {
        let mut w = CodeWriter::new("    ");
        w.block("a {", "}", |w| {
            w.line("b\nc");
            w.blank();
            w.blank();
            w.line("d");
        });
        assert_eq!(w.finish(), "a {\n    b\n    c\n\n    d\n}\n");
    }
}
