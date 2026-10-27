// SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
// Copyright (C) 2026 Doctor-yoi

use veyra_diagnostics::{RenderOptions, SourceMap, render_all};
use veyra_syntax::ast::*;
use veyra_syntax::parse;

const SCHEDULE: &str = r#"
import "models.vey"
import (
  "stores/course.vey"
  "../shared/theme.vey"
)

// 课程类型
enum CourseKind { lecture, lab }

entity Course @persistent {
  id: String @primaryKey
  name: String
  credits: Double @indexed
  kind: CourseKind
}

store CourseStore for Course {
  query search(q: String) -> Stream<List<Course>> {
    where name.contains(q) && credits >= 1.5
    orderBy name asc, credits desc
    limit 50
  }
}

repository CourseRepository {
  getCourses() async throws -> List<Course>
  observeCourses() -> Stream<List<Course>>
}

component CourseRow(course: Course, onTap: Action) {
  Row(spacing: 8) {
    Text(course.name)
    Spacer()
  }
  .onTap(onTap)
}

screen Schedule {
  inject courses: CourseStore

  state {
    query: String = "" @writable
    items: List<Course> = []
    #if ios
    useFaceID: Bool = false @writable
    #endif
  }

  computed {
    count: Int = items.count
  }

  actions {
    refresh() async
    openCourse(id: String)
  }

  lifecycle {
    onAppear: refresh
  }

  view {
    Column(spacing: 16) {
      Text(loc("schedule.title", "我的课表")).font(.title)
      TextField(loc("schedule.search", "搜索"), text: bind(query))
        #if ios
        .swiftui.glassEffect()
        #endif
      if items.isEmpty {
        Text("empty")
      } else if count > 1 {
        List(items, key: .id) { course in
          CourseRow(course: course, onTap: openCourse(course.id))
        }
      } else {
        Spacer()
      }
      #if ios && !debug
      native swiftui {
        """
        Text(verbatim: "{{query}}")
        """
      }
      #elseif android
      Divider()
      #else
      Spacer()
      #endif
    }
  }
}
"#;

fn render(src: &str) -> String {
    let mut map = SourceMap::new();
    let id = map.add("test.vey", src);
    let parsed = parse(id, src);
    render_all(&parsed.diagnostics, &map, RenderOptions::default())
}

#[test]
fn parses_full_example() {
    let parsed = parse(Default::default(), SCHEDULE);
    assert!(parsed.diagnostics.is_empty(), "{}", render(SCHEDULE));
    assert_eq!(parsed.file.items.len(), 6);
    let imports: Vec<_> = parsed
        .file
        .imports
        .iter()
        .map(|i| i.path.as_str())
        .collect();
    assert_eq!(
        imports,
        ["models.vey", "stores/course.vey", "../shared/theme.vey"]
    );
    assert_eq!(parsed.comments.len(), 1);

    let Node::Item(Item::Screen(screen)) = &parsed.file.items[5] else {
        panic!()
    };
    assert_eq!(screen.name.name, "Schedule");
    let view = screen
        .members
        .iter()
        .find_map(|m| match m {
            Node::Item(ScreenMember::View(v)) => Some(v),
            _ => None,
        })
        .unwrap();
    let Node::Item(ViewNode::Element(column)) = &view.items[0] else {
        panic!()
    };
    let body = &column.closure.as_ref().unwrap().body;
    assert_eq!(body.len(), 4);
    let Node::Item(ViewNode::Element(field)) = &body[1] else {
        panic!()
    };
    assert!(matches!(field.modifiers[0], Node::Cfg(_)));
    let Node::Cfg(group) = &body[3] else { panic!() };
    assert_eq!(group.branches.len(), 3);
    let Node::Item(ViewNode::Native(native)) = &group.branches[0].items[0] else {
        panic!()
    };
    assert_eq!(native.code, "Text(verbatim: \"{{query}}\")");
}

#[test]
fn reports_syntax_errors() {
    insta::assert_snapshot!(render(
        r#"
screen A {
  state {
    a: String = "
    b: Int = 1
  }
  view {
    Text("x"
    Button("ok", action: go)
  }
  unknown {}
}
#if ios
model M { a: Int }
"#
    ));
}

#[test]
fn rejects_unknown_item() {
    insta::assert_snapshot!(render("widget Foo {}\nenum E { a b }\n#endif\n"));
}

#[test]
fn rejects_late_import() {
    insta::assert_snapshot!(render(
        "model A { a: Int }\nimport \"b.vey\"\nmodel C { c: Int }\n"
    ));
}
