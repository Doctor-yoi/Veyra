// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! SwiftData持久化：实体、store与查询
//!
//! 枚举字段以原始值字符串存储，避免iOS 17的`#Predicate`不支持枚举比较

use veyra_codegen::CodeWriter;
use veyra_ir::{
    EntityDef, EntityField, Expr, ExprKind, Operand, Predicate, Program, QueryDef, QueryResult,
    StoreDef, Type,
};

use crate::swift::{self, ident};
use crate::{dependency_name, header};

/// 实体在SwiftData中的存储类型
fn stored_ty(t: &Type) -> String {
    match t {
        Type::Enum(_) => "String".into(),
        Type::Optional(inner) => format!("{}?", stored_ty(inner)),
        other => swift::ty(other),
    }
}

/// 值类型字段转为存储值
fn to_stored(f: &EntityField, value: &str) -> String {
    match &f.ty {
        Type::Enum(_) => format!("{value}.rawValue"),
        Type::Optional(inner) if matches!(**inner, Type::Enum(_)) => format!("{value}?.rawValue"),
        _ => value.to_string(),
    }
}

/// 存储值转为值类型字段
fn from_stored(f: &EntityField, program: &Program) -> String {
    let name = ident(&f.name);
    let fallback = |e: &str| {
        program
            .enums
            .iter()
            .find(|x| x.name == e)
            .and_then(|x| x.cases.first())
            .map(|c| ident(c))
            .unwrap_or_default()
    };
    match &f.ty {
        Type::Enum(e) => format!("{e}(rawValue: {name}) ?? .{}", fallback(e)),
        Type::Optional(inner) => match &**inner {
            Type::Enum(e) => format!("{name}.flatMap({e}.init(rawValue:))"),
            _ => name,
        },
        _ => name,
    }
}

pub(crate) fn entity(e: &EntityDef, program: &Program) -> String {
    let mut w = CodeWriter::new("    ");
    header(&mut w, &["Foundation", "SwiftData"]);
    w.line(format!(
        "/// Persistent storage of `{}`, use the value type outside the store",
        e.name
    ));
    w.line("@Model");
    w.block(format!("final class {}Entity {{", e.name), "}", |w| {
        for f in &e.fields {
            let attr = if f.primary_key || f.unique {
                "@Attribute(.unique) "
            } else {
                ""
            };
            w.line(format!(
                "{attr}var {}: {}",
                ident(&f.name),
                stored_ty(&f.ty)
            ));
        }
        w.blank();
        let ps: Vec<_> = e
            .fields
            .iter()
            .map(|f| format!("{}: {}", ident(&f.name), stored_ty(&f.ty)))
            .collect();
        w.block(format!("init({}) {{", ps.join(", ")), "}", |w| {
            for f in &e.fields {
                w.line(format!("self.{0} = {0}", ident(&f.name)));
            }
        });
    });
    w.blank();
    w.block(format!("extension {}Entity {{", e.name), "}", |w| {
        let args: Vec<_> = e
            .fields
            .iter()
            .map(|f| {
                format!(
                    "{}: {}",
                    ident(&f.name),
                    to_stored(f, &format!("model.{}", ident(&f.name)))
                )
            })
            .collect();
        w.block(
            format!("convenience init(_ model: {}) {{", e.name),
            "}",
            |w| {
                w.line(format!("self.init({})", args.join(", ")));
            },
        );
        w.blank();
        w.block(format!("var model: {} {{", e.name), "}", |w| {
            let fields: Vec<_> = e
                .fields
                .iter()
                .map(|f| format!("{}: {}", ident(&f.name), from_stored(f, program)))
                .collect();
            w.line(format!("{}({})", e.name, fields.join(", ")));
        });
        w.blank();
        w.block(
            format!("func update(from model: {}) {{", e.name),
            "}",
            |w| {
                for f in e.fields.iter().filter(|f| !f.primary_key) {
                    w.line(format!(
                        "{} = {}",
                        ident(&f.name),
                        to_stored(f, &format!("model.{}", ident(&f.name)))
                    ));
                }
            },
        );
    });
    w.finish()
}

fn query_sig(q: &QueryDef, entity: &str) -> String {
    let ps: Vec<_> = q
        .params
        .iter()
        .map(|p| format!("{}: {}", ident(&p.name), swift::ty(&p.ty)))
        .collect();
    let ret = match q.result {
        QueryResult::Stream => format!(" -> AsyncStream<[{entity}]>"),
        QueryResult::List => format!(" async throws -> [{entity}]"),
        QueryResult::One => format!(" async throws -> {entity}?"),
    };
    format!("func {}({}){ret}", ident(&q.name), ps.join(", "))
}

/// 谓词中的字面量
fn literal(e: &Expr) -> String {
    match &e.kind {
        ExprKind::EnumCase { case, .. } => swift::string_literal(case),
        _ => swift::expr(swift::ExprCx { state: "" }, e),
    }
}

struct PredicateCx<'a> {
    entity: &'a EntityDef,
    q: &'a QueryDef,
}

impl PredicateCx<'_> {
    fn field(&self, name: &str) -> &EntityField {
        self.entity
            .fields
            .iter()
            .find(|f| f.name == name)
            .unwrap_or(&self.entity.fields[0])
    }

    fn operand(&self, v: &Operand) -> String {
        match v {
            Operand::Param(p) => {
                let is_enum = self
                    .q
                    .params
                    .iter()
                    .any(|x| x.name == *p && matches!(x.ty.unwrap_optional(), Type::Enum(_)));
                if is_enum {
                    format!("{}Raw", p)
                } else {
                    ident(p)
                }
            }
            Operand::Literal(e) => literal(e),
        }
    }

    fn emit(&self, p: &Predicate) -> String {
        match p {
            Predicate::Compare { field, op, value } => {
                format!(
                    "entity.{} {} {}",
                    ident(field),
                    op.symbol(),
                    self.operand(value)
                )
            }
            Predicate::Contains { field, value } => {
                let f = if self.field(field).ty.is_optional() {
                    format!("(entity.{} ?? \"\")", ident(field))
                } else {
                    format!("entity.{}", ident(field))
                };
                format!("{f}.localizedStandardContains({})", self.operand(value))
            }
            Predicate::IsNil { field, negated } => {
                format!(
                    "entity.{} {} nil",
                    ident(field),
                    if *negated { "!=" } else { "==" }
                )
            }
            Predicate::Flag { field, value } => {
                if *value {
                    format!("entity.{}", ident(field))
                } else {
                    format!("!entity.{}", ident(field))
                }
            }
            Predicate::And(a, b) => format!("({} && {})", self.emit(a), self.emit(b)),
            Predicate::Or(a, b) => format!("({} || {})", self.emit(a), self.emit(b)),
            Predicate::Not(a) => format!("!({})", self.emit(a)),
        }
    }
}

fn strip_outer(s: String) -> String {
    // 去掉最外层多余的括号
    if s.starts_with('(') && s.ends_with(')') {
        let inner = &s[1..s.len() - 1];
        let mut depth = 0i32;
        let balanced = inner.chars().all(|c| {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            depth >= 0
        });
        if balanced && depth == 0 {
            return inner.to_string();
        }
    }
    s
}

/// 构造FetchDescriptor的语句，返回后`descriptor`变量可用
fn descriptor(w: &mut CodeWriter, e: &EntityDef, q: &QueryDef) {
    for p in &q.params {
        match &p.ty {
            Type::Enum(_) => w.line(format!("let {}Raw = {}.rawValue", p.name, ident(&p.name))),
            Type::Optional(inner) if matches!(**inner, Type::Enum(_)) => {
                w.line(format!("let {}Raw = {}?.rawValue", p.name, ident(&p.name)))
            }
            _ => {}
        }
    }
    let mut args = Vec::new();
    if let Some(filter) = &q.filter {
        let body = strip_outer(PredicateCx { entity: e, q }.emit(filter));
        args.push(format!(
            "predicate: #Predicate<{}Entity> {{ entity in {body} }}",
            e.name
        ));
    }
    if !q.order.is_empty() {
        let sorts: Vec<_> = q
            .order
            .iter()
            .map(|k| {
                let order = if k.descending { ".reverse" } else { ".forward" };
                format!(
                    "SortDescriptor(\\{}Entity.{}, order: {order})",
                    e.name,
                    ident(&k.field)
                )
            })
            .collect();
        args.push(format!("sortBy: [{}]", sorts.join(", ")));
    }
    let keyword = if q.limit.is_some() || q.result == QueryResult::One {
        "var"
    } else {
        "let"
    };
    w.line(format!(
        "{keyword} descriptor = FetchDescriptor<{}Entity>({})",
        e.name,
        args.join(", ")
    ));
    match (q.limit, q.result) {
        (_, QueryResult::One) => w.line("descriptor.fetchLimit = 1"),
        (Some(n), _) => w.line(format!("descriptor.fetchLimit = {n}")),
        _ => {}
    }
}

pub(crate) fn store(s: &StoreDef, program: &Program) -> String {
    let Some(e) = program.entity(&s.entity) else {
        return String::new();
    };
    let pk = e.primary_key();
    let pk_param = format!("{}: {}", ident(&pk.name), swift::ty(&pk.ty));
    let name = &e.name;
    let mut w = CodeWriter::new("    ");
    header(&mut w, &["Foundation", "SwiftData"]);
    w.line(format!("/// Data access for `{name}`"));
    w.line("@MainActor");
    w.block(format!("protocol {}: AnyObject {{", s.name), "}", |w| {
        w.line(format!("func all() -> AsyncStream<[{name}]>"));
        w.line(format!("func find({pk_param}) async throws -> {name}?"));
        w.line(format!("func save(_ item: {name}) async throws"));
        w.line(format!("func delete({pk_param}) async throws"));
        for q in &s.queries {
            w.line(query_sig(q, name));
        }
    });
    w.blank();
    w.line(format!("/// SwiftData implementation of `{}`", s.name));
    w.line("@MainActor");
    w.block(format!("final class SwiftData{0}: {0} {{", s.name), "}", |w| {
        w.line("private let context: ModelContext");
        w.blank();
        w.block("init(context: ModelContext) {", "}", |w| w.line("self.context = context"));
        w.blank();
        w.block(format!("func all() -> AsyncStream<[{name}]> {{"), "}", |w| {
            w.line(format!("observe(FetchDescriptor<{name}Entity>())"));
        });
        w.blank();
        w.block(format!("func find({pk_param}) async throws -> {name}? {{"), "}", |w| {
            w.line(format!("try entity({0}: {0})?.model", ident(&pk.name)));
        });
        w.blank();
        w.block(format!("func save(_ item: {name}) async throws {{"), "}", |w| {
            w.block(format!("if let existing = try entity({0}: item.{0}) {{", ident(&pk.name)), "} else {", |w| {
                w.line("existing.update(from: item)");
            });
            w.indent();
            w.line(format!("context.insert({name}Entity(item))"));
            w.dedent();
            w.line("}");
            w.line("try context.save()");
        });
        w.blank();
        w.block(format!("func delete({pk_param}) async throws {{"), "}", |w| {
            w.block(format!("if let existing = try entity({0}: {0}) {{", ident(&pk.name)), "}", |w| {
                w.line("context.delete(existing)");
                w.line("try context.save()");
            });
        });
        for q in &s.queries {
            w.blank();
            w.block(format!("{} {{", query_sig(q, name)), "}", |w| {
                descriptor(w, e, q);
                match q.result {
                    QueryResult::Stream => w.line("return observe(descriptor)"),
                    QueryResult::List => w.line("return try context.fetch(descriptor).map(\\.model)"),
                    QueryResult::One => w.line("return try context.fetch(descriptor).first?.model"),
                }
            });
        }
        w.blank();
        w.block(format!("private func entity({pk_param}) throws -> {name}Entity? {{"), "}", |w| {
            let pk_name = ident(&pk.name);
            w.line(format!(
                "var descriptor = FetchDescriptor<{name}Entity>(predicate: #Predicate {{ $0.{pk_name} == {pk_name} }})"
            ));
            w.line("descriptor.fetchLimit = 1");
            w.line("return try context.fetch(descriptor).first");
        });
        w.blank();
        w.block(format!("private func observe(_ descriptor: FetchDescriptor<{name}Entity>) -> AsyncStream<[{name}]> {{"), "}", |w| {
            w.block("VeyraObservation.stream(context: context) { context in", "}", |w| {
                w.line("(try? context.fetch(descriptor))?.map(\\.model) ?? []");
            });
        });
    });
    w.finish()
}

/// 容器、观察工具与默认store集合
pub(crate) fn support(program: &Program) -> String {
    let mut w = CodeWriter::new("    ");
    header(&mut w, &["Foundation", "SwiftData"]);
    w.line("/// Model container that contains every Veyra entity");
    w.block("enum VeyraStorage {", "}", |w| {
        let types: Vec<_> = program.entities.iter().map(|e| format!("{}Entity.self", e.name)).collect();
        w.line(format!("static let schema = Schema([{}])", types.join(", ")));
        w.blank();
        w.block("static func makeContainer(inMemory: Bool = false) throws -> ModelContainer {", "}", |w| {
            w.line("let configuration = ModelConfiguration(schema: schema, isStoredInMemoryOnly: inMemory)");
            w.line("return try ModelContainer(for: schema, configurations: configuration)");
        });
    });
    w.blank();
    w.line("/// Default SwiftData stores, useful when implementing `VeyraDependencies`");
    w.line("@MainActor");
    w.block("final class VeyraStores {", "}", |w| {
        for s in &program.stores {
            w.line(format!("let {}: any {}", dependency_name(&s.name), s.name));
        }
        w.blank();
        w.block("init(context: ModelContext) {", "}", |w| {
            for s in &program.stores {
                w.line(format!(
                    "{} = SwiftData{}(context: context)",
                    dependency_name(&s.name),
                    s.name
                ));
            }
        });
    });
    w.blank();
    w.line("/// Holds main actor values inside sendable closures");
    w.block(
        "private final class VeyraMainActorBox<Value>: @unchecked Sendable {",
        "}",
        |w| {
            w.line("let value: Value");
            w.blank();
            w.block("init(_ value: Value) {", "}", |w| {
                w.line("self.value = value")
            });
        },
    );
    w.blank();
    w.line("/// Re-runs a fetch whenever the model context saves");
    w.line("@MainActor");
    w.block("enum VeyraObservation {", "}", |w| {
        w.block(
            "static func stream<Value: Sendable>(context: ModelContext, fetch: @escaping (ModelContext) -> Value) -> AsyncStream<Value> {",
            "}",
            |w| {
                w.block("AsyncStream { continuation in", "}", |w| {
                    w.line("continuation.yield(fetch(context))");
                    w.line("let source = VeyraMainActorBox((context, fetch))");
                    w.line("let token = NotificationCenter.default.addObserver(forName: ModelContext.didSave, object: nil, queue: .main) { _ in");
                    w.indent();
                    w.block("MainActor.assumeIsolated {", "}", |w| {
                        w.line("_ = continuation.yield(source.value.1(source.value.0))");
                    });
                    w.dedent();
                    w.line("}");
                    w.line("let observer = VeyraMainActorBox(token)");
                    w.block("continuation.onTermination = { _ in", "}", |w| {
                        w.line("NotificationCenter.default.removeObserver(observer.value)");
                    });
                });
            },
        );
    });
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use veyra_ir::CompareOp;

    #[test]
    fn strips_outer_parens() {
        assert_eq!(strip_outer("(a && b)".into()), "a && b");
        assert_eq!(strip_outer("(a) || (b)".into()), "(a) || (b)");
    }

    #[test]
    fn flips_compare() {
        assert_eq!(CompareOp::Lt.flip(), CompareOp::Gt);
    }
}
