// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 命令行端到端测试，在临时目录中创建项目并运行veyra

use std::path::{Path, PathBuf};
use std::process::Command;

struct TempProject {
    root: PathBuf,
}

impl TempProject {
    fn new(name: &str, files: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!("veyra-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (path, text) in files {
            let p = root.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        Self { root }
    }

    fn run(&self, args: &[&str]) -> (bool, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_veyra"))
            .arg("-C")
            .arg(&self.root)
            .arg("--color")
            .arg("never")
            .args(args)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stderr).to_string()
            + &String::from_utf8_lossy(&out.stdout);
        (out.status.success(), text)
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

const CONFIG: &str =
    "[project]\nname = \"Demo\"\n\n[swiftui]\n\n[compose]\npackage = \"com.example.demo\"\n";

const STORE: &str = "entity Item @persistent {\n  id: String @primaryKey\n  name: String\n}\n\nstore ItemStore for Item {}\n";

fn screen(imports: &str) -> String {
    format!(
        "{imports}\nscreen Items {{\n  inject items: ItemStore\n  state {{\n    list: List<Item> = []\n  }}\n  view {{\n    List(list, key: .id) {{ item in\n      Text(item.name)\n    }}\n  }}\n}}\n"
    )
}

#[test]
fn requires_import_for_other_files() {
    let p = TempProject::new(
        "missing",
        &[
            ("veyra.toml", CONFIG),
            ("ui/stores/ItemStore.vey", STORE),
            ("ui/Items.vey", &screen("")),
        ],
    );
    let (ok, out) = p.run(&["check"]);
    assert!(!ok);
    assert!(
        out.contains(
            "error[V1034]: `ItemStore`声明在ui/stores/ItemStore.vey中，当前文件没有import它"
        ),
        "{out}"
    );
    assert!(out.contains("import \"stores/ItemStore.vey\""), "{out}");
}

#[test]
fn accepts_grouped_imports_and_loads_files_outside_sources() {
    let shared = "model Unused { a: Int }\n";
    let p = TempProject::new(
        "grouped",
        &[
            ("veyra.toml", CONFIG),
            ("shared/ItemStore.vey", STORE),
            ("shared/Unused.vey", shared),
            (
                "ui/Items.vey",
                &screen("import (\n  \"../shared/ItemStore.vey\"\n  \"../shared/Unused.vey\"\n)\n"),
            ),
        ],
    );
    let (ok, out) = p.run(&["build"]);
    assert!(ok, "{out}");
    assert!(
        out.contains("warning[V1033]: 未使用的import`../shared/Unused.vey`"),
        "{out}"
    );
    assert!(
        p.path("ios/Generated/Storage/ItemStore.generated.swift")
            .is_file()
    );
    assert!(
        p.path("android/app/src/main/java/com/example/demo/ItemStore.generated.kt")
            .is_file()
    );
}

#[test]
fn reports_bad_import_paths() {
    let p = TempProject::new(
        "bad",
        &[
            ("veyra.toml", CONFIG),
            (
                "ui/Items.vey",
                &screen(
                    "import \"nope.vey\"\nimport \"../../x.vey\"\nimport \"Items.vey\"\nimport \"a.txt\"\n",
                ),
            ),
        ],
    );
    let (ok, out) = p.run(&["check"]);
    assert!(!ok);
    for code in [
        "找不到import的文件ui/nope.vey",
        "import的路径越出了项目目录",
        "文件不能import自身",
        "import的路径必须以.vey结尾",
    ] {
        assert!(out.contains(code), "{code}\n{out}");
    }
}

#[test]
fn keeps_user_owned_files() {
    let p = TempProject::new(
        "owned",
        &[
            ("veyra.toml", CONFIG),
            ("ui/stores/ItemStore.vey", STORE),
            ("ui/Items.vey", &screen("import \"stores/ItemStore.vey\"")),
        ],
    );
    let (ok, out) = p.run(&["build"]);
    assert!(ok, "{out}");
    let vm = p.path("ios/ViewModels/ItemsViewModel.swift");
    std::fs::write(&vm, "// custom").unwrap();
    let (ok, out) = p.run(&["build"]);
    assert!(ok, "{out}");
    assert_eq!(std::fs::read_to_string(&vm).unwrap(), "// custom");
    assert!(Path::new(&p.path(".veyra/manifest.json")).is_file());
}

#[test]
fn rejects_declarations_split_by_apple_system() {
    let config = "[project]\nname = \"Demo\"\n\n[swiftui]\nplatforms = [\"ios\", \"macos\"]\n\n[compose]\npackage = \"com.example.demo\"\n";
    let ui = "screen Home {\n  state {\n    #if ios\n    title: String = \"a\"\n    #else\n    title: String = \"b\"\n    #endif\n  }\n  actions {\n    load()\n  }\n  #if ios\n  lifecycle {\n    onAppear: load\n  }\n  #endif\n  view {\n    Text(title)\n  }\n}\n";
    let p = TempProject::new("split", &[("veyra.toml", config), ("ui/home.vey", ui)]);
    let (ok, out) = p.run(&["check"]);
    assert!(!ok, "{out}");
    assert!(out.contains("V1003"), "{out}");
    assert!(out.contains("按系统区分的声明也不能重名"), "{out}");
    assert!(out.contains("lifecycle块不能按Apple系统区分"), "{out}");
}
