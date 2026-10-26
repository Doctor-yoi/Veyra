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

Declarations:

| Declaration | Purpose |
| --- | --- |
| `enum`, `model` | Value types shared by all layers |
| `repository` | Data access contract, implemented natively on each platform |
| `component` | Reusable view without business state, taking values, `Action` and `Binding<T>` parameters |
| `screen` | A page with `inject`, `state`, `computed`, `actions`, `effects`, `lifecycle` and `view` |

`effects` declares one-off events such as navigation. The view model emits them with `emit`, and the root view or route receives them through `onEffect`, so views never handle them. `lifecycle` dispatches actions when a screen appears or disappears.

Conditional compilation selects platform-specific content. Platform conditions and custom flags declared in `[flags]` are resolved at compile time, and each platform is type checked separately:

```
state {
  #if ios
  useFaceID: Bool = false @writable
  #endif
}
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
