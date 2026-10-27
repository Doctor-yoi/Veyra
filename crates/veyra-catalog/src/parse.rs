// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! `.swiftinterface`的容错解析
//!
//! 接口文件是编译器打印的Swift声明，每个声明基本占一行，`@inlinable`方法带有函数体
//! 解析器只关心声明头：按花括号深度跟踪所在的扩展或类型，收集属性，
//! 再把参数类型映射为[`ParamType`]，无法映射的重载记录原因后跳过

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::schema::*;

/// 一个系统的接口文件
pub struct InterfaceFile {
    /// `ios`、`macos`等
    pub os: String,
    pub text: String,
}

/// 生成结果中被跳过的重载
#[derive(Clone, Debug)]
pub struct Skipped {
    pub name: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default)]
struct AvailAttr {
    /// 键为系统名，`*`表示全部
    introduced: HashMap<String, Version>,
    deprecated: HashMap<String, Version>,
    unavailable: HashSet<String>,
    deprecated_all: bool,
}

impl AvailAttr {
    fn merge(&mut self, other: &AvailAttr) {
        for (k, v) in &other.introduced {
            self.introduced.insert(k.clone(), *v);
        }
        for (k, v) in &other.deprecated {
            self.deprecated.insert(k.clone(), *v);
        }
        self.unavailable.extend(other.unavailable.iter().cloned());
        self.deprecated_all |= other.deprecated_all;
    }

    fn for_os(&self, os: &str) -> OsAvailability {
        if self.unavailable.contains(os) {
            return OsAvailability::Unavailable;
        }
        let deprecated = self.deprecated.get(os).copied().or(if self.deprecated_all {
            Some(Version::default())
        } else {
            None
        });
        OsAvailability::Available {
            introduced: self.introduced.get(os).copied(),
            deprecated,
        }
    }
}

fn os_key(platform: &str) -> Option<&'static str> {
    Some(match platform {
        "iOS" => "ios",
        "macOS" => "macos",
        "watchOS" => "watchos",
        "tvOS" => "tvos",
        "visionOS" | "xrOS" => "visionos",
        _ => return None,
    })
}

/// 解析`@available(...)`
fn parse_available(args: &str, out: &mut AvailAttr) {
    let parts: Vec<&str> = split_top(args, ',').into_iter().map(str::trim).collect();
    let Some(first) = parts.first() else { return };
    // 简写形式：@available(iOS 16.0, macOS 13.0, *)
    if parts
        .iter()
        .all(|p| *p == "*" || p.split_whitespace().count() == 2 && !p.contains(':'))
    {
        for p in &parts {
            let mut it = p.split_whitespace();
            if let (Some(os), Some(v)) = (it.next(), it.next())
                && let (Some(key), Some(v)) = (os_key(os), Version::parse(v))
            {
                out.introduced.insert(key.to_string(), v);
            }
        }
        return;
    }
    // 完整形式：@available(iOS, introduced: 13.0, deprecated: 17.0) 或 @available(*, deprecated)
    let os = if *first == "*" { None } else { os_key(first) };
    if *first != "*" && os.is_none() {
        return;
    }
    for p in &parts[1..] {
        let (k, v) = match p.split_once(':') {
            Some((k, v)) => (k.trim(), Some(v.trim())),
            None => (p.trim(), None),
        };
        match (k, os) {
            ("unavailable", Some(os)) => {
                out.unavailable.insert(os.to_string());
            }
            ("introduced", Some(os)) => {
                if let Some(v) = v.and_then(Version::parse) {
                    out.introduced.insert(os.to_string(), v);
                }
            }
            ("deprecated", Some(os)) => match v.and_then(Version::parse) {
                // 100000表示未来版本才会废弃
                Some(v) if v.major < 10000 => {
                    out.deprecated.insert(os.to_string(), v);
                }
                Some(_) => {}
                None => {
                    out.deprecated.insert(os.to_string(), Version::default());
                }
            },
            ("deprecated", None) => out.deprecated_all = true,
            ("unavailable", None) => {
                for o in ["ios", "macos", "watchos", "tvos", "visionos"] {
                    out.unavailable.insert(o.to_string());
                }
            }
            _ => {}
        }
    }
}

/// 按顶层分隔符切分，忽略括号与尖括号内部
fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let mut in_str = false;
    let bytes: Vec<char> = s.chars().collect();
    let mut byte_pos = 0;
    for (i, c) in bytes.iter().enumerate() {
        let len = c.len_utf8();
        match c {
            '"' => in_str = !in_str,
            '(' | '[' | '<' | '{' if !in_str => depth += 1,
            ')' | ']' | '}' if !in_str => depth -= 1,
            // `->`中的`>`不是尖括号
            '>' if !(in_str || i > 0 && bytes[i - 1] == '-') => depth -= 1,
            c if *c == sep && depth == 0 && !in_str => {
                out.push(&s[start..byte_pos]);
                start = byte_pos + len;
            }
            _ => {}
        }
        byte_pos += len;
    }
    out.push(&s[start..]);
    out
}

/// 去掉模块限定，例如`SwiftUICore::View`、`Swift.Bool`
fn normalize(s: &str) -> String {
    const MODULES: &[&str] = &[
        "Swift",
        "SwiftUI",
        "SwiftUICore",
        "CoreFoundation",
        "CoreGraphics",
        "Foundation",
        "_Concurrency",
        "CoreTransferable",
        "Combine",
        "Observation",
        "UIKit",
        "AppKit",
        "Accessibility",
        "Symbols",
        "DeveloperToolsSupport",
        "UniformTypeIdentifiers",
        "CoreText",
        "QuartzCore",
        "Spatial",
        "os",
        "Darwin",
        "simd",
        "_StringProcessing",
        "CoreData",
        "Dispatch",
    ];
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let prev_dot = start > 0 && chars[start - 1] == '.';
            if i + 1 < chars.len() && chars[i] == ':' && chars[i + 1] == ':' {
                i += 2;
                continue;
            }
            if !prev_dot && i < chars.len() && chars[i] == '.' && MODULES.contains(&word.as_str()) {
                i += 1;
                continue;
            }
            out.push_str(&word);
            continue;
        }
        out.push(c);
        i += 1;
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 不计字符串中的括号
fn count(s: &str, open: char, close: char) -> i32 {
    let mut n = 0;
    let mut in_str = false;
    let mut prev = ' ';
    for c in s.chars() {
        if c == '"' && prev != '\\' {
            in_str = !in_str;
        } else if !in_str {
            if c == open {
                n += 1;
            } else if c == close {
                n -= 1;
            }
        }
        prev = c;
    }
    n
}

/// 拆出语句开头的属性
fn split_attrs(s: &str) -> (Vec<String>, &str) {
    let mut attrs = Vec::new();
    let mut rest = s.trim_start();
    while rest.starts_with('@') {
        let name_end = rest[1..]
            .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.' || c == ':'))
            .map(|i| i + 1)
            .unwrap_or(rest.len());
        let mut end = name_end;
        if rest[name_end..].starts_with('(') {
            let mut depth = 0;
            for (i, c) in rest[name_end..].char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = name_end + i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
        attrs.push(rest[..end].to_string());
        rest = rest[end..].trim_start();
    }
    (attrs, rest)
}

const MODIFIER_WORDS: &[&str] = &[
    "public",
    "nonisolated",
    "final",
    "open",
    "internal",
    "mutating",
    "nonmutating",
    "override",
    "convenience",
    "required",
    "indirect",
    "dynamic",
    "optional",
    "lazy",
    "isolated",
    "borrowing",
    "consuming",
    "package",
];

#[derive(Clone, Debug)]
enum Frame {
    Ext {
        target: String,
        self_type: Option<String>,
        avail: AvailAttr,
        generics: HashMap<String, Vec<String>>,
    },
    Type {
        name: String,
        avail: AvailAttr,
        generics: HashMap<String, Vec<String>>,
    },
    Body,
}

#[derive(Clone, Debug)]
enum DeclKind {
    /// 视图构造器，所属类型
    Init(String),
    /// View扩展中的方法
    Modifier,
    /// 静态成员或枚举成员，所属类型或协议，是否位于`where Self ==`扩展中
    Member { owner: String, protocol: bool },
}

#[derive(Clone, Debug)]
struct RawParam {
    label: Option<String>,
    ty: String,
    builder: bool,
    default: bool,
}

#[derive(Clone, Debug)]
struct RawDecl {
    kind: DeclKind,
    name: String,
    /// None表示属性或无关联值的枚举成员
    params: Option<Vec<RawParam>>,
    ret: String,
    generics: HashMap<String, Vec<String>>,
    avail: AvailAttr,
    disfavored: bool,
}

struct FileResult {
    decls: Vec<RawDecl>,
    /// 类型声明头部的泛型约束，用于类型扩展中的构造器
    type_generics: HashMap<String, HashMap<String, Vec<String>>>,
    view_types: HashSet<String>,
    protocols: HashSet<String>,
}

/// 解析`<A, B: View>`中的泛型参数
fn parse_generics(s: &str, out: &mut HashMap<String, Vec<String>>) {
    for p in split_top(s, ',') {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        match p.split_once(':') {
            Some((name, cons)) => {
                let entry = out.entry(name.trim().to_string()).or_default();
                for c in cons.split('&') {
                    entry.push(c.trim().to_string());
                }
            }
            None => {
                out.entry(p.to_string()).or_default();
            }
        }
    }
}

/// 解析`where A : View, B == Text`，返回`Self ==`右侧的类型
fn parse_where(s: &str, out: &mut HashMap<String, Vec<String>>) -> Option<String> {
    let mut self_type = None;
    for c in split_top(s, ',') {
        let c = c.trim();
        if let Some((l, r)) = c.split_once("==") {
            if l.trim() == "Self" {
                self_type = Some(r.trim().to_string());
            }
        } else if let Some((l, r)) = c.split_once(':') {
            let entry = out.entry(l.trim().to_string()).or_default();
            for p in r.split('&') {
                entry.push(p.trim().to_string());
            }
        }
    }
    self_type
}

/// 拆出`name<generics>(params) rest`
fn split_signature(s: &str) -> Option<(String, String, String, String)> {
    let paren = s.find('(')?;
    let head = &s[..paren];
    let (name, generics) = match head.find('<') {
        Some(lt) => (
            head[..lt].trim().to_string(),
            head[lt + 1..head.rfind('>')?].to_string(),
        ),
        None => (head.trim().to_string(), String::new()),
    };
    let mut depth = 0;
    let mut end = None;
    for (i, c) in s[paren..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(paren + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    Some((
        name,
        generics,
        s[paren + 1..end].to_string(),
        s[end + 1..].to_string(),
    ))
}

fn parse_params(s: &str) -> Vec<RawParam> {
    let mut out = Vec::new();
    for p in split_top(s, ',') {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        let Some((names, rest)) = p.split_once(':') else {
            continue;
        };
        let words: Vec<&str> = names
            .split_whitespace()
            .filter(|w| !w.starts_with('@'))
            .collect();
        let builder = names.contains("Builder");
        let label = match words.as_slice() {
            ["_", _] | ["_"] => None,
            [l, _] | [l] => Some(l.to_string()),
            _ => None,
        };
        let parts = split_top(rest, '=');
        let ty = parts[0].trim();
        let (attrs, ty) = split_attrs(ty);
        let builder = builder || attrs.iter().any(|a| a.contains("Builder"));
        out.push(RawParam {
            label,
            ty: ty.trim().to_string(),
            builder,
            default: parts.len() > 1,
        });
    }
    out
}

/// 去掉`-> ret where ...`中的返回类型与where子句
fn split_rest(rest: &str) -> (String, String) {
    let rest = rest.trim().trim_end_matches('{').trim();
    let rest = rest
        .trim_start_matches("async")
        .trim()
        .trim_start_matches("throws")
        .trim();
    let (ret, wh) = if let Some(w) = rest.strip_prefix("where ") {
        ("", w)
    } else {
        match rest.find(" where ") {
            Some(i) => (&rest[..i], &rest[i + 7..]),
            None => (rest, ""),
        }
    };
    (
        ret.trim_start_matches("->").trim().to_string(),
        wh.trim().to_string(),
    )
}

fn parse_file(text: &str) -> FileResult {
    let lines: Vec<&str> = text.lines().collect();
    let mut stack: Vec<(Frame, i32)> = Vec::new();
    let mut depth = 0i32;
    let mut pending: Vec<String> = Vec::new();
    let mut result = FileResult {
        decls: Vec::new(),
        type_generics: HashMap::new(),
        view_types: HashSet::new(),
        protocols: HashSet::new(),
    };
    let mut i = 0;
    while i < lines.len() {
        let mut stmt = lines[i].trim().to_string();
        i += 1;
        if stmt.is_empty() || stmt.starts_with("//") {
            continue;
        }
        if stmt.starts_with('#') {
            continue;
        }
        let in_body = matches!(stack.last(), Some((Frame::Body, _)));
        if !in_body {
            // 跨行的签名
            while count(&stmt, '(', ')') > 0 && i < lines.len() {
                stmt.push(' ');
                stmt.push_str(lines[i].trim());
                i += 1;
            }
        }
        let delta = count(&stmt, '{', '}');
        if in_body {
            depth += delta;
            while stack.last().is_some_and(|(_, d)| *d > depth) {
                stack.pop();
            }
            continue;
        }
        let (attrs, rest) = split_attrs(&stmt);
        pending.extend(attrs);
        let rest = rest.to_string();
        if rest.is_empty() {
            continue;
        }
        if rest.starts_with('}') {
            depth += delta;
            while stack.last().is_some_and(|(_, d)| *d > depth) {
                stack.pop();
            }
            pending.clear();
            continue;
        }
        let mut words = rest.as_str();
        let mut is_static = false;
        loop {
            let w = words.split_whitespace().next().unwrap_or("");
            if w.starts_with('@') {
                let (_, r) = split_attrs(words);
                words = r;
                continue;
            }
            if w == "static" {
                is_static = true;
                words = words.trim_start()[w.len()..].trim_start();
                continue;
            }
            if MODIFIER_WORDS.contains(&w) || w.starts_with("nonisolated(") || w.starts_with("__") {
                words = words.trim_start()[w.len()..].trim_start();
                continue;
            }
            break;
        }
        let attrs = std::mem::take(&mut pending);
        let mut avail = AvailAttr::default();
        for a in &attrs {
            if let Some(args) = a
                .strip_prefix("@available(")
                .and_then(|s| s.strip_suffix(')'))
            {
                parse_available(args, &mut avail);
            }
        }
        let disfavored = attrs.iter().any(|a| a.contains("_disfavoredOverload"))
            || rest.contains("@_disfavoredOverload");
        let kw = words
            .split(|c: char| c.is_whitespace() || c == '(' || c == '<' || c == '?')
            .next()
            .unwrap_or("");
        let body = words[kw.len()..].trim_start();
        let new_depth = depth + delta;
        let mut frame = None;
        // 所在扩展或类型的信息
        let parent = stack
            .iter()
            .rev()
            .find(|(f, _)| !matches!(f, Frame::Body))
            .map(|(f, _)| f.clone());
        let mut outer_avail = AvailAttr::default();
        for (f, _) in &stack {
            match f {
                Frame::Ext { avail, .. } | Frame::Type { avail, .. } => outer_avail.merge(avail),
                Frame::Body => {}
            }
        }
        outer_avail.merge(&avail);
        let at_member_level = matches!(
            stack.last(),
            Some((Frame::Ext { .. } | Frame::Type { .. }, _))
        );
        match kw {
            "extension" => {
                let header = normalize(body.trim_end_matches('{').trim());
                let (target, wh) = match header.find(" where ") {
                    Some(i) => (header[..i].to_string(), header[i + 7..].to_string()),
                    None => (header.clone(), String::new()),
                };
                let mut generics = HashMap::new();
                let self_type = parse_where(&wh, &mut generics);
                // `extension Text : View`形式的遵循
                let (target, conformances) = match target.split_once(':') {
                    Some((t, c)) => (t.trim().to_string(), c.to_string()),
                    None => (target, String::new()),
                };
                if conformances
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .any(|w| matches!(w, "View" | "Shape" | "InsettableShape"))
                {
                    result.view_types.insert(target.clone());
                }
                frame = Some(Frame::Ext {
                    target: target.trim().to_string(),
                    self_type,
                    avail: outer_avail.clone(),
                    generics,
                });
            }
            "struct" | "enum" | "class" | "actor" | "protocol" => {
                let header = normalize(body.trim_end_matches('{').trim());
                let name_end = header.find(['<', ':', ' ']).unwrap_or(header.len());
                let local = header[..name_end].to_string();
                let mut generics = HashMap::new();
                if let Some(lt) = header.find('<')
                    && lt == name_end
                    && let Some(gt) = header[lt..].find('>')
                {
                    parse_generics(&header[lt + 1..lt + gt], &mut generics);
                }
                if let Some(wi) = header.find(" where ") {
                    parse_where(&header[wi + 7..], &mut generics);
                }
                let conformances = header[name_end..].split(" where ").next().unwrap_or("");
                let prefix = match &parent {
                    Some(Frame::Type { name, .. }) => format!("{name}."),
                    Some(Frame::Ext { target, .. }) => format!("{target}."),
                    _ => String::new(),
                };
                let name = format!("{prefix}{local}");
                if kw == "protocol" {
                    result.protocols.insert(name.clone());
                }
                if kw != "protocol"
                    && conformances
                        .split(|c: char| c == ',' || c == ':' || c.is_whitespace())
                        .any(|w| matches!(w, "View" | "Shape" | "InsettableShape"))
                {
                    result.view_types.insert(name.clone());
                }
                result.type_generics.insert(name.clone(), generics.clone());
                frame = Some(Frame::Type {
                    name,
                    avail: outer_avail.clone(),
                    generics,
                });
            }
            "init" | "func" if at_member_level => {
                let sig = normalize(if kw == "init" { words } else { body });
                if let Some((name, generic_list, params, rest)) = split_signature(&sig) {
                    let name = if kw == "init" {
                        "init".to_string()
                    } else {
                        name
                    };
                    let mut generics = match &parent {
                        Some(Frame::Ext { generics, .. }) | Some(Frame::Type { generics, .. }) => {
                            generics.clone()
                        }
                        _ => HashMap::new(),
                    };
                    // 类型自身的泛型约束
                    if let Some(Frame::Ext { target, .. }) = &parent {
                        for (f, _) in &stack {
                            if let Frame::Type {
                                name, generics: g, ..
                            } = f
                                && name == target
                            {
                                for (k, v) in g {
                                    generics
                                        .entry(k.clone())
                                        .or_default()
                                        .extend(v.iter().cloned());
                                }
                            }
                        }
                    }
                    parse_generics(&generic_list, &mut generics);
                    let (ret, wh) = split_rest(&rest);
                    parse_where(&wh, &mut generics);
                    let kind = match (&parent, kw, is_static) {
                        (Some(Frame::Ext { target, .. }), "func", false) if target == "View" => {
                            Some(DeclKind::Modifier)
                        }
                        (
                            Some(Frame::Ext {
                                target, self_type, ..
                            }),
                            "func",
                            true,
                        ) => Some(DeclKind::Member {
                            owner: target.clone(),
                            protocol: self_type.is_some(),
                        }),
                        (Some(Frame::Type { name, .. }), "func", true) => Some(DeclKind::Member {
                            owner: name.clone(),
                            protocol: false,
                        }),
                        (Some(Frame::Ext { target, .. }), "init", _) => {
                            Some(DeclKind::Init(target.clone()))
                        }
                        (Some(Frame::Type { name, .. }), "init", _) => {
                            Some(DeclKind::Init(name.clone()))
                        }
                        _ => None,
                    };
                    let name = name.trim_matches('`').to_string();
                    if let Some(kind) = kind
                        && !name.starts_with('_')
                        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                    {
                        result.decls.push(RawDecl {
                            kind,
                            name,
                            params: Some(parse_params(&params)),
                            ret,
                            generics,
                            avail: outer_avail.clone(),
                            disfavored,
                        });
                    }
                }
                if delta > 0 {
                    frame = Some(Frame::Body);
                }
            }
            "var" | "let" if at_member_level && is_static => {
                let decl = normalize(body);
                if let Some((name, ty)) = decl.split_once(':') {
                    let name = name.trim().to_string();
                    let ty = ty
                        .split('{')
                        .next()
                        .unwrap_or("")
                        .split('=')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    let owner = match &parent {
                        Some(Frame::Ext {
                            target, self_type, ..
                        }) => Some((target.clone(), self_type.is_some())),
                        Some(Frame::Type { name, .. }) => Some((name.clone(), false)),
                        _ => None,
                    };
                    let name = name.trim_matches('`').to_string();
                    if let Some((owner, protocol)) = owner
                        && !name.starts_with('_')
                        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                    {
                        result.decls.push(RawDecl {
                            kind: DeclKind::Member { owner, protocol },
                            name,
                            params: None,
                            ret: ty,
                            generics: HashMap::new(),
                            avail: outer_avail.clone(),
                            disfavored,
                        });
                    }
                }
                if delta > 0 {
                    frame = Some(Frame::Body);
                }
            }
            "case" if at_member_level => {
                if let Some(Frame::Type { name, .. }) = &parent {
                    for c in split_top(body, ',') {
                        let c = c.trim();
                        // 带关联值的成员暂不支持
                        if c.is_empty() || c.contains('(') || c.contains('=') {
                            continue;
                        }
                        let c = c.trim_matches('`').to_string();
                        result.decls.push(RawDecl {
                            kind: DeclKind::Member {
                                owner: name.clone(),
                                protocol: false,
                            },
                            name: c,
                            params: None,
                            ret: name.clone(),
                            generics: HashMap::new(),
                            avail: outer_avail.clone(),
                            disfavored,
                        });
                    }
                }
            }
            _ => {
                if delta > 0 {
                    frame = Some(Frame::Body);
                }
            }
        }
        if let Some(f) = frame
            && delta > 0
        {
            stack.push((f, new_depth));
        }
        depth = new_depth;
        while stack.last().is_some_and(|(_, d)| *d > depth) {
            stack.pop();
        }
    }
    result
}

/// 类型映射所需的上下文
struct Mapper<'a> {
    token_types: &'a HashSet<String>,
    style_protocols: &'a HashSet<String>,
}

fn strip_type_attrs(t: &str) -> String {
    let mut s = t.trim().to_string();
    loop {
        let trimmed = s.trim_start().to_string();
        if trimmed.starts_with('@') {
            let (_, rest) = split_attrs(&trimmed);
            s = rest.to_string();
            continue;
        }
        if let Some(r) = ["sending ", "inout ", "isolated "]
            .iter()
            .find_map(|w| trimmed.strip_prefix(w))
        {
            s = r.to_string();
            continue;
        }
        break;
    }
    s.trim().to_string()
}

impl Mapper<'_> {
    fn map(
        &self,
        ty: &str,
        builder: bool,
        generics: &HashMap<String, Vec<String>>,
    ) -> Result<ParamType, String> {
        let t = strip_type_attrs(ty);
        if t.starts_with("inout") {
            return Err("inout参数".into());
        }
        if let Some(inner) = t.strip_suffix('?') {
            let inner = inner.trim_start_matches('(').trim_end_matches(')');
            return Ok(ParamType::Optional(Box::new(
                self.map(inner, builder, generics)?,
            )));
        }
        if let Some(inner) = t
            .strip_prefix("Optional<")
            .and_then(|s| s.strip_suffix('>'))
        {
            return Ok(ParamType::Optional(Box::new(
                self.map(inner, builder, generics)?,
            )));
        }
        // 闭包
        if t.starts_with('(') && t.contains("->") {
            let Some((args, ret)) = t.split_once("->") else {
                return Err("闭包".into());
            };
            let args = args.trim();
            let is_async = args.contains("async");
            let args = args.replace("async", "").replace("throws", "");
            let args = args.trim();
            if args != "()" {
                return Err("带参数的闭包".into());
            }
            let ret = ret.trim();
            if ret == "Void" || ret == "()" {
                return Ok(ParamType::Action { is_async });
            }
            let is_view = builder
                || ret == "some View"
                || generics
                    .get(ret)
                    .is_some_and(|c| c.iter().any(|x| x == "View"));
            if is_view {
                return Ok(ParamType::Content);
            }
            return Err(format!("返回{ret}的闭包"));
        }
        match t.as_str() {
            "Bool" => return Ok(ParamType::Bool),
            "Int" => return Ok(ParamType::Int),
            "Double" | "CGFloat" | "TimeInterval" => return Ok(ParamType::Double),
            "Date" => return Ok(ParamType::Date),
            "Float" => return Ok(ParamType::Float),
            "String" => return Ok(ParamType::String),
            "LocalizedStringKey" => return Ok(ParamType::Text(TextKind::LocalizedKey)),
            "URL" => return Ok(ParamType::Url),
            "Text" => return Ok(ParamType::Text(TextKind::Text)),
            _ => {}
        }
        if let Some(inner) = t.strip_prefix("Binding<").and_then(|s| s.strip_suffix('>')) {
            let inner = self.map(inner, false, generics)?;
            return match inner {
                ParamType::Bool
                | ParamType::Int
                | ParamType::Double
                | ParamType::Float
                | ParamType::String
                | ParamType::Date
                | ParamType::Number => Ok(ParamType::Binding(Box::new(inner))),
                ParamType::Optional(ref i)
                    if matches!(**i, ParamType::String | ParamType::Int | ParamType::Double) =>
                {
                    Ok(ParamType::Binding(Box::new(inner)))
                }
                ParamType::Any => Ok(ParamType::Binding(Box::new(inner))),
                _ => Err("非基本类型的Binding".into()),
            };
        }
        let constraints: Option<Vec<String>> = if let Some(p) = t.strip_prefix("some ") {
            Some(
                p.trim_matches(['(', ')'])
                    .split('&')
                    .map(|x| x.trim().to_string())
                    .collect(),
            )
        } else {
            generics.get(&t).cloned()
        };
        if let Some(cons) = constraints {
            if cons.iter().any(|c| c == "StringProtocol") {
                return Ok(ParamType::Text(TextKind::String));
            }
            if cons.iter().any(|c| c == "View") {
                return Ok(ParamType::View);
            }
            if cons.iter().any(|c| c == "BinaryFloatingPoint") {
                return Ok(ParamType::Double);
            }
            if cons
                .iter()
                .any(|c| c == "BinaryInteger" || c == "FixedWidthInteger")
            {
                return Ok(ParamType::Int);
            }
            if cons.iter().any(|c| c == "Strideable") {
                return Ok(ParamType::Number);
            }
            if cons.iter().any(|c| c == "Hashable" || c == "Equatable")
                && cons
                    .iter()
                    .all(|c| matches!(c.as_str(), "Hashable" | "Sendable" | "Equatable"))
            {
                return Ok(ParamType::Any);
            }
            for c in &cons {
                if self.style_protocols.contains(c) {
                    return Ok(ParamType::Style(c.clone()));
                }
            }
            return Err(format!("泛型约束{}", cons.join(" & ")));
        }
        // 泛型数值的步长，例如`V.Stride`
        if t.ends_with(".Stride") {
            return Ok(ParamType::Number);
        }
        if self.token_types.contains(&t) {
            return Ok(ParamType::Token(t));
        }
        Err(format!("不支持的类型{t}"))
    }

    fn params(
        &self,
        raw: &[RawParam],
        generics: &HashMap<String, Vec<String>>,
    ) -> Result<Vec<Param>, String> {
        let mut out = Vec::new();
        for p in raw {
            match self.map(&p.ty, p.builder, generics) {
                Ok(ty) => out.push(Param {
                    label: p.label.clone(),
                    ty,
                    default: p.default,
                }),
                // 有默认值的参数可以省略，不影响整个重载
                Err(_) if p.default => {}
                Err(e) => return Err(format!("参数{}：{e}", p.label.as_deref().unwrap_or("_"))),
            }
        }
        Ok(out)
    }
}

/// (所属类型, 成员名, 参数签名)
type MemberKey = (String, String, String);
/// (参数, 可用性, 是否为协议上的成员)
type MemberEntry = (Option<Vec<Param>>, Availability, bool);

/// 生成目录，同时返回被跳过的重载
pub fn generate(files: &[InterfaceFile], sdk: &str) -> (Catalog, Vec<Skipped>) {
    let parsed: Vec<(String, FileResult)> = files
        .iter()
        .map(|f| (f.os.clone(), parse_file(&f.text)))
        .collect();
    let mut view_types = HashSet::new();
    let mut protocols = HashSet::new();
    let mut token_types = HashSet::new();
    let mut style_protocols = HashSet::new();
    for (_, r) in &parsed {
        view_types.extend(r.view_types.iter().cloned());
        protocols.extend(r.protocols.iter().cloned());
        for d in &r.decls {
            if let DeclKind::Member { owner, protocol } = &d.kind {
                if *protocol || protocols.contains(owner) {
                    style_protocols.insert(owner.clone());
                } else {
                    token_types.insert(owner.clone());
                }
            }
        }
    }
    let mapper = Mapper {
        token_types: &token_types,
        style_protocols: &style_protocols,
    };
    let mut catalog = Catalog {
        sdk: sdk.to_string(),
        ..Catalog::default()
    };
    let mut skipped = Vec::new();
    // 键为声明的唯一标识，值为(签名, 各系统可用性)
    let mut views: BTreeMap<(String, String), (Vec<Param>, Availability)> = BTreeMap::new();
    let mut modifiers: BTreeMap<(String, String), (Vec<Param>, Availability)> = BTreeMap::new();
    let mut members: BTreeMap<MemberKey, MemberEntry> = BTreeMap::new();
    let mut seen_skip = HashSet::new();
    let mut type_generics: HashMap<String, HashMap<String, Vec<String>>> = HashMap::new();
    for (_, r) in &parsed {
        for (k, v) in &r.type_generics {
            type_generics.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    for (os, r) in &parsed {
        for d in &r.decls {
            // 扩展中的构造器继承类型声明上的泛型约束
            let mut generics = d.generics.clone();
            if let DeclKind::Init(owner) = &d.kind
                && let Some(g) = type_generics.get(owner)
            {
                for (k, v) in g {
                    generics
                        .entry(k.clone())
                        .or_default()
                        .extend(v.iter().cloned());
                }
            }
            let params = match &d.params {
                Some(raw) => match mapper.params(raw, &generics) {
                    Ok(p) => Some(p),
                    Err(reason) => {
                        let name = match &d.kind {
                            DeclKind::Init(t) => t.clone(),
                            DeclKind::Modifier => format!(".{}", d.name),
                            DeclKind::Member { owner, .. } => format!("{owner}.{}", d.name),
                        };
                        if seen_skip.insert((name.clone(), reason.clone())) {
                            skipped.push(Skipped { name, reason });
                        }
                        continue;
                    }
                },
                None => None,
            };
            let key_params = format!("{:?}", params);
            let availability = d.avail.for_os(os);
            let insert = |entry: &mut Availability| {
                entry.insert(os.clone(), availability.clone());
            };
            match &d.kind {
                DeclKind::Init(owner)
                    if view_types.contains(owner)
                        && !owner.starts_with('_')
                        && !owner.contains('.') =>
                {
                    let e = views.entry((owner.clone(), key_params)).or_insert_with(|| {
                        (params.clone().unwrap_or_default(), Availability::new())
                    });
                    insert(&mut e.1);
                }
                DeclKind::Init(_) => {}
                DeclKind::Modifier if d.ret.contains("View") => {
                    let e = modifiers
                        .entry((d.name.clone(), key_params))
                        .or_insert_with(|| {
                            (params.clone().unwrap_or_default(), Availability::new())
                        });
                    insert(&mut e.1);
                }
                DeclKind::Modifier => {}
                DeclKind::Member { owner, protocol } => {
                    let e = members
                        .entry((owner.clone(), d.name.clone(), key_params))
                        .or_insert_with(|| {
                            (
                                params.clone(),
                                Availability::new(),
                                *protocol || style_protocols.contains(owner),
                            )
                        });
                    insert(&mut e.1);
                }
            }
            let _ = d.disfavored;
        }
    }
    for ((name, _), (params, availability)) in views {
        catalog.views.entry(name).or_default().push(Signature {
            params,
            availability,
        });
    }
    for ((name, _), (params, availability)) in modifiers {
        catalog.modifiers.entry(name).or_default().push(Signature {
            params,
            availability,
        });
    }
    for ((owner, name, _), (params, availability, protocol)) in members {
        let table = if protocol {
            &mut catalog.protocols
        } else {
            &mut catalog.types
        };
        table
            .entry(owner)
            .or_default()
            .entry(name)
            .or_default()
            .push(Member {
                params,
                availability,
            });
    }
    (catalog, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
@available(iOS 13.0, macOS 10.15, *)
extension SwiftUICore::View {
  @inlinable nonisolated public func padding(_ edges: SwiftUICore::Edge.SwiftUICore::Set = .all, _ length: CoreFoundation::CGFloat? = nil) -> some SwiftUICore::View {
        return modifier(_PaddingLayout(edges: edges, insets: nil))
    }
  @available(iOS 26.0, macOS 26.0, *)
  @available(watchOS, unavailable)
  nonisolated public func glassEffect(_ glass: SwiftUICore::Glass = .regular, in shape: some Shape = DefaultGlassEffectShape()) -> some SwiftUICore::View
  nonisolated public func onTapGesture(count: Swift.Int = 1, perform action: @escaping () -> Swift.Void) -> some SwiftUICore.View
  nonisolated public func refreshable(action: @escaping @Sendable () async -> Swift.Void) -> some SwiftUICore.View
  nonisolated public func overlay<V>(alignment: SwiftUICore.Alignment = .center, @SwiftUICore.ViewBuilder content: () -> V) -> some SwiftUICore.View where V : SwiftUICore.View
  nonisolated public func onChange<V>(of value: V, perform action: @escaping (V) -> Swift.Void) -> some SwiftUICore.View where V : Swift.Equatable
}
@frozen public struct Edge {
  @frozen public struct Set : Swift.OptionSet {
    public static let all: SwiftUICore::Edge.Set
    public static let horizontal: SwiftUICore::Edge.Set
  }
}
public struct Glass {
  public static var regular: SwiftUICore::Glass {
    get
  }
}
public protocol Shape {
}
extension SwiftUICore::Shape where Self == SwiftUICore::Capsule {
  public static var capsule: SwiftUICore::Capsule {
    get
  }
}
public struct Alignment {
  public static let center: SwiftUICore::Alignment
}
@available(iOS 16.0, *)
@available(tvOS, unavailable)
public struct ShareLink<Label> : SwiftUICore.View where Label : SwiftUICore.View {
  nonisolated public init<S>(_ title: S, item: Swift.String) where S : Swift.StringProtocol
  nonisolated public init(item: Swift.String, @SwiftUICore.ContentBuilder label: () -> Label)
}
"#;

    #[test]
    fn parses_interface() {
        let files = [
            InterfaceFile {
                os: "ios".into(),
                text: SAMPLE.into(),
            },
            InterfaceFile {
                os: "macos".into(),
                text: SAMPLE.replace("ShareLink", "Other"),
            },
        ];
        let (c, skipped) = generate(&files, "27.1");
        let padding = &c.modifiers["padding"][0];
        assert_eq!(padding.params.len(), 2);
        assert_eq!(padding.params[0].ty, ParamType::Token("Edge.Set".into()));
        assert_eq!(
            padding.params[1].ty,
            ParamType::Optional(Box::new(ParamType::Double))
        );
        let glass = &c.modifiers["glassEffect"][0];
        assert_eq!(glass.params[1].ty, ParamType::Style("Shape".into()));
        assert_eq!(
            glass.availability["ios"],
            OsAvailability::Available {
                introduced: Version::parse("26.0"),
                deprecated: None
            }
        );
        assert_eq!(
            c.modifiers["onTapGesture"][0].params[1].ty,
            ParamType::Action { is_async: false }
        );
        assert_eq!(
            c.modifiers["refreshable"][0].params[0].ty,
            ParamType::Action { is_async: true }
        );
        assert_eq!(c.modifiers["overlay"][0].params[1].ty, ParamType::Content);
        assert!(!c.modifiers.contains_key("onChange"));
        assert!(skipped.iter().any(|s| s.name == ".onChange"));
        let share = &c.views["ShareLink"];
        assert_eq!(share.len(), 2);
        assert_eq!(share[0].params[0].ty, ParamType::Text(TextKind::String));
        assert!(share.iter().all(|s| !s.availability.contains_key("macos")));
        assert!(c.types["Edge.Set"].contains_key("horizontal"));
        assert!(c.protocols["Shape"].contains_key("capsule"));
    }
}
