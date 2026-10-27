// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// 系统版本，例如`26.0`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let mut parts = s.trim().split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().map(|m| m.parse().ok()).unwrap_or(Some(0))?;
        Some(Version { major, minor })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor).cmp(&(other.major, other.minor))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.minor == 0 {
            write!(f, "{}", self.major)
        } else {
            write!(f, "{}.{}", self.major, self.minor)
        }
    }
}

impl Serialize for Version {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{}.{}", self.major, self.minor))
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Version::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("无效的版本`{s}`")))
    }
}

/// 某个系统上的可用性
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OsAvailability {
    Available {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        introduced: Option<Version>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deprecated: Option<Version>,
    },
    Unavailable,
}

/// 各系统的可用性，键为`ios`、`macos`等，缺失表示不可用
pub type Availability = BTreeMap<String, OsAvailability>;

/// 文本参数在Swift中的类型
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextKind {
    /// `S: StringProtocol`
    String,
    Text,
    LocalizedKey,
}

/// 参数类型
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamType {
    Bool,
    Int,
    Double,
    Float,
    String,
    Text(TextKind),
    Binding(Box<ParamType>),
    Action {
        is_async: bool,
    },
    /// `@ViewBuilder () -> Content`
    Content,
    /// 视图本身作为参数，例如`Section(header:)`
    View,
    /// 任意可哈希的值，例如`Picker`的选中项与`.tag()`
    Any,
    /// `URL`，由字符串构造
    Url,
    Date,
    /// 受`Strideable`约束的数值，Int或Double均可
    Number,
    /// 具体类型的静态成员，例如`Font`
    Token(String),
    /// 受协议约束的泛型，取值为协议上的静态成员，例如`ButtonStyle`
    Style(String),
    Optional(Box<ParamType>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Param {
    /// Swift中的参数标签，`None`表示`_`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(rename = "type")]
    pub ty: ParamType,
    /// 是否有默认值
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default: bool,
}

/// 一个构造器或方法的重载
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    pub params: Vec<Param>,
    pub availability: Availability,
}

/// 静态成员或枚举成员，`params`为None表示属性或枚举成员
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Vec<Param>>,
    pub availability: Availability,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Catalog {
    /// 生成目录所用的SDK版本
    pub sdk: String,
    pub views: BTreeMap<String, Vec<Signature>>,
    pub modifiers: BTreeMap<String, Vec<Signature>>,
    pub types: BTreeMap<String, BTreeMap<String, Vec<Member>>>,
    pub protocols: BTreeMap<String, BTreeMap<String, Vec<Member>>>,
}

impl Catalog {
    /// 类型或协议上的静态成员
    pub fn members(&self, owner: &str) -> Option<&BTreeMap<String, Vec<Member>>> {
        self.types.get(owner).or_else(|| self.protocols.get(owner))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(Version::parse("26.0").unwrap() > Version::parse("17.4").unwrap());
        assert_eq!(Version::parse("17").unwrap().to_string(), "17");
        assert_eq!(Version::parse("17.4").unwrap().to_string(), "17.4");
    }
}
