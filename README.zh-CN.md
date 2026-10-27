# Veyra

[English](README.md) | 简体中文

Veyra是一门静态类型、强制MVVM架构的UI声明语言。一份`.vey`源码描述界面、状态、绑定、命令与本地化文案，编译器据此生成符合各平台惯例的SwiftUI与Jetpack Compose代码。生成结果只依赖平台SDK，不需要任何Veyra运行时。

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
      Text(loc("schedule.title", "我的课表")).font(.largeTitle)
      Text(loc("schedule.count", "共{count}门课程", count: count))
      TextField(loc("schedule.search", "搜索"), text: bind(query))
      Button(loc("schedule.refresh", "刷新"), action: refresh)
        .disabled(isRefreshing)
      List(items, key: .id) { course in
        Text(course.name).onTap(openCourse(course.id))
      }
    }
  }
}
```

## 安装

Veyra使用Rust编写，需要Rust 1.88或更高版本。

```bash
cargo install --path crates/veyra-cli
```

VS Code的语法高亮扩展位于[editors/vscode](editors/vscode)。

## 快速开始

```bash
veyra init MyApp
cd MyApp
veyra build
```

`veyra init`会创建`veyra.toml`和`ui/`下的示例页面，`veyra build`检查全部源码并把生成的代码写入`veyra.toml`中配置的目录。`examples/schedule`是一个完整的示例工程，包含两个平台手写的ViewModel实现。

## 语言

View中的交互只有三种形式，编译器会在生成任何平台代码之前逐一检查：

| 形式 | 写法 | 规则 |
| --- | --- | --- |
| 读取 | `Text(query)` | 只能是基于state与computed的纯表达式 |
| 双向绑定 | `TextField(..., text: bind(query))` | state必须标记`@writable`，且类型与控件一致 |
| 命令分发 | `Button(..., action: openCourse(course.id))` | 参数按action签名检查 |

View不能调用任意函数、修改状态或访问注入的依赖，有副作用的操作都放在action中，由ViewModel实现。

每个文件只能看到自己的声明以及import的文件中的声明。import的路径相对于当前文件，且不会传递：

```
import "models.vey"
import (
  "stores/NoteStore.vey"
  "../shared/Theme.vey"
)
```

使用未import的文件中的类型会报错，诊断信息会给出需要添加的`import`。不在`sources`中的被import文件会自动加载，未使用的import会给出警告。由于每个平台的生成代码位于同一个模块中，所有声明仍共享同一个命名空间。

声明：

| 声明 | 用途 |
| --- | --- |
| `enum`、`model` | 各层共享的值类型 |
| `repository` | 数据访问契约，由各平台原生实现 |
| `entity`、`store` | 持久化数据及其数据访问对象 |
| `settings`、`secure` | 简单配置与安全凭证 |
| `component` | 不含业务状态的可复用视图，参数可以是值、`Action`与`Binding<T>` |
| `screen` | 页面，包含`inject`、`state`、`computed`、`actions`、`effects`、`lifecycle`与`view` |

`effects`声明导航等一次性事件，由ViewModel通过`emit`发出，组装层通过`onEffect`接收，View本身不处理。`lifecycle`在页面出现或消失时分发action。

持久化数据只需声明一次，即可同时生成SwiftData与Room代码：

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

每个store自动提供`all`、`find`、`save`与`delete`，以及声明的查询。查询条件支持比较运算、`contains`、与`nil`比较、Bool字段、`&&`、`||`与`!`，在iOS上编译为`#Predicate`，在Android上编译为SQL。返回`Stream<List<T>>`时持续观察变化，返回`List<T>`或`T?`时只读取一次。View与ViewModel只接触值类型`Note`，不会拿到持久化类型`NoteEntity`。iOS上`VeyraStorage`与`VeyraStores`负责创建SwiftData容器，Android上对应`VeyraDatabase`与`VeyraStores`。Android的表结构变化时，递增`[compose]`中的`database_version`，并在生成的`VeyraMigrations.kt`中添加迁移。

`settings`生成基于`UserDefaults`与DataStore的类型安全配置，每个字段都需要默认值。`secure`生成凭证存储，iOS使用Keychain，Android使用Android Keystore密钥加密后保存，不依赖第三方库。两者与repository、store一样注入到ViewModel中。

条件编译用于选择平台专属内容。`apple`、`android`与`[flags]`中声明的自定义flag在编译期确定。SwiftUI后端为`[swiftui] platforms`中的所有系统生成同一套Swift代码，因此`ios`、`macos`、`watchos`、`tvos`与`visionos`会保留为生成代码中的`#if os(...)`，并对每个系统分别做类型检查：

```toml
[swiftui]
platforms = ["ios", "macos", "visionos"]
```

```
state {
  #if ios
  useFaceID: Bool = false @writable
  #endif
}
```

系统条件可以用于screen、component及其成员、视图与修饰符。`debug`在Apple系统上保留为`#if DEBUG`，在Android上为`BuildConfig.DEBUG`，因此只能用于视图与修饰符，Android工程需要开启`buildFeatures.buildConfig = true`。

框架专属的组件与修饰符位于`swiftui.*`与`compose.*`下，例如`swiftui.ShareLink`、`.swiftui.glassEffect()`、`compose.FloatingActionButton`与`.compose.imePadding()`，需要放在`#if apple`或`#if android`中使用。如果仍不够用，可以用`native swiftui { """...""" }`原样输出代码，其中`{{name}}`会替换为状态读取或action入口。native代码块不受类型检查，编译时会给出警告，可通过`[lint] native`关闭或升级为错误。

```
#if apple
Text(title).swiftui.glassEffect()
#elseif android
compose.FloatingActionButton("+", action: add)
#endif
#if debug
Text(buildInfo)
#endif
```

本地化文案通过`loc(key, 源文案, 参数...)`显式声明。翻译存放在`locales/<locale>.toml`中，由`veyra build`自动同步：新增的key写入空占位，已有翻译原样保留，翻译中的占位符与源文案逐一核对。普通字符串字面量原样输出，不参与本地化。

```
Text(plural("cart.items", count, other: "共{count}件"))
Text(format(price, .currency("CNY")))
Text(format(startsAt, .dateTime(.short)))
```

`plural`生成String Catalog的复数变体与Android的`<plurals>`资源，各语言的翻译按需提供对应的复数类别。`format`支持`.number`、`.percent`、`.currency(code)`、`.date`、`.time`与`.dateTime`，通过平台格式化器跟随用户的区域设置。

## 生成的代码

| | SwiftUI（iOS 17+） | Jetpack Compose |
| --- | --- | --- |
| 状态 | `@Observable`类 | 放在`StateFlow`中的不可变`data class` |
| 契约 | `XxxViewModelContract`协议 | `XxxActions`接口与`XxxViewModelBase` |
| 视图 | 只依赖契约的泛型View | 无状态的`XxxContent` |
| 入口 | `XxxRoot`，依赖来自`Environment` | `XxxRoute`，依赖来自`CompositionLocal` |
| 文案 | `L10n`访问器与String Catalog | `res/values*/veyra_strings.xml` |

以`.generated.swift`或`.generated.kt`结尾的文件每次构建都会重新生成。ViewModel骨架只在首次构建时创建，之后不会被覆盖，业务逻辑始终写在开发者自己的文件中。绑定的写入一律经过生成的setter，ViewModel可以重写setter来校验输入。

通过`inject`声明的依赖只会进入ViewModel。默认由组装层从`VeyraDependencies`读取，应用通过Environment或`CompositionLocal`提供。在`[swiftui]`中设置`di = "factory"`可改用FactoryKit的`@Injected`，在`[compose]`中设置`di = "hilt"`可生成`@HiltViewModel`。

## 命令

| 命令 | 说明 |
| --- | --- |
| `veyra init [path]` | 创建项目 |
| `veyra check` | 检查源码，不写入文件 |
| `veyra build` | 检查源码并生成代码 |
| `veyra fmt [--check] [files]` | 格式化源码，保留注释 |
| `veyra l10n status` | 显示翻译进度 |
| `veyra l10n prune` | 删除源码中已不存在的key |

`check`与`build`支持`--backend <name>`指定平台，`--flag <name>[=bool]`覆盖flag取值。

## 开发

```bash
cargo test --workspace
scripts/check-swift.sh
```

`scripts/check-swift.sh`会生成示例工程，并在Swift 6语言模式下用iOS模拟器SDK检查Swift输出的类型，需要Xcode 16或更高版本。

## 许可证

Veyra采用GNU Affero通用公共许可证第3版并附加Commons Clause条件。修改后的版本必须以相同条款开源，未经另行商业授权，不得出售Veyra或以其为主要价值的服务。使用Veyra从你自己的源码生成的代码不受该许可证约束，可用于任何用途。详见[LICENSE](LICENSE)。
