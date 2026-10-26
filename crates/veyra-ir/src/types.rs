// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

use std::fmt;

/// 值类型。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Bool,
    Int,
    Double,
    String,
    Date,
    List(Box<Type>),
    Optional(Box<Type>),
    /// 只出现在repository与store的返回值中
    Stream(Box<Type>),
    Enum(String),
    Model(String),
    /// 仅在出错时出现，用于抑制连锁报错
    Unknown,
}

impl Type {
    pub fn list(inner: Type) -> Type {
        Type::List(Box::new(inner))
    }

    pub fn optional(inner: Type) -> Type {
        match inner {
            Type::Optional(_) => inner,
            other => Type::Optional(Box::new(other)),
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Int | Type::Double)
    }

    pub fn is_optional(&self) -> bool {
        matches!(self, Type::Optional(_))
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Type::Unknown)
    }

    /// 去掉一层可选。
    pub fn unwrap_optional(&self) -> &Type {
        match self {
            Type::Optional(inner) => inner,
            other => other,
        }
    }

    /// 列表元素类型。
    pub fn element(&self) -> Option<&Type> {
        match self {
            Type::List(inner) => Some(inner),
            _ => None,
        }
    }

    /// 是否包含Stream。
    pub fn contains_stream(&self) -> bool {
        match self {
            Type::Stream(_) => true,
            Type::List(inner) | Type::Optional(inner) => inner.contains_stream(),
            _ => false,
        }
    }

    /// 类型兼容判断，Unknown与任何类型兼容。
    pub fn accepts(&self, other: &Type) -> bool {
        if self.is_unknown() || other.is_unknown() {
            return true;
        }
        match (self, other) {
            (Type::Optional(a), Type::Optional(b)) => a.accepts(b),
            (Type::Optional(a), b) => a.accepts(b),
            (Type::List(a), Type::List(b)) | (Type::Stream(a), Type::Stream(b)) => a.accepts(b),
            (a, b) => a == b,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Bool => f.write_str("Bool"),
            Type::Int => f.write_str("Int"),
            Type::Double => f.write_str("Double"),
            Type::String => f.write_str("String"),
            Type::Date => f.write_str("Date"),
            Type::List(t) => write!(f, "List<{t}>"),
            Type::Optional(t) => write!(f, "{t}?"),
            Type::Stream(t) => write!(f, "Stream<{t}>"),
            Type::Enum(n) | Type::Model(n) => f.write_str(n),
            Type::Unknown => f.write_str("{unknown}"),
        }
    }
}
