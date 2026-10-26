// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 逐个检查tests/ui下的.vey文件，并对诊断输出做快照

use std::fs;
use std::path::Path;

use veyra_diagnostics::{Diagnostics, RenderOptions, SourceMap, render_all};
use veyra_ir::Platform;
use veyra_sema::{Target, analyze};

fn check(path: &Path, platform: Platform) -> String {
    let src = fs::read_to_string(path).unwrap();
    let mut map = SourceMap::new();
    let name = path.file_name().unwrap().to_string_lossy().to_string();
    let id = map.add(&name, src.as_str());
    let parsed = veyra_syntax::parse(id, &src);
    let mut diags = Diagnostics::new();
    diags.extend(parsed.diagnostics);
    let mut target = Target::new(platform);
    target.flags.insert("premium".into(), false);
    let analysis = analyze(&[parsed.file], &target);
    diags.extend(analysis.diagnostics);
    let out = render_all(&diags.sorted(), &map, RenderOptions::default());
    if out.is_empty() {
        "无诊断\n".to_string()
    } else {
        out
    }
}

#[test]
fn ui() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui");
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "vey"))
    {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let ios = check(&path, Platform::Ios);
        let android = check(&path, Platform::Android);
        let out = if ios == android {
            ios
        } else {
            format!("[ios]\n{ios}\n[android]\n{android}")
        };
        insta::assert_snapshot!(stem, out);
    }
}
