// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! `veyra init`

use std::path::Path;

use veyra_diagnostics::{Diagnostic, Diagnostics, RenderOptions, SourceMap, render_all};
use veyra_project::CONFIG_FILE;

const CONFIG: &str = r#"[project]
name = "{name}"
sources = ["ui"]

[l10n]
source = "en"
targets = []
dir = "locales"
# warn | error
missing = "warn"

[swiftui]
out = "ios/Generated"
user_dir = "ios/ViewModels"

[compose]
out = "android/app/src/main/java"
res = "android/app/src/main/res"
user_dir = "android/app/src/main/java"
package = "com.example.{package}.ui"
r_package = "com.example.{package}"

# Custom conditions for #if, override with --flag
[flags]
"#;

const SAMPLE: &str = r#"// Entry screen of {name}
screen Home {
  state {
    name: String = "" @writable
  }

  computed {
    canGreet: Bool = !name.isEmpty
  }

  actions {
    greet()
  }

  view {
    Column(spacing: 12) {
      Text(loc("home.title", "Hello, Veyra")).font(.title)
      TextField(loc("home.name", "Your name"), text: bind(name))
      Button(loc("home.greet", "Greet"), action: greet).disabled(!canGreet)
    }
    .padding(16)
  }
}
"#;

fn fail(message: String, color: bool) -> bool {
    let d = Diagnostics::from(Diagnostic::error("V9008", message));
    eprint!(
        "{}",
        render_all(&d, &SourceMap::new(), RenderOptions { color })
    );
    false
}

pub(crate) fn run(dir: &Path, name: Option<String>, color: bool) -> bool {
    if dir.join(CONFIG_FILE).exists() {
        return fail(format!("{}中已存在{CONFIG_FILE}", dir.display()), color);
    }
    let name = name.unwrap_or_else(|| {
        let abs = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        abs.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "App".into())
    });
    let package: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    let package = if package.is_empty() || package.starts_with(|c: char| c.is_ascii_digit()) {
        format!("app{package}")
    } else {
        package
    };
    let files = [
        (
            dir.join(CONFIG_FILE),
            CONFIG
                .replace("{name}", &name)
                .replace("{package}", &package),
        ),
        (
            dir.join("ui").join("Home.vey"),
            SAMPLE.replace("{name}", &name),
        ),
    ];
    for (path, contents) in &files {
        if path.exists() {
            continue;
        }
        let result = path
            .parent()
            .map(std::fs::create_dir_all)
            .unwrap_or(Ok(()))
            .and_then(|_| std::fs::write(path, contents));
        if let Err(e) = result {
            return fail(format!("无法写入{}：{e}", path.display()), color);
        }
        eprintln!("    创建 {}", path.display());
    }
    eprintln!("已创建项目{name}，运行`veyra build`生成代码");
    true
}
