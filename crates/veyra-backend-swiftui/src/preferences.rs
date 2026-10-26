// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 简单配置与安全凭证：UserDefaults与Keychain

use veyra_codegen::{CodeWriter, pascal_case};
use veyra_ir::{Program, SecureDef, SettingsDef, Type};

use crate::header;
use crate::swift::{self, ident};

/// 从UserDefaults读取的表达式
fn read(t: &Type, key: &str, default: &str) -> String {
    let lit = swift::string_literal(key);
    match t {
        Type::Enum(e) => {
            format!("defaults.string(forKey: {lit}).flatMap({e}.init(rawValue:)) ?? {default}")
        }
        Type::Optional(inner) => match &**inner {
            Type::Enum(e) => format!("defaults.string(forKey: {lit}).flatMap({e}.init(rawValue:))"),
            other => format!("defaults.object(forKey: {lit}) as? {}", swift::ty(other)),
        },
        other => format!(
            "defaults.object(forKey: {lit}) as? {} ?? {default}",
            swift::ty(other)
        ),
    }
}

fn write_value(t: &Type) -> &'static str {
    match t {
        Type::Enum(_) => "newValue.rawValue",
        Type::Optional(inner) if matches!(**inner, Type::Enum(_)) => "newValue?.rawValue",
        _ => "newValue",
    }
}

pub(crate) fn settings(s: &SettingsDef) -> String {
    let mut w = CodeWriter::new("    ");
    header(&mut w, &["Foundation"]);
    w.line(format!("/// Simple preferences of `{}`", s.name));
    w.line("@MainActor");
    w.block(format!("protocol {}: AnyObject {{", s.name), "}", |w| {
        for f in &s.fields {
            w.line(format!(
                "var {}: {} {{ get set }}",
                ident(&f.name),
                swift::ty(&f.ty)
            ));
        }
        w.blank();
        w.line("/// Emits whenever any value changes");
        w.line("func changes() -> AsyncStream<Void>");
    });
    w.blank();
    w.line(format!("/// `UserDefaults` implementation of `{}`", s.name));
    w.line("@MainActor");
    w.block(format!("final class UserDefaults{0}: {0} {{", s.name), "}", |w| {
        w.line("private let defaults: UserDefaults");
        w.blank();
        w.block("init(defaults: UserDefaults = .standard) {", "}", |w| w.line("self.defaults = defaults"));
        for f in &s.fields {
            let key = format!("{}.{}", s.name, f.name);
            let default = crate::default_value(&f.default);
            w.blank();
            w.block(format!("var {}: {} {{", ident(&f.name), swift::ty(&f.ty)), "}", |w| {
                w.line(format!("get {{ {} }}", read(&f.ty, &key, &default)));
                w.line(format!(
                    "set {{ defaults.set({}, forKey: {}) }}",
                    write_value(&f.ty),
                    swift::string_literal(&key)
                ));
            });
        }
        w.blank();
        w.block("func changes() -> AsyncStream<Void> {", "}", |w| {
            w.block("AsyncStream { continuation in", "}", |w| {
                w.line("let token = NotificationCenter.default.addObserver(forName: UserDefaults.didChangeNotification, object: defaults, queue: .main) { _ in");
                w.indent();
                w.line("continuation.yield()");
                w.dedent();
                w.line("}");
                w.line("let observer = VeyraUncheckedBox(token)");
                w.block("continuation.onTermination = { _ in", "}", |w| {
                    w.line("NotificationCenter.default.removeObserver(observer.value)");
                });
            });
        });
    });
    w.finish()
}

/// Keychain中字符串到字段类型的转换
fn decode(t: &Type) -> &'static str {
    match t {
        Type::Int => ".flatMap { Int($0) }",
        Type::Double => ".flatMap { Double($0) }",
        Type::Bool => ".map { $0 == \"true\" }",
        _ => "",
    }
}

fn encode(t: &Type) -> &'static str {
    match t {
        Type::String => "value",
        _ => "value.map { String($0) }",
    }
}

pub(crate) fn secure(s: &SecureDef) -> String {
    let mut w = CodeWriter::new("    ");
    header(&mut w, &["Foundation"]);
    w.line(format!(
        "/// Credentials of `{}`, values are nil until written",
        s.name
    ));
    w.line("@MainActor");
    w.block(format!("protocol {}: AnyObject {{", s.name), "}", |w| {
        for f in &s.fields {
            let t = swift::ty(&f.ty);
            w.line(format!("func {}() throws -> {t}?", ident(&f.name)));
            w.line(format!(
                "func set{}(_ value: {t}?) throws",
                pascal_case(&f.name)
            ));
        }
    });
    w.blank();
    w.line(format!("/// Keychain implementation of `{}`", s.name));
    w.line("@MainActor");
    w.block(
        format!("final class Keychain{0}: {0} {{", s.name),
        "}",
        |w| {
            w.line("private let keychain: VeyraKeychain");
            w.blank();
            w.line(
                "/// Items are stored under the bundle identifier followed by the declaration name",
            );
            w.block(
                format!(
                    "init(service: String = (Bundle.main.bundleIdentifier ?? \"veyra\") + {}) {{",
                    swift::string_literal(&format!(".{}", s.name))
                ),
                "}",
                |w| w.line("keychain = VeyraKeychain(service: service)"),
            );
            for f in &s.fields {
                let t = swift::ty(&f.ty);
                let key = swift::string_literal(&f.name);
                w.blank();
                w.block(
                    format!("func {}() throws -> {t}? {{", ident(&f.name)),
                    "}",
                    |w| {
                        w.line(format!("try keychain.read({key}){}", decode(&f.ty)));
                    },
                );
                w.blank();
                w.block(
                    format!("func set{}(_ value: {t}?) throws {{", pascal_case(&f.name)),
                    "}",
                    |w| {
                        w.line(format!("try keychain.write({key}, {})", encode(&f.ty)));
                    },
                );
            }
        },
    );
    w.finish()
}

/// Keychain读写与通用工具
pub(crate) fn support(program: &Program) -> String {
    let mut w = CodeWriter::new("    ");
    header(&mut w, &["Foundation", "Security"]);
    w.line("/// Holds values that are only used on the main actor inside sendable closures");
    w.block(
        "final class VeyraUncheckedBox<Value>: @unchecked Sendable {",
        "}",
        |w| {
            w.line("let value: Value");
            w.blank();
            w.block("init(_ value: Value) {", "}", |w| {
                w.line("self.value = value")
            });
        },
    );
    if program.secures.is_empty() {
        return w.finish();
    }
    w.blank();
    w.block("struct VeyraKeychainError: Error {", "}", |w| {
        w.line("let status: OSStatus")
    });
    w.blank();
    w.line("/// Generic password items in the keychain");
    w.block("struct VeyraKeychain: Sendable {", "}", |w| {
        w.line("let service: String");
        w.blank();
        w.block("private func query(_ account: String) -> [String: Any] {", "}", |w| {
            w.line("[kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: account]");
        });
        w.blank();
        w.block("func read(_ account: String) throws -> String? {", "}", |w| {
            w.line("var query = query(account)");
            w.line("query[kSecReturnData as String] = true");
            w.line("query[kSecMatchLimit as String] = kSecMatchLimitOne");
            w.line("var result: AnyObject?");
            w.line("let status = SecItemCopyMatching(query as CFDictionary, &result)");
            w.line("if status == errSecItemNotFound { return nil }");
            w.line("guard status == errSecSuccess, let data = result as? Data else { throw VeyraKeychainError(status: status) }");
            w.line("return String(data: data, encoding: .utf8)");
        });
        w.blank();
        w.block("func write(_ account: String, _ value: String?) throws {", "}", |w| {
            w.line("let base = query(account)");
            w.line("let deleted = SecItemDelete(base as CFDictionary)");
            w.line("guard deleted == errSecSuccess || deleted == errSecItemNotFound else { throw VeyraKeychainError(status: deleted) }");
            w.line("guard let value else { return }");
            w.line("var item = base");
            w.line("item[kSecValueData as String] = Data(value.utf8)");
            w.line("item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock");
            w.line("let status = SecItemAdd(item as CFDictionary, nil)");
            w.line("guard status == errSecSuccess else { throw VeyraKeychainError(status: status) }");
        });
    });
    w.finish()
}
