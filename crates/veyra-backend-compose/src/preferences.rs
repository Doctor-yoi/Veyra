// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

//! 简单配置与安全凭证：DataStore与Keystore

use veyra_codegen::{CodeWriter, pascal_case, snake_case};
use veyra_ir::{SecureDef, SettingsDef, Type};

use crate::kotlin::{self, ExprCx, Imports, ident};
use crate::{Options, generated};

/// DataStore键的构造函数与存储类型
fn key_kind(t: &Type) -> (&'static str, &'static str) {
    match t.unwrap_optional() {
        Type::Bool => ("booleanPreferencesKey", "Boolean"),
        Type::Int => ("intPreferencesKey", "Int"),
        Type::Double => ("doublePreferencesKey", "Double"),
        Type::Date => ("longPreferencesKey", "Long"),
        _ => ("stringPreferencesKey", "String"),
    }
}

/// 存储值转为字段值
fn decode(t: &Type, raw: &str) -> String {
    match t.unwrap_optional() {
        Type::Enum(e) => {
            format!("{raw}?.let {{ name -> {e}.entries.firstOrNull {{ it.name == name }} }}")
        }
        Type::Date => format!("{raw}?.let(Instant::ofEpochMilli)"),
        _ => raw.to_string(),
    }
}

fn encode(t: &Type) -> &'static str {
    match t.unwrap_optional() {
        Type::Enum(_) => "value.name",
        Type::Date => "value.toEpochMilli()",
        _ => "value",
    }
}

pub(crate) fn settings(s: &SettingsDef, o: &Options) -> String {
    let mut im = Imports::new(o.r_package());
    for i in [
        "android.content.Context",
        "androidx.datastore.core.DataStore",
        "androidx.datastore.preferences.core.Preferences",
        "androidx.datastore.preferences.core.edit",
        "androidx.datastore.preferences.preferencesDataStore",
        "kotlinx.coroutines.flow.Flow",
        "kotlinx.coroutines.flow.map",
    ] {
        im.add(i);
    }
    let store = format!(
        "{}DataStore",
        kotlin::ident(&veyra_codegen::camel_case(&s.name))
    );
    let mut w = CodeWriter::new("    ");
    w.line(format!("/** Simple preferences of `{}` */", s.name));
    w.block(format!("interface {} {{", s.name), "}", |w| {
        for f in &s.fields {
            let t = kotlin::ty(&f.ty, &mut im);
            w.line(format!("val {}: Flow<{t}>", ident(&f.name)));
            w.line(format!(
                "suspend fun set{}(value: {t})",
                pascal_case(&f.name)
            ));
        }
    });
    w.blank();
    w.line(format!(
        "private val Context.{store}: DataStore<Preferences> by preferencesDataStore(name = {})",
        kotlin::string_literal(&snake_case(&s.name))
    ));
    w.blank();
    w.line(format!("/** DataStore implementation of `{}` */", s.name));
    w.block(
        format!("class DataStore{0}(context: Context) : {0} {{", s.name),
        "}",
        |w| {
            w.line(format!(
                "private val store = context.applicationContext.{store}"
            ));
            for f in &s.fields {
                let (key_fn, _) = key_kind(&f.ty);
                im.add(&format!("androidx.datastore.preferences.core.{key_fn}"));
                if matches!(f.ty.unwrap_optional(), Type::Date) {
                    im.add("java.time.Instant");
                }
                let t = kotlin::ty(&f.ty, &mut im);
                let key = format!("{}Key", f.name);
                let default = kotlin::expr(ExprCx { state: "" }, &f.default, &mut im);
                let raw = format!("it[{key}]");
                let read = decode(&f.ty, &raw);
                let read = match (f.ty.is_optional(), read == raw) {
                    (true, _) => read,
                    (false, true) => format!("{read} ?: {default}"),
                    (false, false) => format!("({read}) ?: {default}"),
                };
                w.blank();
                w.line(format!(
                    "private val {key} = {key_fn}({})",
                    kotlin::string_literal(&f.name)
                ));
                w.line(format!(
                    "override val {}: Flow<{t}> = store.data.map {{ {read} }}",
                    ident(&f.name)
                ));
                w.blank();
                w.block(
                    format!(
                        "override suspend fun set{}(value: {t}) {{",
                        pascal_case(&f.name)
                    ),
                    "}",
                    |w| {
                        if f.ty.is_optional() {
                            w.line(format!(
                        "store.edit {{ if (value == null) it.remove({key}) else it[{key}] = {} }}",
                        encode(&f.ty)
                    ));
                        } else {
                            w.line(format!("store.edit {{ it[{key}] = {} }}", encode(&f.ty)));
                        }
                    },
                );
            }
        },
    );
    generated(&o.package, &im, &w.finish())
}

fn decode_secure(t: &Type) -> &'static str {
    match t {
        Type::Int => "?.toIntOrNull()",
        Type::Double => "?.toDoubleOrNull()",
        Type::Bool => "?.toBooleanStrictOrNull()",
        _ => "",
    }
}

pub(crate) fn secure(s: &SecureDef, o: &Options) -> String {
    let mut im = Imports::new(o.r_package());
    im.add("android.content.Context");
    let mut w = CodeWriter::new("    ");
    w.line(format!(
        "/** Credentials of `{}`, values are null until written */",
        s.name
    ));
    w.block(format!("interface {} {{", s.name), "}", |w| {
        for f in &s.fields {
            let t = kotlin::ty(&f.ty, &mut im);
            w.line(format!("suspend fun {}(): {t}?", ident(&f.name)));
            w.line(format!(
                "suspend fun set{}(value: {t}?)",
                pascal_case(&f.name)
            ));
        }
    });
    w.blank();
    w.line(format!(
        "/** Implementation of `{}` encrypted with an Android Keystore key */",
        s.name
    ));
    w.block(
        format!("class Keystore{0}(context: Context) : {0} {{", s.name),
        "}",
        |w| {
            w.line(format!(
                "private val vault = VeyraVault(context, {})",
                kotlin::string_literal(&snake_case(&s.name))
            ));
            for f in &s.fields {
                let t = kotlin::ty(&f.ty, &mut im);
                let key = kotlin::string_literal(&f.name);
                let write = if f.ty == Type::String {
                    "value".to_string()
                } else {
                    "value?.toString()".to_string()
                };
                w.blank();
                w.line(format!(
                    "override suspend fun {}(): {t}? = vault.read({key}){}",
                    ident(&f.name),
                    decode_secure(&f.ty)
                ));
                w.blank();
                w.line(format!(
                    "override suspend fun set{}(value: {t}?) = vault.write({key}, {write})",
                    pascal_case(&f.name)
                ));
            }
        },
    );
    generated(&o.package, &im, &w.finish())
}

/// 基于Keystore AES-GCM密钥的加密存储，密文保存在SharedPreferences中
pub(crate) fn vault(o: &Options) -> String {
    let mut im = Imports::new(o.r_package());
    for i in [
        "android.content.Context",
        "android.security.keystore.KeyGenParameterSpec",
        "android.security.keystore.KeyProperties",
        "android.util.Base64",
        "java.security.KeyStore",
        "javax.crypto.Cipher",
        "javax.crypto.KeyGenerator",
        "javax.crypto.SecretKey",
        "javax.crypto.spec.GCMParameterSpec",
        "kotlinx.coroutines.Dispatchers",
        "kotlinx.coroutines.withContext",
    ] {
        im.add(i);
    }
    let body = r#"/** Encrypts values with an AES-GCM key kept in the Android Keystore */
internal class VeyraVault(context: Context, name: String) {
    private val prefs = context.applicationContext.getSharedPreferences("veyra_secure_$name", Context.MODE_PRIVATE)
    private val alias = "veyra_secure_$name"

    private fun key(): SecretKey {
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (keyStore.getKey(alias, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        val spec = KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .build()
        generator.init(spec)
        return generator.generateKey()
    }

    suspend fun read(field: String): String? = withContext(Dispatchers.IO) {
        val stored = prefs.getString(field, null) ?: return@withContext null
        val bytes = Base64.decode(stored, Base64.NO_WRAP)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes, 0, IV_SIZE))
        String(cipher.doFinal(bytes, IV_SIZE, bytes.size - IV_SIZE), Charsets.UTF_8)
    }

    suspend fun write(field: String, value: String?) {
        withContext(Dispatchers.IO) {
            if (value == null) {
                prefs.edit().remove(field).apply()
                return@withContext
            }
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, key())
            val encrypted = cipher.iv + cipher.doFinal(value.toByteArray(Charsets.UTF_8))
            prefs.edit().putString(field, Base64.encodeToString(encrypted, Base64.NO_WRAP)).apply()
        }
    }

    private companion object {
        const val IV_SIZE = 12
    }
}
"#;
    generated(&o.package, &im, body)
}
