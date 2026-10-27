// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! veyra-catalog，从本机Xcode的Apple SDK生成SwiftUI API目录
//!
//! 用法：
//!
//! ```text
//! veyra-catalog [--out catalog/swiftui.json] [--explain NAME]
//! ```
//!
//! 通过`xcrun --sdk <sdk> --show-sdk-path`找到各系统的SDK，读取SwiftUI与SwiftUICore的
//! `.swiftinterface`，合并后写入目录。`--explain`列出某个名称被跳过的重载及原因

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use veyra_catalog::{InterfaceFile, generate};

/// (系统, SDK名称, 接口文件名中的目标三元组前缀)
const SDKS: &[(&str, &str, &str)] = &[
    ("ios", "iphonesimulator", "arm64-apple-ios-simulator"),
    ("macos", "macosx", "arm64e-apple-macos"),
    ("watchos", "watchsimulator", "arm64-apple-watchos-simulator"),
    ("tvos", "appletvsimulator", "arm64-apple-tvos-simulator"),
    ("visionos", "xrsimulator", "arm64-apple-xros-simulator"),
];

fn sdk_path(sdk: &str) -> Option<PathBuf> {
    let out = Command::new("xcrun")
        .args(["--sdk", sdk, "--show-sdk-path"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}

fn sdk_version(sdk: &str) -> String {
    Command::new("xcrun")
        .args(["--sdk", sdk, "--show-sdk-version"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn interface(root: &Path, framework: &str, triple: &str) -> Option<String> {
    let path = root.join(format!(
        "System/Library/Frameworks/{framework}.framework/Modules/{framework}.swiftmodule/{triple}.swiftinterface"
    ));
    std::fs::read_to_string(path).ok()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = PathBuf::from("catalog/swiftui.json");
    let mut explain = None;
    let mut stats = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out = PathBuf::from(&args[i]);
            }
            "--stats" => stats = true,
            "--explain" => {
                i += 1;
                explain = Some(args[i].clone());
            }
            other => {
                eprintln!("未知参数：{other}");
                return ExitCode::FAILURE;
            }
        }
        i += 1;
    }
    let mut files = Vec::new();
    for (os, sdk, triple) in SDKS {
        let Some(root) = sdk_path(sdk) else {
            eprintln!("跳过{os}：未安装{sdk} SDK");
            continue;
        };
        let mut text = String::new();
        for framework in ["SwiftUICore", "SwiftUI"] {
            match interface(&root, framework, triple) {
                Some(t) => text.push_str(&t),
                None => eprintln!("警告：{os}缺少{framework}的接口文件"),
            }
            text.push('\n');
        }
        files.push(InterfaceFile {
            os: os.to_string(),
            text,
        });
    }
    if files.is_empty() {
        eprintln!("没有找到任何Apple SDK");
        return ExitCode::FAILURE;
    }
    let (catalog, skipped) = generate(&files, &sdk_version("iphonesimulator"));
    if stats {
        // 按原因的大类统计，便于决定下一步支持哪些类型
        let mut counts: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for s in &skipped {
            let reason = s
                .reason
                .split_once('：')
                .map(|(_, r)| r)
                .unwrap_or(&s.reason);
            *counts.entry(reason.to_string()).or_default() += 1;
        }
        let mut list: Vec<_> = counts.into_iter().collect();
        list.sort_by_key(|item| std::cmp::Reverse(item.1));
        for (reason, n) in list.into_iter().take(40) {
            println!("{n:5} {reason}");
        }
        return ExitCode::SUCCESS;
    }
    if let Some(name) = explain {
        for s in skipped
            .iter()
            .filter(|s| s.name.trim_start_matches('.') == name.trim_start_matches('.'))
        {
            println!("{}：{}", s.name, s.reason);
        }
        return ExitCode::SUCCESS;
    }
    let json = match serde_json::to_string_pretty(&catalog) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("序列化失败：{e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = std::fs::write(&out, json + "\n") {
        eprintln!("无法写入{}：{e}", out.display());
        return ExitCode::FAILURE;
    }
    let overloads: usize = catalog.modifiers.values().map(Vec::len).sum();
    eprintln!(
        "已生成{}：{}个视图，{}个修饰符（{}个重载），{}个类型，{}个协议，跳过{}个重载",
        out.display(),
        catalog.views.len(),
        catalog.modifiers.len(),
        overloads,
        catalog.types.len(),
        catalog.protocols.len(),
        skipped.len()
    );
    ExitCode::SUCCESS
}
