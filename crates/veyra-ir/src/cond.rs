// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 保留到原生代码中的条件
//!
//! Veyra在编译期确定后端平台与自定义flag，但`debug`与Apple各系统只能由原生构建决定
//! 裁剪时把每个节点生效的“世界”记为一个集合，每个世界是一种(系统, 是否调试)的组合，
//! 再把集合化简为[`NativeCond`]输出到原生代码中

use std::fmt;

/// Apple平台上的操作系统
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AppleOs {
    Ios,
    Macos,
    Watchos,
    Tvos,
    Visionos,
}

impl AppleOs {
    pub const ALL: [AppleOs; 5] = [
        AppleOs::Ios,
        AppleOs::Macos,
        AppleOs::Watchos,
        AppleOs::Tvos,
        AppleOs::Visionos,
    ];

    /// 条件编译与配置中使用的名称
    pub fn cfg_name(self) -> &'static str {
        match self {
            AppleOs::Ios => "ios",
            AppleOs::Macos => "macos",
            AppleOs::Watchos => "watchos",
            AppleOs::Tvos => "tvos",
            AppleOs::Visionos => "visionos",
        }
    }

    pub fn from_cfg_name(name: &str) -> Option<AppleOs> {
        AppleOs::ALL.into_iter().find(|o| o.cfg_name() == name)
    }

    /// Swift中`os(...)`与`#available`使用的名称
    pub fn swift_name(self) -> &'static str {
        match self {
            AppleOs::Ios => "iOS",
            AppleOs::Macos => "macOS",
            AppleOs::Watchos => "watchOS",
            AppleOs::Tvos => "tvOS",
            AppleOs::Visionos => "visionOS",
        }
    }
}

impl fmt::Display for AppleOs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.cfg_name())
    }
}

/// 原生条件
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NativeCond {
    Debug,
    Os(AppleOs),
    Not(Box<NativeCond>),
    And(Vec<NativeCond>),
    Or(Vec<NativeCond>),
}

impl NativeCond {
    pub fn negate(self) -> NativeCond {
        match self {
            NativeCond::Not(inner) => *inner,
            other => NativeCond::Not(Box::new(other)),
        }
    }

    /// 是否涉及Apple系统
    pub fn has_os(&self) -> bool {
        match self {
            NativeCond::Debug => false,
            NativeCond::Os(_) => true,
            NativeCond::Not(inner) => inner.has_os(),
            NativeCond::And(items) | NativeCond::Or(items) => items.iter().any(NativeCond::has_os),
        }
    }

    /// 是否涉及调试构建
    pub fn has_debug(&self) -> bool {
        match self {
            NativeCond::Debug => true,
            NativeCond::Os(_) => false,
            NativeCond::Not(inner) => inner.has_debug(),
            NativeCond::And(items) | NativeCond::Or(items) => {
                items.iter().any(NativeCond::has_debug)
            }
        }
    }

    /// 输出为Swift的`#if`条件
    pub fn swift(&self) -> String {
        self.render(&|c| match c {
            NativeCond::Debug => "DEBUG".into(),
            NativeCond::Os(os) => format!("os({})", os.swift_name()),
            _ => unreachable!(),
        })
    }

    /// 以给定的原子渲染函数输出条件，`!`、`&&`、`||`的写法在Swift与Kotlin中相同
    pub fn render(&self, atom: &dyn Fn(&NativeCond) -> String) -> String {
        match self {
            NativeCond::Debug | NativeCond::Os(_) => atom(self),
            NativeCond::Not(inner) => match **inner {
                NativeCond::Debug | NativeCond::Os(_) => format!("!{}", inner.render(atom)),
                _ => format!("!({})", inner.render(atom)),
            },
            NativeCond::And(items) => items
                .iter()
                .map(|c| match c {
                    NativeCond::Or(_) => format!("({})", c.render(atom)),
                    _ => c.render(atom),
                })
                .collect::<Vec<_>>()
                .join(" && "),
            NativeCond::Or(items) => items
                .iter()
                .map(|c| c.render(atom))
                .collect::<Vec<_>>()
                .join(" || "),
        }
    }
}

/// 裁剪时的世界集合
///
/// 世界编号为`os_index * 2 + debug`，没有Apple系统时只有两个世界
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Worlds {
    bits: u32,
    /// 参与区分的系统个数，0表示非Apple平台或已确定系统
    os_count: u8,
}

impl Worlds {
    pub fn all(os_count: usize) -> Self {
        let n = os_count.max(1) * 2;
        Self {
            bits: (1u32 << n) - 1,
            os_count: os_count as u8,
        }
    }

    pub fn empty(os_count: usize) -> Self {
        Self {
            bits: 0,
            os_count: os_count as u8,
        }
    }

    fn slots(self) -> usize {
        (self.os_count as usize).max(1)
    }

    pub fn is_empty(self) -> bool {
        self.bits == 0
    }

    pub fn is_all(self) -> bool {
        self == Worlds::all(self.os_count as usize)
    }

    pub fn contains(self, os_index: usize, debug: bool) -> bool {
        self.bits & (1 << (os_index * 2 + debug as usize)) != 0
    }

    pub fn insert(&mut self, os_index: usize, debug: bool) {
        self.bits |= 1 << (os_index * 2 + debug as usize);
    }

    pub fn and(self, other: Worlds) -> Worlds {
        Worlds {
            bits: self.bits & other.bits,
            os_count: self.os_count,
        }
    }

    pub fn minus(self, other: Worlds) -> Worlds {
        Worlds {
            bits: self.bits & !other.bits,
            os_count: self.os_count,
        }
    }

    /// 每个系统下是否都与调试构建无关
    pub fn depends_on_debug(self) -> bool {
        (0..self.slots()).any(|i| self.contains(i, false) != self.contains(i, true))
    }

    pub fn depends_on_os(self) -> bool {
        let first = [self.contains(0, false), self.contains(0, true)];
        (1..self.slots()).any(|i| [self.contains(i, false), self.contains(i, true)] != first)
    }

    /// 化简为原生条件，`oses`为参与区分的系统；集合为全集时返回None
    pub fn to_cond(self, oses: &[AppleOs]) -> Option<NativeCond> {
        if self.is_all() {
            return None;
        }
        let slots = self.slots();
        let os_cond = |selected: &[usize]| -> NativeCond {
            // 选中的系统较多时写成排除形式
            if selected.len() * 2 > slots && selected.len() < slots {
                let rest: Vec<_> = (0..slots)
                    .filter(|i| !selected.contains(i))
                    .map(|i| NativeCond::Os(oses[i]))
                    .collect();
                let or = if rest.len() == 1 {
                    rest.into_iter().next().unwrap_or(NativeCond::Debug)
                } else {
                    NativeCond::Or(rest)
                };
                or.negate()
            } else {
                let items: Vec<_> = selected.iter().map(|i| NativeCond::Os(oses[*i])).collect();
                if items.len() == 1 {
                    items.into_iter().next().unwrap_or(NativeCond::Debug)
                } else {
                    NativeCond::Or(items)
                }
            }
        };
        if !self.depends_on_debug() {
            let selected: Vec<usize> = (0..slots).filter(|i| self.contains(*i, false)).collect();
            return Some(os_cond(&selected));
        }
        if !self.depends_on_os() {
            return Some(if self.contains(0, true) {
                NativeCond::Debug
            } else {
                NativeCond::Debug.negate()
            });
        }
        // 按系统分组：完全生效的系统、只在调试或发布生效的系统
        let mut always = Vec::new();
        let mut debug_only = Vec::new();
        let mut release_only = Vec::new();
        for i in 0..slots {
            match (self.contains(i, false), self.contains(i, true)) {
                (true, true) => always.push(i),
                (false, true) => debug_only.push(i),
                (true, false) => release_only.push(i),
                (false, false) => {}
            }
        }
        let mut terms = Vec::new();
        if !always.is_empty() {
            terms.push(os_cond(&always));
        }
        if !debug_only.is_empty() {
            terms.push(NativeCond::And(vec![
                os_cond(&debug_only),
                NativeCond::Debug,
            ]));
        }
        if !release_only.is_empty() {
            terms.push(NativeCond::And(vec![
                os_cond(&release_only),
                NativeCond::Debug.negate(),
            ]));
        }
        Some(if terms.len() == 1 {
            terms.into_iter().next().unwrap_or(NativeCond::Debug)
        } else {
            NativeCond::Or(terms)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OSES: [AppleOs; 3] = [AppleOs::Ios, AppleOs::Macos, AppleOs::Visionos];

    fn worlds(items: &[(usize, bool)]) -> Worlds {
        let mut w = Worlds::empty(3);
        for (os, debug) in items {
            w.insert(*os, *debug);
        }
        w
    }

    #[test]
    fn simplifies_conditions() {
        assert_eq!(Worlds::all(3).to_cond(&OSES), None);
        let ios = worlds(&[(0, false), (0, true)]);
        assert_eq!(ios.to_cond(&OSES).unwrap().swift(), "os(iOS)");
        let not_ios = worlds(&[(1, false), (1, true), (2, false), (2, true)]);
        assert_eq!(not_ios.to_cond(&OSES).unwrap().swift(), "!os(iOS)");
        let debug = worlds(&[(0, true), (1, true), (2, true)]);
        assert_eq!(debug.to_cond(&OSES).unwrap().swift(), "DEBUG");
        let ios_debug = worlds(&[(0, true)]);
        assert_eq!(
            ios_debug.to_cond(&OSES).unwrap().swift(),
            "os(iOS) && DEBUG"
        );
        let mixed = worlds(&[(0, false), (0, true), (1, true)]);
        assert_eq!(
            mixed.to_cond(&OSES).unwrap().swift(),
            "os(iOS) || os(macOS) && DEBUG"
        );
    }

    #[test]
    fn handles_single_os() {
        let mut w = Worlds::empty(0);
        w.insert(0, true);
        assert_eq!(w.to_cond(&[]).unwrap().swift(), "DEBUG");
        assert_eq!(Worlds::all(0).to_cond(&[]), None);
    }
}
