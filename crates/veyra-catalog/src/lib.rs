// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! SwiftUI API目录
//!
//! 目录由`veyra-catalog`工具从各Apple SDK的`.swiftinterface`生成，提交在仓库的
//! `catalog/swiftui.json`中，编译器构建时嵌入。目录只收录参数能映射到Veyra类型的重载：
//!
//! - `views`：遵循`View`的类型及其构造器
//! - `modifiers`：`View`扩展中的方法
//! - `types`与`protocols`：用于`.token`取值的静态成员与枚举成员
//!
//! 每个条目都带有各系统的可用性，供版本检查使用

mod parse;
mod schema;

pub use parse::{InterfaceFile, generate};
pub use schema::*;

use std::sync::OnceLock;

/// 嵌入编译器的目录
pub fn swiftui() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../../../catalog/swiftui.json"))
            .expect("catalog/swiftui.json格式无效")
    })
}
