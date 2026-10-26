// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use veyra_codegen::{GENERATED_NOTICE, OutputFile, Ownership};

const MANIFEST: &str = ".veyra/manifest.json";

#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    generated: BTreeSet<PathBuf>,
}

/// 写入结果统计
#[derive(Clone, Debug, Default)]
pub struct WriteReport {
    pub written: Vec<PathBuf>,
    pub unchanged: Vec<PathBuf>,
    pub created: Vec<PathBuf>,
    pub kept: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
}

/// 写入全部输出文件，并清理上次生成但本次不再生成的文件
pub fn write_outputs(root: &Path, files: &[OutputFile]) -> io::Result<WriteReport> {
    let mut report = WriteReport::default();
    let manifest_path = root.join(MANIFEST);
    let old: Manifest = std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let mut new = Manifest::default();

    for f in files {
        let abs = root.join(&f.path);
        match f.ownership {
            Ownership::Generated => {
                new.generated.insert(f.path.clone());
                if std::fs::read_to_string(&abs).is_ok_and(|c| c == f.contents) {
                    report.unchanged.push(f.path.clone());
                    continue;
                }
                write(&abs, &f.contents)?;
                report.written.push(f.path.clone());
            }
            Ownership::UserOwned => {
                if abs.exists() {
                    report.kept.push(f.path.clone());
                } else {
                    write(&abs, &f.contents)?;
                    report.created.push(f.path.clone());
                }
            }
        }
    }

    for stale in old.generated.difference(&new.generated) {
        let abs = root.join(stale);
        // 只删除仍带有生成标记的文件
        if std::fs::read_to_string(&abs).is_ok_and(|c| c.contains(GENERATED_NOTICE)) {
            std::fs::remove_file(&abs)?;
            report.removed.push(stale.clone());
        }
    }

    let json = serde_json::to_string_pretty(&new).map_err(io::Error::other)?;
    write(&manifest_path, &(json + "\n"))?;
    Ok(report)
}

fn write(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, contents)
}
