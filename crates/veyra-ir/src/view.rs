// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

use indexmap::IndexMap;

use crate::Type;

/// 带类型的纯表达式
#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub ty: Type,
}

impl Expr {
    pub fn new(kind: ExprKind, ty: Type) -> Self {
        Self { kind, ty }
    }

    pub fn unknown() -> Self {
        Self {
            kind: ExprKind::Nil,
            ty: Type::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Neg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Coalesce,
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl BinaryOp {
    pub fn precedence(self) -> u8 {
        match self {
            BinaryOp::Or => 1,
            BinaryOp::And => 2,
            BinaryOp::Eq
            | BinaryOp::Ne
            | BinaryOp::Lt
            | BinaryOp::Le
            | BinaryOp::Gt
            | BinaryOp::Ge => 3,
            BinaryOp::Coalesce => 4,
            BinaryOp::Add | BinaryOp::Sub => 5,
            BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => 6,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Bool(bool),
    Int(i64),
    Double(f64),
    String(String),
    Nil,
    List(Vec<Expr>),
    EnumCase {
        enum_name: String,
        case: String,
    },
    /// screen的state字段
    State(String),
    /// screen的computed字段
    Computed(String),
    /// 闭包参数或组件参数
    Local(String),
    /// 模型字段访问
    Field(Box<Expr>, String),
    Count(Box<Expr>),
    IsEmpty(Box<Expr>),
    First(Box<Expr>),
    /// 字符串包含
    Contains(Box<Expr>, Box<Expr>),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    /// 本地化文案，结果为String
    Loc {
        key: String,
        args: Vec<(String, Expr)>,
    },
    /// 复数文案，`count`同时作为`{count}`占位符
    Plural {
        key: String,
        count: Box<Expr>,
    },
    /// 按用户区域设置格式化为String
    Format {
        value: Box<Expr>,
        style: FormatStyle,
    },
}

/// 日期与时间的详略程度
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateLength {
    Short,
    Medium,
    Long,
    Full,
}

impl DateLength {
    pub const NAMES: [&str; 4] = ["short", "medium", "long", "full"];

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "short" => Some(Self::Short),
            "medium" => Some(Self::Medium),
            "long" => Some(Self::Long),
            "full" => Some(Self::Full),
            _ => None,
        }
    }
}

/// `format()`的样式
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormatStyle {
    Number,
    Percent,
    /// ISO 4217货币代码
    Currency(String),
    Date(DateLength),
    Time(DateLength),
    DateTime(DateLength),
}

/// 命令分发
#[derive(Clone, Debug, PartialEq)]
pub enum ActionRef {
    /// 调用screen的action
    Dispatch {
        action: String,
        args: Vec<(String, Expr)>,
        is_async: bool,
    },
    /// 转发组件的Action参数
    Param(String),
}

/// 双向绑定
#[derive(Clone, Debug, PartialEq)]
pub enum BindingRef {
    /// 绑定到可写state，写入经过setter
    State {
        field: String,
        setter: String,
        ty: Type,
    },
    /// 转发组件的Binding参数
    Param { name: String, ty: Type },
}

/// 组件或修饰符的实参
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Expr(Expr),
    Action(ActionRef),
    Binding(BindingRef),
    /// `.id`形式的字段路径
    KeyPath(String),
    /// `.title`、`.leading`等样式记号
    Token(String),
}

impl Value {
    pub fn as_expr(&self) -> Option<&Expr> {
        match self {
            Value::Expr(e) => Some(e),
            _ => None,
        }
    }

    pub fn as_action(&self) -> Option<&ActionRef> {
        match self {
            Value::Action(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_binding(&self) -> Option<&BindingRef> {
        match self {
            Value::Binding(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_token(&self) -> Option<&str> {
        match self {
            Value::Token(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_key_path(&self) -> Option<&str> {
        match self {
            Value::KeyPath(k) => Some(k),
            _ => None,
        }
    }
}

/// 内置组件
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Builtin {
    Column,
    Row,
    Stack,
    ScrollView,
    Text,
    Button,
    TextField,
    SecureField,
    Toggle,
    Slider,
    Image,
    Spacer,
    Divider,
    Progress,
    List,
    ForEach,
}

impl Builtin {
    pub const ALL: [Builtin; 16] = [
        Builtin::Column,
        Builtin::Row,
        Builtin::Stack,
        Builtin::ScrollView,
        Builtin::Text,
        Builtin::Button,
        Builtin::TextField,
        Builtin::SecureField,
        Builtin::Toggle,
        Builtin::Slider,
        Builtin::Image,
        Builtin::Spacer,
        Builtin::Divider,
        Builtin::Progress,
        Builtin::List,
        Builtin::ForEach,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Builtin::Column => "Column",
            Builtin::Row => "Row",
            Builtin::Stack => "Stack",
            Builtin::ScrollView => "ScrollView",
            Builtin::Text => "Text",
            Builtin::Button => "Button",
            Builtin::TextField => "TextField",
            Builtin::SecureField => "SecureField",
            Builtin::Toggle => "Toggle",
            Builtin::Slider => "Slider",
            Builtin::Image => "Image",
            Builtin::Spacer => "Spacer",
            Builtin::Divider => "Divider",
            Builtin::Progress => "Progress",
            Builtin::List => "List",
            Builtin::ForEach => "ForEach",
        }
    }

    pub fn from_name(name: &str) -> Option<Builtin> {
        Builtin::ALL.into_iter().find(|b| b.name() == name)
    }

    /// 是否为布局容器
    pub fn is_container(self) -> bool {
        matches!(
            self,
            Builtin::Column | Builtin::Row | Builtin::Stack | Builtin::ScrollView
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ElementKind {
    Builtin(Builtin),
    /// 用户声明的component
    Component(String),
}

/// 组件实例
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub kind: ElementKind,
    /// 实参，位置参数的键为`_`
    pub args: IndexMap<String, Value>,
    pub children: Vec<ViewNode>,
    /// List、ForEach的元素变量
    pub item: Option<(String, Type)>,
    pub modifiers: Vec<Modifier>,
}

impl Element {
    pub fn arg(&self, name: &str) -> Option<&Value> {
        self.args.get(name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModifierKind {
    Padding,
    Font,
    Foreground,
    Background,
    Frame,
    Disabled,
    Hidden,
    OnTap,
    AccessibilityLabel,
}

impl ModifierKind {
    pub const ALL: [ModifierKind; 9] = [
        ModifierKind::Padding,
        ModifierKind::Font,
        ModifierKind::Foreground,
        ModifierKind::Background,
        ModifierKind::Frame,
        ModifierKind::Disabled,
        ModifierKind::Hidden,
        ModifierKind::OnTap,
        ModifierKind::AccessibilityLabel,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ModifierKind::Padding => "padding",
            ModifierKind::Font => "font",
            ModifierKind::Foreground => "foreground",
            ModifierKind::Background => "background",
            ModifierKind::Frame => "frame",
            ModifierKind::Disabled => "disabled",
            ModifierKind::Hidden => "hidden",
            ModifierKind::OnTap => "onTap",
            ModifierKind::AccessibilityLabel => "accessibilityLabel",
        }
    }

    pub fn from_name(name: &str) -> Option<ModifierKind> {
        ModifierKind::ALL.into_iter().find(|m| m.name() == name)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Modifier {
    pub kind: ModifierKind,
    pub args: IndexMap<String, Value>,
}

impl Modifier {
    pub fn arg(&self, name: &str) -> Option<&Value> {
        self.args.get(name)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ViewNode {
    Element(Element),
    If {
        cond: Expr,
        then: Vec<ViewNode>,
        else_: Vec<ViewNode>,
    },
}

impl Expr {
    /// 先序遍历表达式树
    pub fn walk(&self, f: &mut dyn FnMut(&Expr)) {
        f(self);
        match &self.kind {
            ExprKind::List(items) => items.iter().for_each(|i| i.walk(f)),
            ExprKind::Field(b, _)
            | ExprKind::Count(b)
            | ExprKind::IsEmpty(b)
            | ExprKind::First(b)
            | ExprKind::Unary(_, b) => b.walk(f),
            ExprKind::Contains(a, b) | ExprKind::Binary(_, a, b) => {
                a.walk(f);
                b.walk(f);
            }
            ExprKind::Conditional(c, a, b) => {
                c.walk(f);
                a.walk(f);
                b.walk(f);
            }
            ExprKind::Loc { args, .. } => args.iter().for_each(|(_, v)| v.walk(f)),
            ExprKind::Plural { count, .. } => count.walk(f),
            ExprKind::Format { value, .. } => value.walk(f),
            _ => {}
        }
    }
}

impl Value {
    pub fn walk(&self, f: &mut dyn FnMut(&Expr)) {
        match self {
            Value::Expr(e) => e.walk(f),
            Value::Action(ActionRef::Dispatch { args, .. }) => {
                args.iter().for_each(|(_, v)| v.walk(f))
            }
            _ => {}
        }
    }
}

impl ViewNode {
    /// 遍历视图树中的全部表达式
    pub fn walk_exprs(&self, f: &mut dyn FnMut(&Expr)) {
        match self {
            ViewNode::Element(el) => {
                el.args.values().for_each(|v| v.walk(f));
                for m in &el.modifiers {
                    m.args.values().for_each(|v| v.walk(f));
                }
                el.children.iter().for_each(|c| c.walk_exprs(f));
            }
            ViewNode::If { cond, then, else_ } => {
                cond.walk(f);
                then.iter().chain(else_).for_each(|c| c.walk_exprs(f));
            }
        }
    }
}
