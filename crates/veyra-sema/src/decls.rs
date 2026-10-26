// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 枚举、模型与repository声明的检查，以及类型解析。

use indexmap::{IndexMap, IndexSet};
use veyra_diagnostics::Diagnostic;
use veyra_ir::{EnumDef, FieldDef, MethodDef, ModelDef, ParamDef, RepositoryDef, Type};
use veyra_syntax::ast::{self, TypeExpr, TypeExprKind};

use crate::{Checker, Global};

/// 类型出现的位置，决定是否允许Stream。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypePos {
    Value,
    Return,
}

impl<'a> Checker<'a> {
    pub(crate) fn check_decls(&mut self) {
        let globals: Vec<_> = self.globals.values().map(|(g, _)| *g).collect();
        for g in &globals {
            if let Global::Enum(d) = g {
                self.check_enum(d);
            }
        }
        // 先登记模型名称，字段类型可以互相引用
        for g in &globals {
            if let Global::Model(d) = g {
                self.models.insert(
                    d.name.name.clone(),
                    ModelDef {
                        name: d.name.name.clone(),
                        fields: Vec::new(),
                    },
                );
            }
        }
        for g in &globals {
            if let Global::Model(d) = g {
                let fields = self.check_model_fields(d);
                if let Some(m) = self.models.get_mut(&d.name.name) {
                    m.fields = fields;
                }
            }
        }
        self.check_model_cycles(&globals);
        for g in &globals {
            if let Global::Repository(d) = g {
                let def = self.check_repository(d);
                self.repositories.insert(def.name.clone(), def);
            }
        }
    }

    fn check_enum(&mut self, d: &'a ast::EnumDecl) {
        let mut cases = IndexSet::new();
        for case in self.prune_decls(&d.cases) {
            self.check_member_name(case);
            if !cases.insert(case.name.clone()) {
                self.error(
                    "V1003",
                    format!("枚举成员`{}`重复", case.name),
                    case.span,
                    "",
                );
            }
        }
        if cases.is_empty() {
            self.error(
                "V2060",
                format!("枚举`{}`至少需要一个成员", d.name.name),
                d.name.span,
                "",
            );
        }
        self.enums.insert(
            d.name.name.clone(),
            EnumDef {
                name: d.name.name.clone(),
                cases: cases.into_iter().collect(),
            },
        );
    }

    fn check_model_fields(&mut self, d: &'a ast::ModelDecl) -> Vec<FieldDef> {
        let mut out: Vec<FieldDef> = Vec::new();
        for field in self.prune_decls(&d.fields) {
            self.check_member_name(&field.name);
            if out.iter().any(|f| f.name == field.name.name) {
                self.error(
                    "V1003",
                    format!("字段`{}`重复", field.name.name),
                    field.name.span,
                    "",
                );
                continue;
            }
            if let Some(default) = &field.default {
                self.error("V2041", "model字段不能有默认值", default.span, "");
            }
            for attr in &field.attrs {
                self.error(
                    "V2040",
                    format!("model字段不支持`@{}`", attr.name.name),
                    attr.span,
                    "",
                );
            }
            let ty = self.resolve_type(&field.ty, TypePos::Value);
            out.push(FieldDef {
                name: field.name.name.clone(),
                ty,
            });
        }
        out
    }

    /// 结构体不能直接或经由可选字段包含自身，列表字段不受限制。
    fn check_model_cycles(&mut self, globals: &[Global<'a>]) {
        fn direct_deps(ty: &Type, out: &mut Vec<String>) {
            match ty {
                Type::Model(n) => out.push(n.clone()),
                Type::Optional(inner) => direct_deps(inner, out),
                _ => {}
            }
        }
        let graph: IndexMap<String, Vec<String>> = self
            .models
            .values()
            .map(|m| {
                let mut deps = Vec::new();
                for f in &m.fields {
                    direct_deps(&f.ty, &mut deps);
                }
                (m.name.clone(), deps)
            })
            .collect();
        for g in globals {
            let Global::Model(d) = g else { continue };
            let mut stack = vec![d.name.name.clone()];
            let mut seen = IndexSet::new();
            let mut cyclic = false;
            while let Some(n) = stack.pop() {
                for dep in graph.get(&n).into_iter().flatten() {
                    if *dep == d.name.name {
                        cyclic = true;
                    } else if seen.insert(dep.clone()) {
                        stack.push(dep.clone());
                    }
                }
            }
            if cyclic {
                self.diags.push(
                    Diagnostic::error("V2012", format!("模型`{}`直接包含了自身", d.name.name))
                        .with_label(d.name.span, "")
                        .with_note("值类型不能递归包含自身，可以改用List字段"),
                );
            }
        }
    }

    fn check_repository(&mut self, d: &'a ast::RepositoryDecl) -> RepositoryDef {
        let mut methods: Vec<MethodDef> = Vec::new();
        for m in self.prune_decls(&d.methods) {
            self.check_member_name(&m.name);
            if methods.iter().any(|x| x.name == m.name.name) {
                self.error(
                    "V1003",
                    format!("方法`{}`重复", m.name.name),
                    m.name.span,
                    "",
                );
                continue;
            }
            let params = self.params(&m.params);
            let ret = m
                .ret
                .as_ref()
                .map(|t| self.resolve_type(t, TypePos::Return));
            if let (Some(Type::Stream(_)), Some(t)) = (&ret, &m.ret)
                && m.is_async
            {
                self.error(
                    "V2050",
                    "返回Stream的方法不能是async",
                    t.span,
                    "Stream本身就是异步的",
                );
            }
            methods.push(MethodDef {
                name: m.name.name.clone(),
                params,
                is_async: m.is_async,
                throws: m.throws,
                ret,
            });
        }
        RepositoryDef {
            name: d.name.name.clone(),
            methods,
        }
    }

    pub(crate) fn params(&mut self, params: &[ast::Param]) -> Vec<ParamDef> {
        let mut out: Vec<ParamDef> = Vec::new();
        for p in params {
            self.check_member_name(&p.name);
            if out.iter().any(|x| x.name == p.name.name) {
                self.error(
                    "V1003",
                    format!("参数`{}`重复", p.name.name),
                    p.name.span,
                    "",
                );
            }
            let ty = self.resolve_type(&p.ty, TypePos::Value);
            out.push(ParamDef {
                name: p.name.name.clone(),
                ty,
            });
        }
        out
    }

    pub(crate) fn resolve_type(&mut self, t: &TypeExpr, pos: TypePos) -> Type {
        match &t.kind {
            TypeExprKind::Optional(inner) => {
                let inner = self.resolve_type(inner, TypePos::Value);
                if inner.is_optional() {
                    self.error("V2003", "不能嵌套可选类型", t.span, "");
                }
                Type::optional(inner)
            }
            TypeExprKind::Named { name, args } => {
                let arity = |c: &mut Self, n: usize| {
                    if args.len() != n {
                        c.error(
                            "V2004",
                            format!("`{}`需要{n}个类型参数", name.name),
                            t.span,
                            "",
                        );
                        false
                    } else {
                        true
                    }
                };
                let ty = match name.name.as_str() {
                    "Bool" => Type::Bool,
                    "Int" => Type::Int,
                    "Double" => Type::Double,
                    "String" => Type::String,
                    "Date" => Type::Date,
                    "List" => {
                        if !arity(self, 1) {
                            return Type::Unknown;
                        }
                        let inner = self.resolve_type(&args[0], TypePos::Value);
                        return Type::list(inner);
                    }
                    "Stream" => {
                        if !arity(self, 1) {
                            return Type::Unknown;
                        }
                        let inner = self.resolve_type(&args[0], TypePos::Value);
                        if pos != TypePos::Return {
                            self.diags.push(
                                Diagnostic::error(
                                    "V2005",
                                    "Stream只能作为repository方法的返回类型",
                                )
                                .with_label(t.span, "")
                                .with_note("ViewModel负责订阅Stream并写入state，View只读取state"),
                            );
                        }
                        return Type::Stream(Box::new(inner));
                    }
                    other => match self.globals.get(other).map(|(g, _)| *g) {
                        Some(Global::Enum(_)) => Type::Enum(other.to_string()),
                        Some(Global::Model(_)) => Type::Model(other.to_string()),
                        Some(_) => {
                            self.error("V2006", format!("`{other}`不是值类型"), name.span, "");
                            return Type::Unknown;
                        }
                        None => {
                            self.error("V1010", format!("未定义的类型`{other}`"), name.span, "");
                            return Type::Unknown;
                        }
                    },
                };
                if !args.is_empty() {
                    self.error(
                        "V2004",
                        format!("`{}`不接受类型参数", name.name),
                        t.span,
                        "",
                    );
                }
                ty
            }
        }
    }
}
