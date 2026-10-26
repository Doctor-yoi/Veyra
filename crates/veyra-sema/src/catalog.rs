// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 内置组件与修饰符的参数表

use veyra_ir::{
    AndroidElement, AndroidModifier, Builtin, ElementKind, IosElement, IosModifier, ModifierKind,
    Type,
};

/// 参数种类
#[derive(Clone, Debug)]
pub(crate) enum ArgKind {
    /// 显示文本：String表达式或loc/plural
    Text,
    /// 指定类型的表达式
    Expr(Type),
    /// 数值，整数字面量会提升为Double
    Number,
    /// 任意元素类型的列表
    List,
    Action,
    Binding(Type),
    /// `.id`字段路径
    KeyPath,
    /// 固定取值的样式记号
    Token(&'static [&'static str]),
    /// 数值或`.infinity`
    Dimension,
}

#[derive(Clone, Debug)]
pub(crate) struct ArgSpec {
    /// 位置参数为`_`
    pub label: &'static str,
    pub kind: ArgKind,
    pub required: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Children {
    None,
    Required,
    /// 需要子节点且闭包带一个元素参数
    Item,
}

pub(crate) struct ElementSpec {
    pub args: Vec<ArgSpec>,
    pub children: Children,
}

pub(crate) struct ModifierSpec {
    pub args: Vec<ArgSpec>,
    /// `None`表示可用于任意组件
    pub applies_to: Option<&'static [Builtin]>,
}

const fn req(label: &'static str, kind: ArgKind) -> ArgSpec {
    ArgSpec {
        label,
        kind,
        required: true,
    }
}

const fn opt(label: &'static str, kind: ArgKind) -> ArgSpec {
    ArgSpec {
        label,
        kind,
        required: false,
    }
}

pub(crate) const H_ALIGN: &[&str] = &["leading", "center", "trailing"];
pub(crate) const V_ALIGN: &[&str] = &["top", "center", "bottom"];
pub(crate) const ALIGN: &[&str] = &[
    "topLeading",
    "top",
    "topTrailing",
    "leading",
    "center",
    "trailing",
    "bottomLeading",
    "bottom",
    "bottomTrailing",
];
pub(crate) const FONTS: &[&str] = &[
    "largeTitle",
    "title",
    "title2",
    "title3",
    "headline",
    "subheadline",
    "body",
    "callout",
    "footnote",
    "caption",
];
pub(crate) const COLORS: &[&str] = &[
    "primary",
    "secondary",
    "accent",
    "red",
    "orange",
    "yellow",
    "green",
    "blue",
    "purple",
    "gray",
    "white",
    "black",
    "clear",
];

pub(crate) fn element(builtin: Builtin) -> ElementSpec {
    use ArgKind::*;
    let (args, children) = match builtin {
        Builtin::Column => (
            vec![opt("spacing", Number), opt("alignment", Token(H_ALIGN))],
            Children::Required,
        ),
        Builtin::Row => (
            vec![opt("spacing", Number), opt("alignment", Token(V_ALIGN))],
            Children::Required,
        ),
        Builtin::Stack => (vec![opt("alignment", Token(ALIGN))], Children::Required),
        Builtin::ScrollView => (vec![], Children::Required),
        Builtin::Text => (vec![req("_", Text)], Children::None),
        Builtin::Button => (vec![req("_", Text), req("action", Action)], Children::None),
        Builtin::TextField | Builtin::SecureField => (
            vec![req("_", Text), req("text", Binding(Type::String))],
            Children::None,
        ),
        Builtin::Toggle => (
            vec![req("_", Text), req("isOn", Binding(Type::Bool))],
            Children::None,
        ),
        Builtin::Slider => (
            vec![
                req("value", Binding(Type::Double)),
                opt("min", Number),
                opt("max", Number),
            ],
            Children::None,
        ),
        Builtin::Image => (vec![req("_", Expr(Type::String))], Children::None),
        Builtin::Spacer | Builtin::Divider => (vec![], Children::None),
        Builtin::Progress => (vec![opt("value", Number)], Children::None),
        Builtin::List | Builtin::ForEach => {
            (vec![req("_", List), req("key", KeyPath)], Children::Item)
        }
    };
    ElementSpec { args, children }
}

const TEXT_ONLY: &[Builtin] = &[Builtin::Text];
const CONTROLS: &[Builtin] = &[
    Builtin::Button,
    Builtin::TextField,
    Builtin::SecureField,
    Builtin::Toggle,
    Builtin::Slider,
];

const AUTOCAPITALIZATION: &[&str] = &["never", "words", "sentences", "characters"];
const LIST_STYLES: &[&str] = &["plain", "inset", "insetGrouped", "grouped", "sidebar"];
const KEYBOARD_DISMISS: &[&str] = &["immediately", "interactively", "never"];
const TEXT_INPUTS: &[Builtin] = &[Builtin::TextField, Builtin::SecureField];
const LISTS: &[Builtin] = &[Builtin::List];
const SCROLLABLES: &[Builtin] = &[Builtin::ScrollView, Builtin::List];

/// 平台专属组件，`platform`为`ios`或`android`
pub(crate) fn platform_element(platform: &str, name: &str) -> Option<(ElementKind, ElementSpec)> {
    use ArgKind::*;
    let (kind, args) = match (platform, name) {
        ("ios", "ShareLink") => (
            ElementKind::Ios(IosElement::ShareLink),
            vec![req("_", Text), req("item", Expr(Type::String))],
        ),
        ("android", "FloatingActionButton") => (
            ElementKind::Android(AndroidElement::FloatingActionButton),
            vec![req("_", Text), req("action", Action)],
        ),
        _ => return None,
    };
    Some((
        kind,
        ElementSpec {
            args,
            children: Children::None,
        },
    ))
}

pub(crate) const IOS_ELEMENTS: &[&str] = &["ShareLink"];
pub(crate) const ANDROID_ELEMENTS: &[&str] = &["FloatingActionButton"];
pub(crate) const IOS_MODIFIERS: &[&str] = &[
    "glassEffect",
    "navigationTitle",
    "textInputAutocapitalization",
    "listStyle",
    "scrollDismissesKeyboard",
];
pub(crate) const ANDROID_MODIFIERS: &[&str] = &["imePadding", "systemBarsPadding", "testTag"];

/// 平台专属修饰符
pub(crate) fn platform_modifier(platform: &str, name: &str) -> Option<ModifierKind> {
    Some(match (platform, name) {
        ("ios", "glassEffect") => ModifierKind::Ios(IosModifier::GlassEffect),
        ("ios", "navigationTitle") => ModifierKind::Ios(IosModifier::NavigationTitle),
        ("ios", "textInputAutocapitalization") => {
            ModifierKind::Ios(IosModifier::TextInputAutocapitalization)
        }
        ("ios", "listStyle") => ModifierKind::Ios(IosModifier::ListStyle),
        ("ios", "scrollDismissesKeyboard") => {
            ModifierKind::Ios(IosModifier::ScrollDismissesKeyboard)
        }
        ("android", "imePadding") => ModifierKind::Android(AndroidModifier::ImePadding),
        ("android", "systemBarsPadding") => {
            ModifierKind::Android(AndroidModifier::SystemBarsPadding)
        }
        ("android", "testTag") => ModifierKind::Android(AndroidModifier::TestTag),
        _ => return None,
    })
}

/// 以组件参数形式生成、无法按debug条件切换的修饰符
pub(crate) fn is_parameter_modifier(kind: ModifierKind) -> bool {
    matches!(
        kind,
        ModifierKind::Font | ModifierKind::Foreground | ModifierKind::Disabled
    )
}

pub(crate) fn modifier(kind: ModifierKind) -> ModifierSpec {
    use ArgKind::*;
    let (args, applies_to) = match kind {
        ModifierKind::Ios(m) => match m {
            IosModifier::GlassEffect => (vec![], None),
            IosModifier::NavigationTitle => (vec![req("_", Text)], None),
            IosModifier::TextInputAutocapitalization => {
                (vec![req("_", Token(AUTOCAPITALIZATION))], Some(TEXT_INPUTS))
            }
            IosModifier::ListStyle => (vec![req("_", Token(LIST_STYLES))], Some(LISTS)),
            IosModifier::ScrollDismissesKeyboard => {
                (vec![req("_", Token(KEYBOARD_DISMISS))], Some(SCROLLABLES))
            }
        },
        ModifierKind::Android(m) => match m {
            AndroidModifier::ImePadding | AndroidModifier::SystemBarsPadding => (vec![], None),
            AndroidModifier::TestTag => (vec![req("_", Expr(Type::String))], None),
        },
        ModifierKind::Padding => (vec![opt("_", Number)], None),
        ModifierKind::Font => (vec![req("_", Token(FONTS))], Some(TEXT_ONLY)),
        ModifierKind::Foreground => (vec![req("_", Token(COLORS))], Some(TEXT_ONLY)),
        ModifierKind::Background => (vec![req("_", Token(COLORS))], None),
        ModifierKind::Frame => (
            vec![
                opt("width", Number),
                opt("height", Number),
                opt("maxWidth", Dimension),
                opt("maxHeight", Dimension),
            ],
            None,
        ),
        ModifierKind::Disabled => (vec![req("_", Expr(Type::Bool))], Some(CONTROLS)),
        ModifierKind::Hidden => (vec![req("_", Expr(Type::Bool))], None),
        ModifierKind::OnTap => (vec![req("_", Action)], None),
        ModifierKind::AccessibilityLabel => (vec![req("_", Text)], None),
    };
    ModifierSpec { args, applies_to }
}
