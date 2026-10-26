// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

/// 按`.`、`_`、`-`切分单词
fn words(s: &str) -> Vec<String> {
    s.split(['.', '_', '-', ' '])
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `schedule.course_count` -> `ScheduleCourseCount`
pub fn pascal_case(s: &str) -> String {
    words(s).iter().map(|w| capitalize(w)).collect()
}

/// `schedule.course_count` -> `scheduleCourseCount`
pub fn camel_case(s: &str) -> String {
    let p = pascal_case(s);
    let mut chars = p.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `schedule.courseCount` -> `schedule_course_count`
pub fn snake_case(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c == '.' || c == '-' || c == ' ' {
            out.push('_');
        } else if c.is_ascii_uppercase() {
            if i > 0 && !out.ends_with('_') {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// 与语言关键字冲突时加反引号
pub fn escape_ident(name: &str, keywords: &[&str]) -> String {
    if keywords.contains(&name) {
        format!("`{name}`")
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_cases() {
        assert_eq!(pascal_case("schedule.course_count"), "ScheduleCourseCount");
        assert_eq!(camel_case("schedule.course_count"), "scheduleCourseCount");
        assert_eq!(camel_case("CourseRepository"), "courseRepository");
        assert_eq!(snake_case("schedule.courseCount"), "schedule_course_count");
        assert_eq!(escape_ident("default", &["default"]), "`default`");
    }
}
