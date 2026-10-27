// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

use veyra_backend_compose::ComposeBackend;
use veyra_codegen::{Backend, BackendInput};
use veyra_ir::{Catalog, CatalogEntry, Platform, Translation};
use veyra_sema::{Target, analyze};

#[test]
fn generates_schedule() {
    let src = include_str!("fixtures.vey");
    let parsed = veyra_syntax::parse(Default::default(), src);
    assert!(parsed.diagnostics.is_empty());
    let analysis = analyze(&[parsed.file], &Target::new(Platform::Android));
    assert!(
        !analysis.diagnostics.has_errors(),
        "{:?}",
        analysis.diagnostics
    );
    let entries = analysis
        .program
        .strings
        .iter()
        .map(|s| {
            let mut translations = indexmap::IndexMap::new();
            if s.key == "schedule.title" {
                translations.insert("en".to_string(), Translation::Text("My Schedule".into()));
            }
            if s.key == "schedule.items" {
                let mut forms = indexmap::IndexMap::new();
                forms.insert("one".to_string(), "{count} course".to_string());
                forms.insert("other".to_string(), "{count} courses".to_string());
                translations.insert("en".to_string(), Translation::Plural(forms));
            }
            if s.key == "schedule.count" {
                translations.insert(
                    "en".to_string(),
                    Translation::Text("{filter}: {count} courses".into()),
                );
            }
            CatalogEntry {
                string: s.clone(),
                translations,
            }
        })
        .collect();
    let catalog = Catalog {
        source_locale: "zh-Hans".into(),
        target_locales: vec!["en".into()],
        entries,
    };
    let options: toml::Table = toml::from_str(
        "package = \"com.example.schedule.ui\"\nr_package = \"com.example.schedule\"",
    )
    .unwrap();
    let files = ComposeBackend
        .generate(&BackendInput {
            program: &analysis.program,
            catalog: &catalog,
            options: &options,
        })
        .unwrap();
    if let Ok(dir) = std::env::var("VEYRA_DUMP") {
        for f in &files {
            let path = std::path::Path::new(&dir).join(&f.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, &f.contents).unwrap();
        }
    }
    for f in files {
        let name = f.path.to_string_lossy().replace('/', "__");
        insta::assert_snapshot!(name, f.contents);
    }
}

#[test]
fn generates_with_di_option() {
    let src = include_str!("fixtures.vey");
    let parsed = veyra_syntax::parse(Default::default(), src);
    let analysis = analyze(&[parsed.file], &Target::new(Platform::Android));
    let catalog = Catalog {
        source_locale: "zh-Hans".into(),
        target_locales: Vec::new(),
        entries: Vec::new(),
    };
    let options: toml::Table = toml::from_str("package = \"com.example.schedule.ui\"\nr_package = \"com.example.schedule\"\ndi = \"hilt\"").unwrap();
    let files = ComposeBackend
        .generate(&BackendInput {
            program: &analysis.program,
            catalog: &catalog,
            options: &options,
        })
        .unwrap();
    let keep = ["Route", "ViewModel.kt", "VeyraDependencies"];
    for f in files {
        let name = f.path.to_string_lossy().replace('/', "__");
        if keep.iter().any(|k| name.contains(k)) {
            insta::assert_snapshot!(format!("di__{name}"), f.contents);
        }
    }
}

#[test]
fn generates_storage() {
    let src = include_str!("storage.vey");
    let parsed = veyra_syntax::parse(Default::default(), src);
    assert!(parsed.diagnostics.is_empty());
    let analysis = analyze(&[parsed.file], &Target::new(Platform::Android));
    assert!(
        !analysis.diagnostics.has_errors(),
        "{:?}",
        analysis.diagnostics
    );
    let catalog = Catalog {
        source_locale: "en".into(),
        target_locales: Vec::new(),
        entries: Vec::new(),
    };
    let options: toml::Table = toml::from_str("package = \"com.example.tasks\"").unwrap();
    let files = ComposeBackend
        .generate(&BackendInput {
            program: &analysis.program,
            catalog: &catalog,
            options: &options,
        })
        .unwrap();
    if let Ok(dir) = std::env::var("VEYRA_DUMP_STORAGE") {
        for f in &files {
            let path = std::path::Path::new(&dir).join(&f.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, &f.contents).unwrap();
        }
    }
    for f in files {
        let name = f.path.to_string_lossy().replace('/', "__");
        insta::assert_snapshot!(format!("storage__{name}"), f.contents);
    }
}

#[test]
fn generates_layout() {
    let src = include_str!("layout.vey");
    let parsed = veyra_syntax::parse(Default::default(), src);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let analysis = analyze(&[parsed.file], &Target::new(Platform::Android));
    assert!(
        !analysis.diagnostics.has_errors(),
        "{:?}",
        analysis.diagnostics
    );
    let catalog = Catalog {
        source_locale: "en".into(),
        target_locales: Vec::new(),
        entries: Vec::new(),
    };
    let options: toml::Table = toml::from_str("package = \"com.example.gallery\"").unwrap();
    let files = ComposeBackend
        .generate(&BackendInput {
            program: &analysis.program,
            catalog: &catalog,
            options: &options,
        })
        .unwrap();
    if let Ok(dir) = std::env::var("VEYRA_DUMP_LAYOUT") {
        for f in &files {
            let path = std::path::Path::new(&dir).join(&f.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, &f.contents).unwrap();
        }
    }
    for f in files {
        let name = f.path.to_string_lossy().replace('/', "__");
        if name.contains("Content") || name.contains("View.generated") {
            insta::assert_snapshot!(format!("layout__{name}"), f.contents);
        }
    }
}
