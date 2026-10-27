# Veyra Syntax Reference

English | [简体中文](syntax.zh-CN.md)

This document describes the `.vey` language as accepted by the current compiler. Diagnostic codes such as `V2001` are listed where a rule is enforced, so an error message can be traced back to the rule that produced it.

## Lexical structure

Source files are UTF-8. Line breaks carry no meaning, and statements need no separators.

```
// line comment
/* block comment, /* which may be nested */ */
```

Identifiers start with a letter or `_`, followed by letters, digits or `_`. Keywords such as `screen`, `state` and `view` are contextual: they only act as keywords where a declaration or block is expected, so they remain usable as field names.

| Literal | Examples | Notes |
| --- | --- | --- |
| Integer | `0`, `42`, `-1` | Becomes a `Double` where a `Double` is expected |
| Float | `0.5`, `1.25` | |
| String | `"Hello"`, `"a\nb"` | Escapes: `\n` `\t` `\r` `\"` `\\` `\{` `\}` |
| Multi-line string | `"""` ... `"""` | Only used in `native` blocks, common indentation is removed |
| Boolean | `true`, `false` | |
| Nil | `nil` | Only valid for optional types |
| List | `[]`, `[1, 2, 3]` | |

`{name}` inside a localized string is a placeholder. Write `\{` and `\}` for literal braces.

## Files and imports

A file contains optional imports followed by declarations. Each file sees its own declarations and those of the files it imports directly. Imports are not transitive.

```
import "models.vey"
import (
  "stores/NoteStore.vey"
  "../shared/Theme.vey"
)
```

Paths are relative to the current file. Using a declaration from a file that is not imported is an error (V1034), and an unused import is a warning. All declarations share one namespace, so two files cannot declare the same name (V1001).

## Types

| Type | Description |
| --- | --- |
| `Bool`, `Int`, `Double`, `String` | Primitive values |
| `Date` | A point in time, `Date` in Swift and `java.time.Instant` in Kotlin |
| `List<T>` | Ordered list |
| `T?` | Optional value, which cannot be nested (V2003) |
| `Stream<T>` | Observed values, only allowed as a return type of `repository` and `store` methods (V2005) |
| Enum or model name | A declared `enum`, `model` or `entity` |

Names reserved by the platform SDKs, such as `View`, `Task` and `Context`, cannot be used for declarations (V1002).

## Declarations

### enum

```
enum CourseKind { lecture, lab, seminar }
```

Cases are written as `.lecture` when the type is known from context, or as `CourseKind.lecture`.

### model

A model is an immutable value type shared by every layer. Fields have no default values (V2041), and a model cannot contain itself directly (V2012).

```
model Course {
  id: String
  name: String
  teacher: String?
  credits: Double
  kind: CourseKind
}
```

### repository

A repository is a data access contract that each platform implements natively and injects into view models.

```
repository CourseRepository {
  getCourses() async throws -> List<Course>
  observeCourses() -> Stream<List<Course>>
}
```

A method returning `Stream` cannot be `async` (V2050).

### entity and store

An entity is a persistent record generated for SwiftData and Room. It needs exactly one `@primaryKey` field of type `String` or `Int` (V5002, V5003). Fields can also be marked `@indexed` or `@unique`, and cannot have default values.

```
entity Note @persistent {
  id: String @primaryKey
  courseId: String @indexed
  text: String
  createdAt: Date
}
```

A store is the data access object of one entity. Every store provides `all`, `find`, `save` and `delete`, and declares extra queries:

```
store NoteStore for Note {
  query forCourse(course: String) -> Stream<List<Note>> {
    where courseId == course
    orderBy createdAt desc
    limit 50
  }

  query latest() -> Note? {
    orderBy createdAt desc
  }
}
```

| Clause | Description |
| --- | --- |
| `where` | Comparisons, `contains`, `== nil`, Bool fields, `&&`, `||` and `!` over entity fields and query parameters |
| `orderBy` | One or more fields separated by commas, each optionally followed by `asc` or `desc` |
| `limit` | Maximum number of results |

The return type decides how a query runs: `Stream<List<T>>` observes changes, `List<T>` reads once and `T?` reads the first match. Views and view models see the value type `Note`, never the persistent `NoteEntity`.

### settings and secure

```
settings AppPreferences {
  reminder: Bool = true
  volume: Double = 0.6
  lastSync: Date?
}

secure Account {
  token: String
}
```

`settings` fields can be `Bool`, `Int`, `Double`, `String`, `Date`, enums and their optionals (V5020). Every non-optional field needs a default value (V2010), and `Date` fields must be optional (V5021). `secure` fields can be `String`, `Int`, `Double` or `Bool`, are always read as optionals and have no defaults. Both are injected into view models like repositories.

### component

A component is a reusable view without business state. Its body is a view tree.

```
component CourseRow(course: Course, note: String?, onTap: Action) {
  Row(spacing: 8) {
    Text(course.name).font(.headline)
    Spacer()
    Text(note ?? "").foreground(.secondary)
  }
  .onTap(onTap)
}
```

| Parameter type | Description |
| --- | --- |
| A value type | Read-only input |
| `Action` | Callback, passed as an action reference such as `openCourse(course.id)` |
| `Binding<T>` | Two-way binding, passed as `bind(field)` or another binding parameter |

Arguments are always passed by name (V2073). Optional value parameters may be omitted and default to `nil`.

```
CourseRow(course: course, note: nil, onTap: openCourse(course.id))
```

### screen

A screen is a page with its own view model. Members can appear in any order.

```
screen Schedule {
  inject courses: CourseRepository

  state {
    query: String = "" @writable
    items: List<Course> = []
    isRefreshing: Bool = false
    updatedAt: Date = now()
  }

  computed {
    count: Int = items.count
    canRefresh: Bool = !isRefreshing
  }

  actions {
    refresh() async
    openCourse(id: String)
  }

  effects {
    showCourse(id: String)
    close
  }

  lifecycle {
    onAppear: refresh
  }

  view {
    Text(loc("schedule.count", "{count} courses", count: count))
  }
}
```

| Member | Rules |
| --- | --- |
| `inject name: Type` | A repository, store, `settings` or `secure` type. Only the view model receives it, the view never sees it |
| `state` | Fields with constant defaults: literals, list literals, enum cases, `nil` and `now()` (V2011). Optional fields default to `nil`. `@writable` allows bindings and generates a setter |
| `computed` | Pure expressions over state and other computed fields, without cycles (V3007) |
| `actions` | Commands implemented by the view model. They may be `async`, but cannot return values (V3018) or `throws` (V3019) |
| `effects` | One-off events such as navigation, emitted by the view model with `emit` and handled by the root view or route through `onEffect`. They cannot be `async`, `throws` or return values, and views cannot trigger them (V3033) |
| `lifecycle` | `onAppear` and `onDisappear`, each dispatching one action |
| `view` | Exactly one non-empty view block (V3020, V3021) |

A writable field generates a setter named `setQuery` for `query`, which cannot collide with an action name.

## Expressions

Expressions in views and computed fields are pure. They read state, computed fields, component parameters and closure parameters, and cannot call actions or functions.

| Precedence | Operators | Description |
| --- | --- | --- |
| 1 | `\|\|` | Logical or |
| 2 | `&&` | Logical and |
| 3 | `==` `!=` `<` `<=` `>` `>=` | Comparison |
| 4 | `??` | Default for optionals, `teacher ?? "-"` |
| 5 | `+` `-` | Addition, subtraction, `String` concatenation |
| 6 | `*` `/` `%` | Multiplication, division, remainder |
| Prefix | `!` `-` | Not, negation |

`cond ? a : b` selects a value and binds looser than all binary operators. Parentheses group as usual.

| Member | Applies to | Result |
| --- | --- | --- |
| `.count` | `List<T>`, `String` | `Int` |
| `.isEmpty` | `List<T>`, `String` | `Bool` |
| `.first` | `List<T>` | `T?` |
| `.contains(x)` | `String` | `Bool` |
| `.field` | model | The field value |

| Function | Description |
| --- | --- |
| `loc(key, text, name: value...)` | Localized text, only in views |
| `plural(key, count, one: ..., other: ...)` | Localized plural text, only in views |
| `format(value, style)` | Locale-aware formatting into a `String` |
| `now()` | The current time, also valid as a state default |
| `bind(field)` | A two-way binding to a `@writable` state, only as a binding argument |

## Views

A view is a tree of components, each written as a call with optional arguments, an optional trailing closure and a chain of modifiers:

```
Column(spacing: 12, alignment: .leading) {
  Text(title).font(.headline)
  Button(loc("home.refresh", "Refresh"), action: refresh)
    .disabled(isRefreshing)
}
.padding(16)
```

Arguments are matched by label, and unlabeled arguments fill positional parameters in order. Missing required arguments (V2032), unknown labels (V2030) and extra positional arguments (V2035) are errors. Implicit members such as `.leading` and `.infinity` select fixed values.

Collections pass each element to the closure as a parameter:

```
List(items, key: .id) { course in
  CourseRow(course: course, note: nil, onTap: openCourse(course.id))
}
```

`key` names a field of the element model that identifies it (V2037).

### Reading, binding and dispatching

| Form | Example | Rule |
| --- | --- | --- |
| Read | `Text(query)` | Any pure expression of the expected type |
| Bind | `TextField(..., text: bind(query))` | The state must be `@writable` (V3001), and its type must match the control |
| Dispatch | `Button(..., action: openCourse(course.id))` | An action name, with arguments checked against its parameters |

An action without parameters can be passed by name, `action: refresh`. Arguments follow the parameter order or use labels, `openCourse(id: course.id)`. Async actions are started in a task automatically. A component's `Action` parameter is forwarded by name, `.onTap(onTap)`.

### Conditions

```
if isRefreshing {
  Progress()
} else if items.isEmpty {
  Text(loc("schedule.empty", "No courses"))
} else {
  List(items, key: .id) { course in
    Text(course.name)
  }
}
```

The condition is a `Bool` expression. `if available(...)` checks the system version at run time on Apple platforms, and Android always takes the first branch:

```
if available(ios: 26, macos: 26) {
  Text(title).swiftui.glassEffect(.regular, in: .capsule)
} else {
  Text(title)
}
```

### Built-in components

Built-in components are available on every platform. Parameters marked `_` are positional.

| Component | Parameters | Content |
| --- | --- | --- |
| `Column` | `spacing`, `alignment: .leading \| .center \| .trailing` | Children |
| `Row` | `spacing`, `alignment: .top \| .center \| .bottom` | Children |
| `Stack` | `alignment: .topLeading ... .bottomTrailing` | Children, layered |
| `ScrollView` | `axis: .vertical \| .horizontal`, `indicators: Bool` | Children |
| `List`, `ForEach` | `_: List<T>`, `key: .field` | `{ item in ... }` |
| `LazyColumn`, `LazyRow` | `_: List<T>`, `key: .field`, `spacing` | `{ item in ... }` |
| `LazyGrid` | `_: List<T>`, `key: .field`, `columns: Int` or `minWidth`, `spacing` | `{ item in ... }` |
| `Form` | | Children |
| `Section` | `header`, `footer` | Children |
| `GroupBox` | `_` title | Children |
| `DisclosureGroup` | `_` title, `isExpanded: bind(...)` | Children |
| `Text` | `_` text | |
| `Image` | `_: String` asset name | |
| `Label` | `_` title, `icon: String` SF Symbol name | |
| `Button` | `_` title, `role: .destructive \| .cancel`, `action` | |
| `Link` | `_` title, `url: String` | |
| `Menu` | `_` title | Buttons and dividers |
| `TextField`, `SecureField` | `_` label, `text: bind(String)` | |
| `TextEditor` | `text: bind(String)` | |
| `Toggle` | `_` label, `isOn: bind(Bool)` | |
| `Slider` | `value: bind(Double)`, `min`, `max` | |
| `Stepper` | `_` label, `value: bind(Int \| Double)`, `min`, `max`, `step` | |
| `Picker` | `_` label, `selection: bind(...)`, `style: .automatic \| .menu \| .segmented \| .inline \| .wheel \| .palette` | `Option` entries |
| `Option` | `_` label, `value` | |
| `DatePicker` | `_` label, `selection: bind(Date)`, `components: .date \| .time \| .dateTime`, `style: .automatic \| .compact \| .graphical \| .wheel` | |
| `Gauge` | `value`, `min`, `max`, `label` | |
| `Progress` | `_` label, `value`, `total`, `style: .linear \| .circular` | |
| `Spacer`, `Divider` | | |

A `Picker` selection can be `Int`, `String`, `Bool`, an enum or an optional of these (V2046). Its content can only contain `Option` entries, written directly or produced by `ForEach` (V2047), and each `value` must have the selection type. `Option` is only valid inside a `Picker` (V2045).

```
Picker(loc("tag.title", "Tag"), selection: bind(tag)) {
  Option(loc("tag.none", "None"), value: nil)
  ForEach(tags, key: .id) { t in
    Option(t.title, value: t.id)
  }
}
```

Components that a system does not provide, such as `Stepper` on tvOS or `Picker(style: .wheel)` on macOS, are reported for that system (V6031). Wrap them in `#if` to restrict them to the systems that provide them.

### Built-in modifiers

| Modifier | Parameters | Applies to |
| --- | --- | --- |
| `.padding` | Optional amount | Any |
| `.frame` | `width`, `height`, `maxWidth`, `maxHeight`, where the maximums accept `.infinity` | Any |
| `.background` | Color | Any |
| `.font` | `.largeTitle`, `.title`, `.title2`, `.title3`, `.headline`, `.subheadline`, `.body`, `.callout`, `.footnote`, `.caption` | `Text` |
| `.foreground` | Color | `Text` |
| `.disabled` | `Bool` | Controls |
| `.hidden` | `Bool` | Any |
| `.onTap` | Action | Any |
| `.accessibilityLabel` | Text | Any |
| `.refreshable` | Action, `isRefreshing: Bool` | Any |
| `.searchable` | `text: bind(String)`, `prompt` | Any |
| `.keyboard` | `.default`, `.email`, `.number`, `.decimal`, `.phone`, `.url` | `TextField`, `SecureField` |
| `.submitLabel` | `.done`, `.go`, `.next`, `.search`, `.send`, `.return`, `.continue`, `.join`, `.route` | `TextField`, `SecureField` |
| `.onSubmit` | Action | `TextField`, `SecureField` |
| `.autocapitalization` | `.never`, `.words`, `.sentences`, `.characters` | `TextField`, `SecureField` |

Colors are `.primary`, `.secondary`, `.accent`, `.red`, `.orange`, `.yellow`, `.green`, `.blue`, `.purple`, `.gray`, `.white`, `.black` and `.clear`. Using a modifier on a component it does not apply to is an error (V2042).

## Conditional compilation

`#if`, `#elseif`, `#else` and `#endif` select declarations, members, views and modifiers. Conditions combine names with `!`, `&&`, `||` and parentheses.

| Condition | Resolved |
| --- | --- |
| `apple`, `android`, `harmony` | At compile time, by backend |
| Names in `[flags]` | At compile time, overridable with `--flag name=bool` |
| `ios`, `macos`, `watchos`, `tvos`, `visionos` | In Swift as `#if os(...)` when several systems are enabled, false on Android |
| `debug` | In Swift as `#if DEBUG`, in Kotlin as `BuildConfig.DEBUG` |

```
state {
  #if ios
  useFaceID: Bool = false @writable
  #endif
}

view {
  Column {
    #if ios
    Toggle(loc("settings.face_id", "Face ID"), isOn: bind(useFaceID))
    #elseif macos
    Text(loc("settings.touch_id", "Touch ID"))
    #endif
    Text(title)
      #if debug
      .background(.red)
      #endif
  }
}
```

| Position | `apple`, flags | System names | `debug` |
| --- | --- | --- | --- |
| Top-level declarations | Yes | Screens and components | No (V6002) |
| Screen members and fields | Yes | Yes, except `view` and `lifecycle` blocks (V6005) | No (V6002) |
| Views and modifiers | Yes | Yes | Yes |

Modifiers that become component parameters in Compose, such as `.font` and `.keyboard`, cannot depend on `debug` (V6003). Declarations with the same name in mutually exclusive system branches are not supported when several systems are enabled (V1003). Declare one field and branch inside the view instead.

## Platform APIs

Components and modifiers specific to one framework are namespaced and only valid in the matching condition (V6004):

```
#if apple
swiftui.ShareLink(loc("course.share", "Share"), item: url)
Text(title).swiftui.navigationTitle(loc("course.title", "Course"))
#elseif android
compose.FloatingActionButton("+", action: add)
#endif
```

`swiftui.*` covers views and modifiers from the SwiftUI SDK catalog whose parameters map to Veyra types. Overloads are resolved by argument labels and types, and each API is checked against `[swiftui] deployment_targets` (V6030) and per-system availability (V6031). Deprecated APIs produce a warning (V6032). Closure parameters receive view content, `label: { Text("x") }`.

`compose.*` provides `FloatingActionButton` and the modifiers `.compose.imePadding()`, `.compose.systemBarsPadding()` and `.compose.testTag(String)`. The former `ios.*` and `android.*` names report V6020.

A `native` block emits platform code verbatim and bypasses type checking (V6010):

```
#if apple
native swiftui {
  """
  Text("Rows: \({{count}})").monospacedDigit()
  """
}
#endif
```

`{{name}}` is replaced by a state or computed read, an action entry point or a component callback. `[lint] native` in `veyra.toml` sets the warning to `allow`, `warn` or `deny`.

## Localization

```
Text(loc("schedule.title", "My Schedule"))
Text(loc("schedule.greeting", "Hello, {name}", name: user.name))
Text(plural("cart.items", count, one: "{count} item", other: "{count} items"))
Text(format(price, .currency("USD")))
Text(format(startsAt, .dateTime(.short)))
```

Keys consist of lowercase letters, digits and `_`, separated by `.` (V4002). The source text is written in the language set by `[l10n] source`, and every `{placeholder}` must be supplied as a labeled argument. `loc` and `plural` are only valid in views (V4005), so state keeps raw data and the view composes text.

`plural` accepts the categories `zero`, `one`, `two`, `few`, `many` and `other`. `other` is required, and the texts can only use the `{count}` placeholder.

| Format style | Value type |
| --- | --- |
| `.number` | `Int`, `Double` |
| `.percent` | `Double` |
| `.currency("USD")` | `Int`, `Double`, with an ISO 4217 code |
| `.date(length)`, `.time(length)`, `.dateTime(length)` | `Date`, with `.short`, `.medium` (default), `.long` or `.full` |

Plain string literals are not localized and are emitted verbatim.
