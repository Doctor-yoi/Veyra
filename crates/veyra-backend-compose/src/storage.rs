// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! Room持久化：实体、DAO、store与数据库

use veyra_codegen::{CodeWriter, USER_OWNED_NOTICE, camel_case};
use veyra_ir::{
    CompareOp, EntityDef, Expr, ExprKind, Operand, Predicate, Program, QueryDef, QueryResult,
    StoreDef,
};

use crate::kotlin::{self, Imports, ident};
use crate::{Options, assemble, dependency_name, generated};

pub(crate) fn entity(e: &EntityDef, o: &Options) -> String {
    let mut im = Imports::new(o.r_package());
    im.add("androidx.room.Entity");
    im.add("androidx.room.PrimaryKey");
    let mut w = CodeWriter::new("    ");
    let indices: Vec<_> = e
        .fields
        .iter()
        .filter(|f| (f.indexed || f.unique) && !f.primary_key)
        .map(|f| {
            if f.unique {
                format!("Index(value = [\"{}\"], unique = true)", f.name)
            } else {
                format!("Index(value = [\"{}\"])", f.name)
            }
        })
        .collect();
    if indices.is_empty() {
        w.line(format!("@Entity(tableName = \"{}\")", e.table));
    } else {
        im.add("androidx.room.Index");
        w.line(format!(
            "@Entity(tableName = \"{}\", indices = [{}])",
            e.table,
            indices.join(", ")
        ));
    }
    w.line(format!("data class {}Entity(", e.name));
    w.indent();
    for f in &e.fields {
        let pk = if f.primary_key { "@PrimaryKey " } else { "" };
        w.line(format!(
            "{pk}val {}: {},",
            ident(&f.name),
            kotlin::ty(&f.ty, &mut im)
        ));
    }
    w.dedent();
    w.line(")");
    w.blank();
    let fields: Vec<_> = e
        .fields
        .iter()
        .map(|f| format!("{0} = {0}", ident(&f.name)))
        .collect();
    w.line(format!(
        "fun {0}Entity.toModel(): {0} = {0}({1})",
        e.name,
        fields.join(", ")
    ));
    w.blank();
    w.line(format!(
        "fun {0}.toEntity(): {0}Entity = {0}Entity({1})",
        e.name,
        fields.join(", ")
    ));
    generated(&o.package, &im, &w.finish())
}

fn sql_literal(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Bool(b) => if *b { "1" } else { "0" }.into(),
        ExprKind::Int(v) => v.to_string(),
        ExprKind::Double(v) => kotlin::double_literal(*v),
        ExprKind::String(s) => format!("'{}'", s.replace('\'', "''")),
        ExprKind::EnumCase { case, .. } => format!("'{case}'"),
        ExprKind::Unary(_, inner) => format!("-{}", sql_literal(inner)),
        _ => "NULL".into(),
    }
}

fn operand(v: &Operand) -> String {
    match v {
        Operand::Param(p) => format!(":{p}"),
        Operand::Literal(e) => sql_literal(e),
    }
}

fn sql(p: &Predicate) -> String {
    match p {
        Predicate::Compare { field, op, value } => {
            let sym = match op {
                CompareOp::Eq => "=",
                other => other.symbol(),
            };
            format!("{field} {sym} {}", operand(value))
        }
        Predicate::Contains { field, value } => match value {
            Operand::Literal(Expr {
                kind: ExprKind::String(s),
                ..
            }) => {
                format!("{field} LIKE '%{}%'", s.replace('\'', "''"))
            }
            other => format!("{field} LIKE '%' || {} || '%'", operand(other)),
        },
        Predicate::IsNil { field, negated } => {
            format!("{field} IS {}NULL", if *negated { "NOT " } else { "" })
        }
        Predicate::Flag { field, value } => format!("{field} = {}", if *value { 1 } else { 0 }),
        Predicate::And(a, b) => format!("({} AND {})", sql(a), sql(b)),
        Predicate::Or(a, b) => format!("({} OR {})", sql(a), sql(b)),
        Predicate::Not(a) => format!("NOT ({})", sql(a)),
    }
}

fn query_sql(e: &EntityDef, q: &QueryDef) -> String {
    let mut out = format!("SELECT * FROM {}", e.table);
    if let Some(f) = &q.filter {
        out.push_str(&format!(" WHERE {}", sql(f)));
    }
    if !q.order.is_empty() {
        let keys: Vec<_> = q
            .order
            .iter()
            .map(|k| format!("{} {}", k.field, if k.descending { "DESC" } else { "ASC" }))
            .collect();
        out.push_str(&format!(" ORDER BY {}", keys.join(", ")));
    }
    match (q.result, q.limit) {
        (QueryResult::One, _) => out.push_str(" LIMIT 1"),
        (_, Some(n)) => out.push_str(&format!(" LIMIT {n}")),
        _ => {}
    }
    out
}

fn params(q: &QueryDef, im: &mut Imports) -> String {
    q.params
        .iter()
        .map(|p| format!("{}: {}", ident(&p.name), kotlin::ty(&p.ty, im)))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn store(s: &StoreDef, program: &Program, o: &Options) -> String {
    let Some(e) = program.entity(&s.entity) else {
        return String::new();
    };
    let mut im = Imports::new(o.r_package());
    for i in [
        "androidx.room.Dao",
        "androidx.room.Query",
        "androidx.room.Upsert",
        "kotlinx.coroutines.flow.Flow",
        "kotlinx.coroutines.flow.map",
    ] {
        im.add(i);
    }
    let pk = e.primary_key();
    let pk_param = format!("{}: {}", ident(&pk.name), kotlin::ty(&pk.ty, &mut im));
    let name = &e.name;
    let dao = format!("{}Dao", s.name);
    let mut w = CodeWriter::new("    ");

    w.line("@Dao");
    w.block(format!("interface {dao} {{"), "}", |w| {
        w.line(format!("@Query(\"SELECT * FROM {}\")", e.table));
        w.line(format!("fun all(): Flow<List<{name}Entity>>"));
        w.blank();
        w.line(format!(
            "@Query(\"SELECT * FROM {} WHERE {} = :{} LIMIT 1\")",
            e.table, pk.name, pk.name
        ));
        w.line(format!("suspend fun find({pk_param}): {name}Entity?"));
        w.blank();
        w.line("@Upsert");
        w.line(format!("suspend fun save(entity: {name}Entity)"));
        w.blank();
        w.line(format!(
            "@Query(\"DELETE FROM {} WHERE {} = :{}\")",
            e.table, pk.name, pk.name
        ));
        w.line(format!("suspend fun delete({pk_param})"));
        for q in &s.queries {
            w.blank();
            w.line(format!(
                "@Query({})",
                kotlin::string_literal(&query_sql(e, q))
            ));
            let ps = params(q, &mut im);
            match q.result {
                QueryResult::Stream => w.line(format!(
                    "fun {}({ps}): Flow<List<{name}Entity>>",
                    ident(&q.name)
                )),
                QueryResult::List => w.line(format!(
                    "suspend fun {}({ps}): List<{name}Entity>",
                    ident(&q.name)
                )),
                QueryResult::One => w.line(format!(
                    "suspend fun {}({ps}): {name}Entity?",
                    ident(&q.name)
                )),
            }
        }
    });
    w.blank();

    let sig = |q: &QueryDef, im: &mut Imports| {
        let ps = params(q, im);
        match q.result {
            QueryResult::Stream => format!("fun {}({ps}): Flow<List<{name}>>", ident(&q.name)),
            QueryResult::List => format!("suspend fun {}({ps}): List<{name}>", ident(&q.name)),
            QueryResult::One => format!("suspend fun {}({ps}): {name}?", ident(&q.name)),
        }
    };
    w.line(format!("/** Data access for `{name}` */"));
    w.block(format!("interface {} {{", s.name), "}", |w| {
        w.line(format!("fun all(): Flow<List<{name}>>"));
        w.line(format!("suspend fun find({pk_param}): {name}?"));
        w.line(format!("suspend fun save(item: {name})"));
        w.line(format!("suspend fun delete({pk_param})"));
        for q in &s.queries {
            w.line(sig(q, &mut im));
        }
    });
    w.blank();

    w.line(format!("/** Room implementation of `{}` */", s.name));
    w.block(format!("class Room{0}(private val dao: {dao}) : {0} {{", s.name), "}", |w| {
        let pk_name = ident(&pk.name);
        w.line(format!("override fun all(): Flow<List<{name}>> = dao.all().map {{ rows -> rows.map {{ it.toModel() }} }}"));
        w.line(format!("override suspend fun find({pk_param}): {name}? = dao.find({pk_name})?.toModel()"));
        w.line(format!("override suspend fun save(item: {name}) = dao.save(item.toEntity())"));
        w.line(format!("override suspend fun delete({pk_param}) = dao.delete({pk_name})"));
        for q in &s.queries {
            let args: Vec<_> = q.params.iter().map(|p| ident(&p.name)).collect();
            let call = format!("dao.{}({})", ident(&q.name), args.join(", "));
            let body = match q.result {
                QueryResult::Stream => format!("{call}.map {{ rows -> rows.map {{ it.toModel() }} }}"),
                QueryResult::List => format!("{call}.map {{ it.toModel() }}"),
                QueryResult::One => format!("{call}?.toModel()"),
            };
            w.line(format!("override {} = {body}", sig(q, &mut im)));
        }
    });
    generated(&o.package, &im, &w.finish())
}

pub(crate) fn database(program: &Program, o: &Options) -> String {
    let mut im = Imports::new(o.r_package());
    for i in [
        "android.content.Context",
        "androidx.room.Database",
        "androidx.room.Room",
        "androidx.room.RoomDatabase",
        "androidx.room.TypeConverter",
        "androidx.room.TypeConverters",
        "java.time.Instant",
    ] {
        im.add(i);
    }
    let mut w = CodeWriter::new("    ");
    let entities: Vec<_> = program
        .entities
        .iter()
        .map(|e| format!("{}Entity::class", e.name))
        .collect();
    w.line(format!(
        "@Database(entities = [{}], version = {}, exportSchema = true)",
        entities.join(", "),
        o.database_version
    ));
    w.line("@TypeConverters(VeyraConverters::class)");
    w.block(
        "abstract class VeyraDatabase : RoomDatabase() {",
        "}",
        |w| {
            for s in &program.stores {
                w.line(format!(
                    "abstract fun {}(): {}Dao",
                    camel_case(&format!("{}Dao", s.name)),
                    s.name
                ));
            }
            w.blank();
            w.block("companion object {", "}", |w| {
                w.line("fun build(context: Context): VeyraDatabase =");
                w.indent();
                w.line(format!(
                    "Room.databaseBuilder(context, VeyraDatabase::class.java, {})",
                    kotlin::string_literal(&o.database)
                ));
                w.indent();
                w.line(".addMigrations(*VeyraMigrations.all)");
                w.line(".build()");
                w.dedent();
                w.dedent();
            });
        },
    );
    w.blank();
    w.block("class VeyraConverters {", "}", |w| {
        w.line("@TypeConverter");
        w.line("fun instantToMillis(value: Instant?): Long? = value?.toEpochMilli()");
        w.blank();
        w.line("@TypeConverter");
        w.line("fun millisToInstant(value: Long?): Instant? = value?.let(Instant::ofEpochMilli)");
    });
    w.blank();
    w.line("/** Default Room stores, useful when implementing `VeyraDependencies` */");
    w.block("class VeyraStores(database: VeyraDatabase) {", "}", |w| {
        for s in &program.stores {
            w.line(format!(
                "val {}: {} = Room{}(database.{}())",
                dependency_name(&s.name),
                s.name,
                s.name,
                camel_case(&format!("{}Dao", s.name))
            ));
        }
    });
    generated(&o.package, &im, &w.finish())
}

/// Hilt模式下为store、settings与secure提供默认实现
pub(crate) fn hilt_module(program: &Program, o: &Options) -> String {
    let mut im = Imports::new(o.r_package());
    for i in [
        "android.content.Context",
        "dagger.Module",
        "dagger.Provides",
        "dagger.hilt.InstallIn",
        "dagger.hilt.android.qualifiers.ApplicationContext",
        "dagger.hilt.components.SingletonComponent",
        "javax.inject.Singleton",
    ] {
        im.add(i);
    }
    let mut w = CodeWriter::new("    ");
    w.line("@Module");
    w.line("@InstallIn(SingletonComponent::class)");
    w.block("object VeyraLocalModule {", "}", |w| {
        let mut first = true;
        let mut sep = |w: &mut CodeWriter| {
            if !first {
                w.blank();
            }
            first = false;
        };
        if !program.entities.is_empty() {
            sep(w);
            w.line("@Provides");
            w.line("@Singleton");
            w.line("fun provideDatabase(@ApplicationContext context: Context): VeyraDatabase = VeyraDatabase.build(context)");
            for s in &program.stores {
                w.blank();
                w.line("@Provides");
                w.line(format!(
                    "fun provide{0}(database: VeyraDatabase): {0} = Room{0}(database.{1}())",
                    s.name,
                    camel_case(&format!("{}Dao", s.name))
                ));
            }
        }
        for s in &program.settings {
            sep(w);
            w.line("@Provides");
            w.line("@Singleton");
            w.line(format!("fun provide{0}(@ApplicationContext context: Context): {0} = DataStore{0}(context)", s.name));
        }
        for s in &program.secures {
            sep(w);
            w.line("@Provides");
            w.line("@Singleton");
            w.line(format!("fun provide{0}(@ApplicationContext context: Context): {0} = Keystore{0}(context)", s.name));
        }
    });
    generated(&o.package, &im, &w.finish())
}

pub(crate) fn migrations(o: &Options) -> String {
    let mut im = Imports::new(o.r_package());
    im.add("androidx.room.migration.Migration");
    let body = "/** Migrations for `VeyraDatabase`, raise `database_version` in veyra.toml when the schema changes */\nobject VeyraMigrations {\n    val all: Array<Migration> = arrayOf()\n}\n";
    assemble(&o.package, &im, body, USER_OWNED_NOTICE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use veyra_ir::Type;

    #[test]
    fn builds_sql() {
        let p = Predicate::And(
            Box::new(Predicate::Contains {
                field: "title".into(),
                value: Operand::Param("q".into()),
            }),
            Box::new(Predicate::Compare {
                field: "name".into(),
                op: CompareOp::Eq,
                value: Operand::Literal(Expr::new(ExprKind::String("it's".into()), Type::String)),
            }),
        );
        assert_eq!(sql(&p), "(title LIKE '%' || :q || '%' AND name = 'it''s')");
    }
}
