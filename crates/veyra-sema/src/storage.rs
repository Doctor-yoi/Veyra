// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 持久化实体与store的检查，查询条件降级为与平台无关的谓词

use veyra_diagnostics::Diagnostic;
use veyra_ir::{
    self as ir, CompareOp, EntityDef, EntityField, FieldDef, Operand, OrderKey, Predicate,
    QueryDef, QueryResult, STORE_BUILTINS, StoreDef, Type,
};
use veyra_syntax::ast::{self, BinaryOp, ExprKind as A};

use crate::decls::TypePos;
use crate::{Checker, Global};

/// `CourseItem`转为表名`course_item`
fn snake_case(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// 可以持久化的字段类型
fn storable(t: &Type) -> bool {
    matches!(
        t.unwrap_optional(),
        Type::Bool | Type::Int | Type::Double | Type::String | Type::Date | Type::Enum(_)
    )
}

impl<'a> Checker<'a> {
    pub(crate) fn check_entity(&mut self, d: &'a ast::EntityDecl) {
        for attr in &d.attrs {
            if attr.name.name != "persistent" {
                self.error(
                    "V2040",
                    format!("entity不支持`@{}`", attr.name.name),
                    attr.span,
                    "",
                );
            }
        }
        let mut fields: Vec<EntityField> = Vec::new();
        for f in self.prune_decls(&d.fields) {
            self.check_member_name(&f.name);
            if fields.iter().any(|x| x.name == f.name.name) {
                self.error(
                    "V1003",
                    format!("字段`{}`重复", f.name.name),
                    f.name.span,
                    "",
                );
                continue;
            }
            if let Some(default) = &f.default {
                self.error("V2041", "entity字段不能有默认值", default.span, "");
            }
            let ty = self.resolve_type(&f.ty, TypePos::Value);
            if !ty.is_unknown() && !storable(&ty) {
                self.diags.push(
                    Diagnostic::error("V5001", format!("entity字段不能是`{ty}`"))
                        .with_label(f.ty.span, "")
                        .with_note(
                            "可持久化的类型：Bool、Int、Double、String、Date、枚举及其可选类型",
                        ),
                );
            }
            let mut field = EntityField {
                name: f.name.name.clone(),
                ty,
                primary_key: false,
                indexed: false,
                unique: false,
            };
            for attr in &f.attrs {
                match attr.name.name.as_str() {
                    "primaryKey" => field.primary_key = true,
                    "indexed" => field.indexed = true,
                    "unique" => field.unique = true,
                    other => self.error(
                        "V2040",
                        format!("entity字段不支持`@{other}`"),
                        attr.span,
                        "",
                    ),
                }
            }
            if field.primary_key && !matches!(field.ty, Type::String | Type::Int | Type::Unknown) {
                self.error("V5002", "主键必须是非可选的String或Int", f.ty.span, "");
            }
            fields.push(field);
        }
        match fields.iter().filter(|f| f.primary_key).count() {
            1 => {}
            0 => self.diags.push(
                Diagnostic::error("V5003", format!("entity`{}`缺少主键", d.name.name))
                    .with_label(d.name.span, "")
                    .with_note("为一个String或Int字段添加@primaryKey"),
            ),
            _ => self.error(
                "V5003",
                format!("entity`{}`只能有一个主键", d.name.name),
                d.name.span,
                "",
            ),
        }
        if let Some(m) = self.models.get_mut(&d.name.name) {
            m.fields = fields
                .iter()
                .map(|f| FieldDef {
                    name: f.name.clone(),
                    ty: f.ty.clone(),
                })
                .collect();
        }
        let table = snake_case(&d.name.name);
        self.entities.insert(
            d.name.name.clone(),
            EntityDef {
                name: d.name.name.clone(),
                table,
                fields,
            },
        );
    }

    pub(crate) fn check_store(&mut self, d: &'a ast::StoreDecl) {
        let entity = match self.globals.get(&d.entity.name).map(|(g, _)| *g) {
            Some(Global::Entity(_)) => d.entity.name.clone(),
            Some(_) => {
                self.error(
                    "V5004",
                    format!("`{}`不是entity", d.entity.name),
                    d.entity.span,
                    "store只能用于entity",
                );
                return;
            }
            None => {
                self.error(
                    "V1010",
                    format!("未定义的entity`{}`", d.entity.name),
                    d.entity.span,
                    "",
                );
                return;
            }
        };
        let Some(def) = self.entities.get(&entity).cloned() else {
            return;
        };
        let mut queries: Vec<QueryDef> = Vec::new();
        for q in self.prune_decls(&d.queries) {
            self.check_member_name(&q.name);
            if STORE_BUILTINS.contains(&q.name.name.as_str())
                || queries.iter().any(|x| x.name == q.name.name)
            {
                self.diags.push(
                    Diagnostic::error("V5005", format!("查询名称`{}`已被使用", q.name.name))
                        .with_label(q.name.span, "")
                        .with_note("store自动提供all、find、save与delete"),
                );
                continue;
            }
            if let Some(q) = self.check_query(&def, q) {
                queries.push(q);
            }
        }
        self.stores.insert(
            d.name.name.clone(),
            StoreDef {
                name: d.name.name.clone(),
                entity,
                queries,
            },
        );
    }

    fn check_query(&mut self, entity: &EntityDef, q: &'a ast::QueryDecl) -> Option<QueryDef> {
        let params = self.params(&q.params);
        for p in &params {
            if !storable(&p.ty) && !p.ty.is_unknown() {
                self.error("V5006", format!("查询参数不能是`{}`", p.ty), q.span, "");
            }
            if entity.fields.iter().any(|f| f.name == p.name) {
                self.diags.push(
                    Diagnostic::error("V5007", format!("查询参数`{}`与字段同名", p.name))
                        .with_label(q.name.span, "")
                        .with_note("请给参数换一个名称，避免条件中的名称产生歧义"),
                );
                return None;
            }
        }
        let model = Type::Model(entity.name.clone());
        let ret = self.resolve_type(&q.ret, TypePos::Return);
        let result = match &ret {
            Type::Stream(inner) if **inner == Type::list(model.clone()) => QueryResult::Stream,
            Type::List(inner) if **inner == model => QueryResult::List,
            Type::Optional(inner) if **inner == model => QueryResult::One,
            Type::Unknown => QueryResult::List,
            _ => {
                let n = &entity.name;
                self.diags.push(
                    Diagnostic::error(
                        "V5008",
                        format!("查询的返回类型必须是`Stream<List<{n}>>`、`List<{n}>`或`{n}?`"),
                    )
                    .with_label(q.ret.span, ""),
                );
                return None;
            }
        };
        let filter = q
            .filter
            .as_ref()
            .and_then(|e| self.predicate(entity, &params, e));
        let mut order = Vec::new();
        for k in &q.order {
            let Some(field) = entity.fields.iter().find(|f| f.name == k.field.name) else {
                self.error(
                    "V2025",
                    format!("`{}`没有字段`{}`", entity.name, k.field.name),
                    k.field.span,
                    "",
                );
                continue;
            };
            if !matches!(
                field.ty.unwrap_optional(),
                Type::Int | Type::Double | Type::String | Type::Date
            ) {
                self.diags.push(
                    Diagnostic::error("V5016", format!("不能按`{}`排序", field.ty))
                        .with_label(k.field.span, "")
                        .with_note("排序字段必须是Int、Double、String或Date"),
                );
                continue;
            }
            order.push(OrderKey {
                field: k.field.name.clone(),
                descending: k.descending,
            });
        }
        let limit = match &q.limit {
            None => None,
            Some(ast::Expr {
                kind: A::Int(n), ..
            }) if *n > 0 => Some(*n),
            Some(e) => {
                self.error("V5009", "limit必须是正整数字面量", e.span, "");
                None
            }
        };
        if result == QueryResult::One && limit.is_some() {
            self.error("V5009", "返回单个结果的查询不需要limit", q.span, "");
        }
        Some(QueryDef {
            name: q.name.name.clone(),
            params,
            result,
            filter,
            order,
            limit,
        })
    }

    fn field<'e>(&mut self, entity: &'e EntityDef, e: &ast::Expr) -> Option<&'e EntityField> {
        match &e.kind {
            A::Ident(n) => entity.fields.iter().find(|f| f.name == *n),
            _ => None,
        }
    }

    /// 比较运算右侧的值：查询参数或字面量
    fn operand(
        &mut self,
        params: &[ir::ParamDef],
        field: &EntityField,
        e: &ast::Expr,
    ) -> Option<Operand> {
        if let A::Ident(n) = &e.kind
            && let Some(p) = params.iter().find(|p| p.name == *n)
        {
            if !field.ty.unwrap_optional().accepts(p.ty.unwrap_optional()) {
                self.mismatch(e.span, &field.ty, &p.ty);
            }
            return Some(Operand::Param(n.clone()));
        }
        let ok = matches!(
            e.kind,
            A::Int(_)
                | A::Float(..)
                | A::Str(..)
                | A::Bool(_)
                | A::ImplicitMember(_)
                | A::Member(..)
        );
        if !ok {
            self.error("V5010", "条件右侧必须是查询参数或字面量", e.span, "");
            return None;
        }
        let sc = crate::expr::Scope::new(crate::expr::Owner::Constant, false);
        let v = self.expr(&sc, e, Some(field.ty.unwrap_optional()));
        Some(Operand::Literal(v))
    }

    fn predicate(
        &mut self,
        entity: &EntityDef,
        params: &[ir::ParamDef],
        e: &ast::Expr,
    ) -> Option<Predicate> {
        match &e.kind {
            A::Paren(inner) => self.predicate(entity, params, inner),
            A::Unary(ast::UnaryOp::Not, inner) => Some(Predicate::Not(Box::new(
                self.predicate(entity, params, inner)?,
            ))),
            A::Binary(BinaryOp::And, l, r) => {
                let (l, r) = (
                    self.predicate(entity, params, l),
                    self.predicate(entity, params, r),
                );
                Some(Predicate::And(Box::new(l?), Box::new(r?)))
            }
            A::Binary(BinaryOp::Or, l, r) => {
                let (l, r) = (
                    self.predicate(entity, params, l),
                    self.predicate(entity, params, r),
                );
                Some(Predicate::Or(Box::new(l?), Box::new(r?)))
            }
            A::Binary(
                op @ (BinaryOp::Eq
                | BinaryOp::Ne
                | BinaryOp::Lt
                | BinaryOp::Le
                | BinaryOp::Gt
                | BinaryOp::Ge),
                l,
                r,
            ) => {
                let op = match op {
                    BinaryOp::Eq => CompareOp::Eq,
                    BinaryOp::Ne => CompareOp::Ne,
                    BinaryOp::Lt => CompareOp::Lt,
                    BinaryOp::Le => CompareOp::Le,
                    BinaryOp::Gt => CompareOp::Gt,
                    _ => CompareOp::Ge,
                };
                // 字段统一放在左侧
                let (field, value, op) = match (self.field(entity, l), self.field(entity, r)) {
                    (Some(f), _) => (f.clone(), r, op),
                    (None, Some(f)) => (f.clone(), l, op.flip()),
                    (None, None) => {
                        self.error("V5011", "比较的一侧必须是实体字段", e.span, "");
                        return None;
                    }
                };
                if matches!(value.kind, A::Nil) {
                    if !field.ty.is_optional() {
                        self.error(
                            "V5012",
                            format!("字段`{}`不是可选类型，不能与nil比较", field.name),
                            e.span,
                            "",
                        );
                    }
                    return match op {
                        CompareOp::Eq => Some(Predicate::IsNil {
                            field: field.name,
                            negated: false,
                        }),
                        CompareOp::Ne => Some(Predicate::IsNil {
                            field: field.name,
                            negated: true,
                        }),
                        _ => {
                            self.error("V5012", "nil只能用于==与!=", e.span, "");
                            None
                        }
                    };
                }
                let ordered = matches!(
                    field.ty,
                    Type::Int | Type::Double | Type::String | Type::Date
                );
                if !matches!(op, CompareOp::Eq | CompareOp::Ne) && !ordered {
                    let mut d =
                        Diagnostic::error("V5013", format!("字段`{}`不支持大小比较", field.name))
                            .with_label(e.span, "");
                    if field.ty.is_optional() {
                        d = d.with_note("可选字段只能与值或nil判断是否相等");
                    }
                    self.diags.push(d);
                }
                let value = self.operand(params, &field, value)?;
                Some(Predicate::Compare {
                    field: field.name,
                    op,
                    value,
                })
            }
            A::Call(callee, args) => {
                let A::Member(base, method) = &callee.kind else {
                    self.error("V5014", "不支持的查询条件", e.span, "");
                    return None;
                };
                let Some(field) = self.field(entity, base).cloned() else {
                    self.error("V5011", "contains必须用于实体字段", base.span, "");
                    return None;
                };
                if method.name != "contains" || args.len() != 1 || args[0].label.is_some() {
                    self.error("V5014", "字符串字段只支持contains(x)", e.span, "");
                    return None;
                }
                if *field.ty.unwrap_optional() != Type::String {
                    self.error(
                        "V5014",
                        format!("字段`{}`不是String，不能使用contains", field.name),
                        e.span,
                        "",
                    );
                    return None;
                }
                let value = self.operand(params, &field, &args[0].value)?;
                Some(Predicate::Contains {
                    field: field.name,
                    value,
                })
            }
            A::Ident(_) => match self.field(entity, e).cloned() {
                Some(f) if f.ty == Type::Bool => Some(Predicate::Flag {
                    field: f.name,
                    value: true,
                }),
                Some(f) => {
                    self.error(
                        "V5015",
                        format!("字段`{}`不是Bool，不能直接作为条件", f.name),
                        e.span,
                        "",
                    );
                    None
                }
                None => {
                    self.error(
                        "V1010",
                        format!("`{}`没有字段`{}`", entity.name, ast_ident(e)),
                        e.span,
                        "",
                    );
                    None
                }
            },
            _ => {
                self.diags.push(
                    Diagnostic::error("V5014", "不支持的查询条件")
                        .with_label(e.span, "")
                        .with_note("可用：比较运算、contains、与nil比较、Bool字段、&&、||、!"),
                );
                None
            }
        }
    }
}

impl<'a> Checker<'a> {
    pub(crate) fn check_settings(&mut self, d: &'a ast::FieldsDecl) {
        let mut fields: Vec<ir::SettingField> = Vec::new();
        for f in self.prune_decls(&d.fields) {
            self.check_member_name(&f.name);
            if fields.iter().any(|x| x.name == f.name.name) {
                self.error(
                    "V1003",
                    format!("字段`{}`重复", f.name.name),
                    f.name.span,
                    "",
                );
                continue;
            }
            for attr in &f.attrs {
                self.error(
                    "V2040",
                    format!("settings字段不支持`@{}`", attr.name.name),
                    attr.span,
                    "",
                );
            }
            let ty = self.resolve_type(&f.ty, TypePos::Value);
            if !ty.is_unknown() && !storable(&ty) {
                self.diags.push(
                    Diagnostic::error("V5020", format!("settings字段不能是`{ty}`"))
                        .with_label(f.ty.span, "")
                        .with_note("可用的类型：Bool、Int、Double、String、Date、枚举及其可选类型"),
                );
            }
            let sc = crate::expr::Scope::new(crate::expr::Owner::Constant, false);
            let default = match &f.default {
                Some(e) => self.expr(&sc, e, Some(&ty)),
                None if ty.is_optional() => ir::Expr::new(ir::ExprKind::Nil, ty.clone()),
                None => {
                    if *ty.unwrap_optional() == Type::Date {
                        self.error(
                            "V5021",
                            "Date类型的settings字段必须是可选类型",
                            f.ty.span,
                            "",
                        );
                    } else if !ty.is_unknown() {
                        self.error(
                            "V2010",
                            format!("settings字段`{}`需要默认值", f.name.name),
                            f.name.span,
                            "",
                        );
                    }
                    ir::Expr::unknown()
                }
            };
            fields.push(ir::SettingField {
                name: f.name.name.clone(),
                ty,
                default,
            });
        }
        self.settings.insert(
            d.name.name.clone(),
            ir::SettingsDef {
                name: d.name.name.clone(),
                fields,
            },
        );
    }

    pub(crate) fn check_secure(&mut self, d: &'a ast::FieldsDecl) {
        let mut fields: Vec<FieldDef> = Vec::new();
        for f in self.prune_decls(&d.fields) {
            self.check_member_name(&f.name);
            if fields.iter().any(|x| x.name == f.name.name) {
                self.error(
                    "V1003",
                    format!("字段`{}`重复", f.name.name),
                    f.name.span,
                    "",
                );
                continue;
            }
            if let Some(e) = &f.default {
                self.error(
                    "V2041",
                    "secure字段不能有默认值",
                    e.span,
                    "凭证在首次写入前为空",
                );
            }
            for attr in &f.attrs {
                self.error(
                    "V2040",
                    format!("secure字段不支持`@{}`", attr.name.name),
                    attr.span,
                    "",
                );
            }
            let ty = self.resolve_type(&f.ty, TypePos::Value);
            let ty = ty.unwrap_optional().clone();
            if !matches!(
                ty,
                Type::String | Type::Int | Type::Double | Type::Bool | Type::Unknown
            ) {
                self.error(
                    "V5022",
                    format!("secure字段不能是`{ty}`"),
                    f.ty.span,
                    "可用的类型：String、Int、Double、Bool",
                );
            }
            fields.push(FieldDef {
                name: f.name.name.clone(),
                ty,
            });
        }
        self.secures.insert(
            d.name.name.clone(),
            ir::SecureDef {
                name: d.name.name.clone(),
                fields,
            },
        );
    }
}

fn ast_ident(e: &ast::Expr) -> &str {
    match &e.kind {
        A::Ident(n) => n,
        _ => "?",
    }
}
