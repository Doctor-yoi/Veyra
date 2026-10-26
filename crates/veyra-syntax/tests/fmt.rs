// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

use std::path::Path;

use veyra_syntax::{format, parse};

fn fmt(src: &str) -> String {
    let parsed = parse(Default::default(), src);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    format(&parsed, src)
}

/// 递归收集仓库中的全部.vey文件
fn collect(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "vey") {
            out.push(path);
        }
    }
}

#[test]
fn formatting_is_idempotent_and_keeps_comments() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    collect(&root.join("examples"), &mut files);
    collect(&root.join("crates/veyra-backend-swiftui/tests"), &mut files);
    for path in files {
        let src = std::fs::read_to_string(&path).unwrap();
        let once = fmt(&src);
        let twice = fmt(&once);
        assert_eq!(once, twice, "{}", path.display());
        let count = |s: &str| parse(Default::default(), s).comments.len();
        assert_eq!(count(&src), count(&once), "注释丢失：{}", path.display());
    }
}

#[test]
fn formats_messy_source() {
    insta::assert_snapshot!(fmt(r#"import "b.vey"
import "a.vey"
// 枚举
enum   Kind{a,b}
model M{x:Int  // 行尾注释
y : List<String>?}



screen S{
inject r:Repo
state{q:String="" @writable
n:Int=1}
actions{go( id:String )async}
lifecycle{onAppear:go("x")}
view{Column(spacing:8){Text(loc("a.b","x")).font(.title)
#if ios
Text("i")
#else
Text("o")
#endif
if q.isEmpty{Spacer()}else if n>1{Divider()}else{Text(q)}
List(items,key:.id){item in Text(item.name).padding().background(.red)
}
/* 块注释 */
}.padding(16)}
}
"#));
}
