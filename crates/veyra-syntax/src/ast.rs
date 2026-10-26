// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! Veyra语法树。所有节点都带有源码区间，供诊断与格式化使用

use veyra_diagnostics::{FileId, Span};

#[derive(Clone, Debug, PartialEq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

/// 单个源文件
#[derive(Clone, Debug, PartialEq)]
pub struct File {
    pub id: FileId,
    /// 相对于项目根目录的路径，由调用方填写，用于诊断中的import建议
    pub path: String,
    pub imports: Vec<Import>,
    pub items: Vec<Node<Item>>,
}

/// `import "path.vey"`
#[derive(Clone, Debug, PartialEq)]
pub struct Import {
    /// 相对于当前文件的路径
    pub path: String,
    pub span: Span,
    /// 解析后的文件，由调用方填写
    pub resolved: Option<FileId>,
}

/// 可被`#if`包裹的节点
#[derive(Clone, Debug, PartialEq)]
pub enum Node<T> {
    Item(T),
    Cfg(CfgGroup<T>),
}

/// `#if ... #elseif ... #else ... #endif`整体
#[derive(Clone, Debug, PartialEq)]
pub struct CfgGroup<T> {
    pub branches: Vec<CfgBranch<T>>,
    pub span: Span,
    pub end_span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CfgBranch<T> {
    /// `None`表示`#else`
    pub cond: Option<CfgExpr>,
    pub items: Vec<Node<T>>,
    /// 指令本身的区间
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CfgExpr {
    Ident(Ident),
    Not(Box<CfgExpr>, Span),
    And(Box<CfgExpr>, Box<CfgExpr>),
    Or(Box<CfgExpr>, Box<CfgExpr>),
    Paren(Box<CfgExpr>, Span),
}

impl CfgExpr {
    pub fn span(&self) -> Span {
        match self {
            CfgExpr::Ident(i) => i.span,
            CfgExpr::Not(_, s) | CfgExpr::Paren(_, s) => *s,
            CfgExpr::And(a, b) | CfgExpr::Or(a, b) => a.span().to(b.span()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Enum(EnumDecl),
    Model(ModelDecl),
    Entity(EntityDecl),
    Repository(RepositoryDecl),
    Store(StoreDecl),
    Settings(FieldsDecl),
    Secure(FieldsDecl),
    Component(ComponentDecl),
    Screen(ScreenDecl),
}

impl Item {
    pub fn name(&self) -> &Ident {
        match self {
            Item::Enum(d) => &d.name,
            Item::Model(d) => &d.name,
            Item::Entity(d) => &d.name,
            Item::Repository(d) => &d.name,
            Item::Store(d) => &d.name,
            Item::Settings(d) | Item::Secure(d) => &d.name,
            Item::Component(d) => &d.name,
            Item::Screen(d) => &d.name,
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Item::Enum(d) => d.span,
            Item::Model(d) => d.span,
            Item::Entity(d) => d.span,
            Item::Repository(d) => d.span,
            Item::Store(d) => d.span,
            Item::Settings(d) | Item::Secure(d) => d.span,
            Item::Component(d) => d.span,
            Item::Screen(d) => d.span,
        }
    }

    /// 声明关键字
    pub fn keyword(&self) -> &'static str {
        match self {
            Item::Enum(_) => "enum",
            Item::Model(_) => "model",
            Item::Entity(_) => "entity",
            Item::Repository(_) => "repository",
            Item::Store(_) => "store",
            Item::Settings(_) => "settings",
            Item::Secure(_) => "secure",
            Item::Component(_) => "component",
            Item::Screen(_) => "screen",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Attr {
    pub name: Ident,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnumDecl {
    pub name: Ident,
    pub cases: Vec<Node<Ident>>,
    pub span: Span,
}

/// 字段：`name: Type = default @attr`
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: Ident,
    pub ty: TypeExpr,
    pub default: Option<Expr>,
    pub attrs: Vec<Attr>,
    pub span: Span,
}

impl Field {
    pub fn has_attr(&self, name: &str) -> bool {
        self.attrs.iter().any(|a| a.name.name == name)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelDecl {
    pub name: Ident,
    pub fields: Vec<Node<Field>>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EntityDecl {
    pub name: Ident,
    pub attrs: Vec<Attr>,
    pub fields: Vec<Node<Field>>,
    pub span: Span,
}

/// settings与secure共用的结构
#[derive(Clone, Debug, PartialEq)]
pub struct FieldsDecl {
    pub name: Ident,
    pub fields: Vec<Node<Field>>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: Ident,
    pub ty: TypeExpr,
    pub span: Span,
}

/// 方法签名：`name(params) async throws -> Type`
#[derive(Clone, Debug, PartialEq)]
pub struct MethodSig {
    pub name: Ident,
    pub params: Vec<Param>,
    pub is_async: bool,
    pub throws: bool,
    pub ret: Option<TypeExpr>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RepositoryDecl {
    pub name: Ident,
    pub methods: Vec<Node<MethodSig>>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoreDecl {
    pub name: Ident,
    pub entity: Ident,
    pub queries: Vec<Node<QueryDecl>>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryDecl {
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub filter: Option<Expr>,
    pub order: Vec<OrderKey>,
    pub limit: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrderKey {
    pub field: Ident,
    pub descending: bool,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComponentDecl {
    pub name: Ident,
    pub params: Vec<Param>,
    pub body: Vec<Node<ViewNode>>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScreenDecl {
    pub name: Ident,
    pub members: Vec<Node<ScreenMember>>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ScreenMember {
    Inject(InjectDecl),
    State(Block<Field>),
    Computed(Block<Field>),
    Actions(Block<MethodSig>),
    Effects(Block<MethodSig>),
    Lifecycle(Block<LifecycleHook>),
    View(Block<ViewNode>),
}

/// 带关键字的花括号块
#[derive(Clone, Debug, PartialEq)]
pub struct Block<T> {
    pub keyword: Span,
    pub items: Vec<Node<T>>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InjectDecl {
    pub name: Ident,
    pub ty: Ident,
    pub span: Span,
}

/// `onAppear: refresh`
#[derive(Clone, Debug, PartialEq)]
pub struct LifecycleHook {
    pub event: Ident,
    pub action: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeExpr {
    pub kind: TypeExprKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeExprKind {
    Named { name: Ident, args: Vec<TypeExpr> },
    Optional(Box<TypeExpr>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ViewNode {
    Element(Element),
    If(IfNode),
    Native(NativeNode),
}

impl ViewNode {
    pub fn span(&self) -> Span {
        match self {
            ViewNode::Element(e) => e.span,
            ViewNode::If(i) => i.span,
            ViewNode::Native(n) => n.span,
        }
    }
}

/// 组件调用：`Column(spacing: 8) { ... }.padding(16)`
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    /// 例如`["ios", "ShareLink"]`
    pub path: Vec<Ident>,
    pub args: Vec<Arg>,
    pub has_parens: bool,
    pub closure: Option<Closure>,
    pub modifiers: Vec<Node<Modifier>>,
    pub span: Span,
}

impl Element {
    pub fn path_string(&self) -> String {
        path_string(&self.path)
    }
}

pub fn path_string(path: &[Ident]) -> String {
    path.iter()
        .map(|i| i.name.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

#[derive(Clone, Debug, PartialEq)]
pub struct Closure {
    pub params: Vec<Ident>,
    pub body: Vec<Node<ViewNode>>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Modifier {
    pub path: Vec<Ident>,
    pub args: Vec<Arg>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IfNode {
    pub cond: Expr,
    pub then: Vec<Node<ViewNode>>,
    pub then_end: Span,
    pub else_: Option<ElseBranch>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ElseBranch {
    Block(Vec<Node<ViewNode>>, Span),
    If(Box<IfNode>),
}

/// `native ios { """...""" }`
#[derive(Clone, Debug, PartialEq)]
pub struct NativeNode {
    pub platform: Ident,
    pub code: String,
    pub raw: String,
    pub code_span: Span,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Arg {
    pub label: Option<Ident>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
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
    pub fn symbol(self) -> &'static str {
        match self {
            BinaryOp::Coalesce => "??",
            BinaryOp::Or => "||",
            BinaryOp::And => "&&",
            BinaryOp::Eq => "==",
            BinaryOp::Ne => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Rem => "%",
        }
    }

    /// 优先级，数值越大结合越紧
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
    Int(i64),
    /// 保留原始文本以便格式化
    Float(f64, String),
    /// 已处理转义的内容与原始文本
    Str(String, String),
    Bool(bool),
    Nil,
    Ident(String),
    /// `.title`
    ImplicitMember(Ident),
    Member(Box<Expr>, Ident),
    Call(Box<Expr>, Vec<Arg>),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    /// `cond ? a : b`
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    List(Vec<Expr>),
    Paren(Box<Expr>),
    /// 解析失败时的占位
    Error,
}
