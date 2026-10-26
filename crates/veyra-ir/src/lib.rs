// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! Veyra语义层中间表示（Semantic MVVM IR）
//!
//! IR是前端与各平台后端之间的契约：
//!
//! - 条件编译已按目标平台裁剪，只剩`debug`这类需要原生保留的条件
//! - 所有名称已解析，所有表达式都带类型
//! - 读取、双向绑定、命令分发三种交互已区分为不同的节点
//!
//! 后端只需把IR映射为对应平台的惯用写法，不再做语义检查

mod l10n;
mod types;
mod view;

pub use l10n::{
    Catalog, CatalogEntry, LocArg, LocArgType, LocString, PLURAL_CATEGORIES, PluralForms, Segment,
    Translation, parse_template,
};
pub use types::Type;
pub use view::*;

/// 目标平台
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Platform {
    Ios,
    Android,
    Harmony,
}

impl Platform {
    pub const ALL: [Platform; 3] = [Platform::Ios, Platform::Android, Platform::Harmony];

    /// 条件编译中使用的名称
    pub fn cfg_name(self) -> &'static str {
        match self {
            Platform::Ios => "ios",
            Platform::Android => "android",
            Platform::Harmony => "harmony",
        }
    }

    pub fn from_cfg_name(name: &str) -> Option<Platform> {
        Platform::ALL.into_iter().find(|p| p.cfg_name() == name)
    }
}

/// 单个目标平台的完整程序
#[derive(Clone, Debug, Default)]
pub struct Program {
    pub enums: Vec<EnumDef>,
    pub models: Vec<ModelDef>,
    pub repositories: Vec<RepositoryDef>,
    pub entities: Vec<EntityDef>,
    pub stores: Vec<StoreDef>,
    pub settings: Vec<SettingsDef>,
    pub secures: Vec<SecureDef>,
    pub components: Vec<ComponentDef>,
    pub screens: Vec<ScreenDef>,
    /// 本平台用到的全部本地化文案
    pub strings: Vec<LocString>,
}

impl Program {
    pub fn entity(&self, name: &str) -> Option<&EntityDef> {
        self.entities.iter().find(|e| e.name == name)
    }

    pub fn model(&self, name: &str) -> Option<&ModelDef> {
        self.models.iter().find(|m| m.name == name)
    }

    /// 遍历程序中的全部表达式
    pub fn walk_exprs(&self, f: &mut dyn FnMut(&Expr)) {
        for s in &self.screens {
            s.state.iter().for_each(|x| x.default.walk(f));
            s.computed.iter().for_each(|x| x.expr.walk(f));
            for h in &s.lifecycle {
                if let ActionRef::Dispatch { args, .. } = &h.action {
                    args.iter().for_each(|(_, v)| v.walk(f));
                }
            }
            s.view.iter().for_each(|n| n.walk_exprs(f));
        }
        for c in &self.components {
            c.body.iter().for_each(|n| n.walk_exprs(f));
        }
    }

    /// 是否使用了满足条件的修饰符
    pub fn any_modifier(&self, pred: impl Fn(&Modifier) -> bool) -> bool {
        let mut found = false;
        let mut check = |el: &Element| found |= el.modifiers.iter().any(&pred);
        for s in &self.screens {
            s.view.iter().for_each(|n| n.walk_elements(&mut check));
        }
        for c in &self.components {
            c.body.iter().for_each(|n| n.walk_elements(&mut check));
        }
        found
    }

    /// 是否存在满足条件的视图节点
    pub fn any_node(&self, pred: impl Fn(&ViewNode) -> bool) -> bool {
        fn visit(n: &ViewNode, pred: &dyn Fn(&ViewNode) -> bool) -> bool {
            if pred(n) {
                return true;
            }
            match n {
                ViewNode::Element(el) => el.children.iter().any(|c| visit(c, pred)),
                ViewNode::If { then, else_, .. } | ViewNode::Debug { then, else_ } => {
                    then.iter().chain(else_).any(|c| visit(c, pred))
                }
                ViewNode::Native(_) => false,
            }
        }
        self.screens
            .iter()
            .flat_map(|s| &s.view)
            .chain(self.components.iter().flat_map(|c| &c.body))
            .any(|n| visit(n, &pred))
    }

    pub fn any_expr(&self, pred: impl Fn(&Expr) -> bool) -> bool {
        let mut found = false;
        self.walk_exprs(&mut |e| found |= pred(e));
        found
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnumDef {
    pub name: String,
    pub cases: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldDef {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelDef {
    pub name: String,
    pub fields: Vec<FieldDef>,
}

impl ModelDef {
    /// 含`id`字段的模型在Swift中实现`Identifiable`
    pub fn id_field(&self) -> Option<&FieldDef> {
        self.fields.iter().find(|f| f.name == "id")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParamDef {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MethodDef {
    pub name: String,
    pub params: Vec<ParamDef>,
    pub is_async: bool,
    pub throws: bool,
    pub ret: Option<Type>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RepositoryDef {
    pub name: String,
    pub methods: Vec<MethodDef>,
}

/// 被注入到ViewModel的依赖类别
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectKind {
    Repository,
    Store,
    Settings,
    Secure,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InjectDef {
    pub name: String,
    pub ty: String,
    pub kind: InjectKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StateField {
    pub name: String,
    pub ty: Type,
    pub default: Expr,
    pub writable: bool,
}

impl StateField {
    /// 双向绑定对应的契约方法名，例如`setQuery`
    pub fn setter_name(&self) -> Option<String> {
        self.writable.then(|| setter_name(&self.name))
    }
}

pub fn setter_name(field: &str) -> String {
    let mut chars = field.chars();
    match chars.next() {
        Some(c) => format!("set{}{}", c.to_uppercase(), chars.as_str()),
        None => "set".to_string(),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComputedField {
    pub name: String,
    pub ty: Type,
    pub expr: Expr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActionDef {
    pub name: String,
    pub params: Vec<ParamDef>,
    pub is_async: bool,
}

/// ViewModel发出的一次性事件，例如导航
#[derive(Clone, Debug, PartialEq)]
pub struct EffectDef {
    pub name: String,
    pub params: Vec<ParamDef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleEvent {
    /// 页面出现
    Appear,
    /// 页面消失
    Disappear,
}

impl LifecycleEvent {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "onAppear" => Some(Self::Appear),
            "onDisappear" => Some(Self::Disappear),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LifecycleHook {
    pub event: LifecycleEvent,
    pub action: ActionRef,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScreenDef {
    pub name: String,
    pub injects: Vec<InjectDef>,
    pub state: Vec<StateField>,
    pub computed: Vec<ComputedField>,
    pub actions: Vec<ActionDef>,
    pub effects: Vec<EffectDef>,
    pub lifecycle: Vec<LifecycleHook>,
    pub view: Vec<ViewNode>,
}

impl ScreenDef {
    pub fn hook(&self, event: LifecycleEvent) -> Option<&ActionRef> {
        self.lifecycle
            .iter()
            .find(|h| h.event == event)
            .map(|h| &h.action)
    }

    pub fn writable_fields(&self) -> impl Iterator<Item = &StateField> {
        self.state.iter().filter(|f| f.writable)
    }
}

/// 组件参数类型
#[derive(Clone, Debug, PartialEq)]
pub enum ComponentParamType {
    Value(Type),
    /// 无参数的回调
    Action,
    /// 双向绑定
    Binding(Type),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComponentParam {
    pub name: String,
    pub ty: ComponentParamType,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComponentDef {
    pub name: String,
    pub params: Vec<ComponentParam>,
    pub body: Vec<ViewNode>,
}

/// 持久化实体的字段
#[derive(Clone, Debug, PartialEq)]
pub struct EntityField {
    pub name: String,
    pub ty: Type,
    pub primary_key: bool,
    pub indexed: bool,
    pub unique: bool,
}

/// 持久化实体，同名的值类型位于[`Program::models`]
#[derive(Clone, Debug, PartialEq)]
pub struct EntityDef {
    pub name: String,
    /// 数据库表名
    pub table: String,
    pub fields: Vec<EntityField>,
}

impl EntityDef {
    pub fn primary_key(&self) -> &EntityField {
        self.fields
            .iter()
            .find(|f| f.primary_key)
            .unwrap_or(&self.fields[0])
    }
}

/// 查询结果的形态
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryResult {
    /// 持续观察，`Stream<List<T>>`
    Stream,
    /// 读取一次，`List<T>`
    List,
    /// 读取第一条，`T?`
    One,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CompareOp {
    pub fn symbol(self) -> &'static str {
        match self {
            CompareOp::Eq => "==",
            CompareOp::Ne => "!=",
            CompareOp::Lt => "<",
            CompareOp::Le => "<=",
            CompareOp::Gt => ">",
            CompareOp::Ge => ">=",
        }
    }

    /// 交换左右操作数后的运算符
    pub fn flip(self) -> Self {
        match self {
            CompareOp::Lt => CompareOp::Gt,
            CompareOp::Le => CompareOp::Ge,
            CompareOp::Gt => CompareOp::Lt,
            CompareOp::Ge => CompareOp::Le,
            other => other,
        }
    }
}

/// 谓词中的值
#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    /// 查询参数
    Param(String),
    /// 字面量或枚举成员
    Literal(Expr),
}

/// 查询条件，左侧总是实体字段
#[derive(Clone, Debug, PartialEq)]
pub enum Predicate {
    Compare {
        field: String,
        op: CompareOp,
        value: Operand,
    },
    Contains {
        field: String,
        value: Operand,
    },
    IsNil {
        field: String,
        negated: bool,
    },
    /// 布尔字段本身作为条件
    Flag {
        field: String,
        value: bool,
    },
    And(Box<Predicate>, Box<Predicate>),
    Or(Box<Predicate>, Box<Predicate>),
    Not(Box<Predicate>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrderKey {
    pub field: String,
    pub descending: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryDef {
    pub name: String,
    pub params: Vec<ParamDef>,
    pub result: QueryResult,
    pub filter: Option<Predicate>,
    pub order: Vec<OrderKey>,
    pub limit: Option<i64>,
}

/// 实体的数据访问对象，自动提供`all`、`find`、`save`与`delete`
#[derive(Clone, Debug, PartialEq)]
pub struct StoreDef {
    pub name: String,
    pub entity: String,
    pub queries: Vec<QueryDef>,
}

/// store自动生成的方法名
pub const STORE_BUILTINS: [&str; 4] = ["all", "find", "save", "delete"];

/// 简单配置项，带默认值
#[derive(Clone, Debug, PartialEq)]
pub struct SettingField {
    pub name: String,
    pub ty: Type,
    pub default: Expr,
}

/// 简单配置，iOS使用UserDefaults，Android使用DataStore
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsDef {
    pub name: String,
    pub fields: Vec<SettingField>,
}

/// 安全凭证，iOS使用Keychain，Android使用Keystore加密，读取结果总是可选的
#[derive(Clone, Debug, PartialEq)]
pub struct SecureDef {
    pub name: String,
    pub fields: Vec<FieldDef>,
}
