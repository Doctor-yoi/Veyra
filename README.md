# Veyra

English | [简体中文](README.zh-CN.md)

Veyra is a statically typed UI language that enforces MVVM. A single `.vey` source describes views, state, bindings, commands and localized text, and the compiler generates idiomatic SwiftUI and Jetpack Compose code. The generated code depends only on the platform SDKs and needs no Veyra runtime.

```
screen Schedule {
  inject courses: CourseRepository

  state {
    query: String = "" @writable
    items: List<Course> = []
    isRefreshing: Bool = false
  }

  computed {
    count: Int = items.count
  }

  actions {
    refresh() async
    openCourse(id: String)
  }

  view {
    Column(spacing: 12, alignment: .leading) {
      Text(loc("schedule.title", "My Schedule")).font(.largeTitle)
      Text(loc("schedule.count", "{count} courses", count: count))
      TextField(loc("schedule.search", "Search"), text: bind(query))
      Button(loc("schedule.refresh", "Refresh"), action: refresh)
        .disabled(isRefreshing)
      List(items, key: .id) { course in
        Text(course.name).onTap(openCourse(course.id))
      }
    }
  }
}
```

## Installation

Veyra is written in Rust and requires Rust 1.88 or later.

```bash
cargo install --path crates/veyra-cli
```

## Quick start

```bash
veyra init MyApp
cd MyApp
veyra build
```

`veyra init` creates `veyra.toml` and a sample screen in `ui/`. `veyra build` checks every source file and writes the generated code to the directories configured in `veyra.toml`. The `examples/schedule` directory contains a complete project with hand-written view models for both platforms.

## Language

Every interaction in a view falls into one of three forms, and the compiler checks each before any platform code is generated:

| Form | Syntax | Rule |
| --- | --- | --- |
| Read | `Text(query)` | Pure expressions over state and computed values |
| Two-way binding | `TextField(..., text: bind(query))` | The state must be marked `@writable`, and the type must match the control |
| Command | `Button(..., action: openCourse(course.id))` | Arguments are checked against the action signature |

Views cannot call arbitrary functions, mutate state or access injected dependencies. Side effects belong in actions, which the view model implements.

Each file sees only its own declarations and those of the files it imports. Import paths are relative to the current file, and imports are not transitive:

```
import "models.vey"
import (
  "stores/NoteStore.vey"
  "../shared/Theme.vey"
)
```

Using a type from a file that is not imported is an error, and the message suggests the missing `import`. Imported files outside `sources` are loaded automatically, and unused imports produce a warning. All declarations still share one namespace, because the generated code lives in one module per platform.

Declarations:

| Declaration | Purpose |
| --- | --- |
| `enum`, `model` | Value types shared by all layers |
| `repository` | Data access contract, implemented natively on each platform |
| `entity`, `store` | Persistent data and its data access object |
| `settings`, `secure` | Simple preferences and credentials |
| `component` | Reusable view without business state, taking values, `Action` and `Binding<T>` parameters |
| `screen` | A page with `inject`, `state`, `computed`, `actions`, `effects`, `lifecycle` and `view` |

`effects` declares one-off events such as navigation. The view model emits them with `emit`, and the root view or route receives them through `onEffect`, so views never handle them. `lifecycle` dispatches actions when a screen appears or disappears.

Persistent data is declared once and generated for SwiftData and Room:

```
entity Note @persistent {
  id: String @primaryKey
  courseId: String @indexed
  text: String
  createdAt: Date
}

store NoteStore for Note {
  query forCourse(course: String) -> Stream<List<Note>> {
    where courseId == course
    orderBy createdAt desc
  }
}
```

Every store provides `all`, `find`, `save` and `delete`, plus the declared queries. Conditions support comparisons, `contains`, `nil` checks, Bool fields, `&&`, `||` and `!`, and compile to `#Predicate` on iOS and SQL on Android. Returning `Stream<List<T>>` observes changes, while `List<T>` and `T?` read once. Views and view models only see the value type `Note`, never the persistent `NoteEntity`. `VeyraStorage` and `VeyraStores` set up the SwiftData container, while `VeyraDatabase` and `VeyraStores` set up Room. On Android, increase `database_version` under `[compose]` and add migrations to the generated `VeyraMigrations.kt` when the schema changes.

`settings` generates typed preferences backed by `UserDefaults` and DataStore, and every field needs a default value. `secure` generates credential storage backed by the Keychain on iOS and by values encrypted with an Android Keystore key on Android, without third-party libraries. Both are injected into view models like repositories and stores.

Conditional compilation selects platform-specific content. Platform conditions and custom flags declared in `[flags]` are resolved at compile time, and each platform is type checked separately:

```
state {
  #if ios
  useFaceID: Bool = false @writable
  #endif
}
```

`debug` is kept in the native code as `#if DEBUG` on iOS and `BuildConfig.DEBUG` on Android, so it can only guard views and modifiers. Android projects need `buildFeatures.buildConfig = true`. Inside a platform block, platform-only components and modifiers are available, such as `ios.ShareLink`, `.ios.glassEffect()`, `android.FloatingActionButton` and `.android.imePadding()`. As a last resort, `native ios { """...""" }` emits code verbatim, with `{{name}}` replaced by state reads or action entry points. Native blocks bypass type checking, so they produce a warning that `[lint] native` can turn off or into an error.

```
#if ios
Text(title).ios.glassEffect()
#elseif android
android.FloatingActionButton("+", action: add)
#endif
#if debug
Text(buildInfo)
#endif
```

Localized text is declared explicitly with `loc(key, sourceText, args...)`. Translations live in `locales/<locale>.toml`, which `veyra build` keeps in sync: new keys are added as empty placeholders, existing translations are preserved, and placeholders are validated against the source text. Plain string literals are emitted verbatim.

```
Text(plural("cart.items", count, one: "{count} item", other: "{count} items"))
Text(format(price, .currency("USD")))
Text(format(startsAt, .dateTime(.short)))
```

`plural` generates String Catalog plural variations and Android `<plurals>` resources, and each translation provides the plural categories its language needs. `format` supports `.number`, `.percent`, `.currency(code)`, `.date`, `.time` and `.dateTime`, and follows the user's locale through the platform formatters.

## Generated code

| | SwiftUI (iOS 17+) | Jetpack Compose |
| --- | --- | --- |
| State | `@Observable` class | Immutable `data class` in a `StateFlow` |
| Contract | `XxxViewModelContract` protocol | `XxxActions` interface and `XxxViewModelBase` |
| View | Generic view over the contract | Stateless `XxxContent` composable |
| Entry | `XxxRoot` with dependencies from `Environment` | `XxxRoute` with dependencies from a `CompositionLocal` |
| Strings | `L10n` accessors and a String Catalog | `res/values*/veyra_strings.xml` |

Files ending in `.generated.swift` or `.generated.kt` are rewritten on every build. View model skeletons are created once and never overwritten, so business logic stays in files you own. Writes from bindings always go through generated setters, which view models may override to validate input.

Dependencies declared with `inject` only ever reach view models. By default the root view or route reads them from `VeyraDependencies`, which the app provides through the environment or a `CompositionLocal`. Set `di = "factory"` under `[swiftui]` to resolve them with FactoryKit's `@Injected`, or `di = "hilt"` under `[compose]` to generate `@HiltViewModel` view models.

## Commands

| Command | Description |
| --- | --- |
| `veyra init [path]` | Create a project |
| `veyra check` | Check sources without writing files |
| `veyra build` | Check sources and generate code |
| `veyra l10n status` | Show translation progress |
| `veyra l10n prune` | Remove translations whose keys no longer exist |

`check` and `build` accept `--backend <name>` to limit the platforms and `--flag <name>[=bool]` to override flags.

## Development

```bash
cargo test --workspace
scripts/check-swift.sh
```

`scripts/check-swift.sh` builds the example project and typechecks the Swift output against the iOS simulator SDK in Swift 6 language mode. It requires Xcode 16 or later.

## License

Veyra is licensed under the GNU Affero General Public License v3.0 with the Commons Clause condition. Modified versions must be released under the same terms, and selling Veyra or services whose value derives substantially from it is not permitted without a separate commercial license. Code generated by Veyra from your own sources is not covered by this license and may be used for any purpose. See [LICENSE](LICENSE) for details.
