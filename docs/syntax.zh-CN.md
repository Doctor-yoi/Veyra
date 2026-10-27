# Veyra语法参考

[English](syntax.md) | 简体中文

本文档描述当前编译器接受的`.vey`语言。规则由编译器检查时会注明对应的诊断代码（例如`V2001`），便于从错误信息找到产生它的规则。

## 词法

源文件使用UTF-8编码。换行没有语法含义，语句之间不需要分隔符。

```
// 行注释
/* 块注释，/* 可以嵌套 */ */
```

标识符以字母或`_`开头，后接字母、数字或`_`。`screen`、`state`、`view`等关键字是上下文相关的：只在需要声明或代码块的位置作为关键字，因此仍可用作字段名。

| 字面量 | 示例 | 说明 |
| --- | --- | --- |
| 整数 | `0`、`42`、`-1` | 在需要`Double`的位置会转为`Double` |
| 浮点数 | `0.5`、`1.25` | |
| 字符串 | `"Hello"`、`"a\nb"` | 转义：`\n` `\t` `\r` `\"` `\\` `\{` `\}` |
| 多行字符串 | `"""` ... `"""` | 只用于`native`代码块，会去掉公共缩进 |
| 布尔值 | `true`、`false` | |
| 空值 | `nil` | 只能用于可选类型 |
| 列表 | `[]`、`[1, 2, 3]` | |

本地化文案中的`{name}`是占位符，字面的花括号写作`\{`与`\}`。

## 文件与import

文件由可选的import与之后的声明组成。每个文件只能看到自身的声明，以及它直接import的文件中的声明，import不会传递。

```
import "models.vey"
import (
  "stores/NoteStore.vey"
  "../shared/Theme.vey"
)
```

路径相对于当前文件。使用未import文件中的声明会报错（V1034），未使用的import会给出警告。所有声明共用一个命名空间，因此两个文件不能声明同名的类型（V1001）。

## 类型

| 类型 | 说明 |
| --- | --- |
| `Bool`、`Int`、`Double`、`String` | 基本类型 |
| `Date` | 时间点，Swift中为`Date`，Kotlin中为`java.time.Instant` |
| `List<T>` | 有序列表 |
| `T?` | 可选类型，不能嵌套（V2003） |
| `Stream<T>` | 可观察的值，只能作为`repository`与`store`方法的返回类型（V2005） |
| 枚举或模型名称 | 已声明的`enum`、`model`或`entity` |

平台SDK占用的名称（例如`View`、`Task`、`Context`）不能用于声明（V1002）。

## 声明

### enum

```
enum CourseKind { lecture, lab, seminar }
```

类型可由上下文推断时写作`.lecture`，否则写作`CourseKind.lecture`。

### model

model是各层共用的不可变值类型。字段不能有默认值（V2041），也不能直接包含自身（V2012）。

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

repository是数据访问契约，由各平台原生实现，并注入到ViewModel中。

```
repository CourseRepository {
  getCourses() async throws -> List<Course>
  observeCourses() -> Stream<List<Course>>
}
```

返回`Stream`的方法不能是`async`（V2050）。

### entity与store

entity是持久化记录，生成SwiftData与Room代码。它需要且只能有一个类型为`String`或`Int`的`@primaryKey`字段（V5002、V5003）。字段还可以标记`@indexed`或`@unique`，不能有默认值。

```
entity Note @persistent {
  id: String @primaryKey
  courseId: String @indexed
  text: String
  createdAt: Date
}
```

store是某个entity的数据访问对象。每个store都提供`all`、`find`、`save`与`delete`，另外可以声明查询：

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

| 子句 | 说明 |
| --- | --- |
| `where` | 对entity字段与查询参数做比较，支持`contains`、`== nil`、Bool字段、`&&`、`||`与`!` |
| `orderBy` | 一个或多个以逗号分隔的字段，每个字段后可跟`asc`或`desc` |
| `limit` | 结果数量上限 |

返回类型决定查询方式：`Stream<List<T>>`持续观察变化，`List<T>`读取一次，`T?`读取第一条匹配的记录。View与ViewModel只能看到值类型`Note`，看不到持久化类型`NoteEntity`。

### settings与secure

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

`settings`字段可以是`Bool`、`Int`、`Double`、`String`、`Date`、枚举及其可选类型（V5020）。非可选字段都需要默认值（V2010），`Date`字段必须是可选类型（V5021）。`secure`字段可以是`String`、`Int`、`Double`或`Bool`，读取结果总是可选值，不能有默认值。两者都像repository一样注入到ViewModel中。

### component

component是不含业务状态的可复用视图，主体是一棵视图树。

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

| 参数类型 | 说明 |
| --- | --- |
| 值类型 | 只读输入 |
| `Action` | 回调，传入action引用，例如`openCourse(course.id)` |
| `Binding<T>` | 双向绑定，传入`bind(field)`或其他Binding参数 |

实参必须带名称（V2073）。可选的值参数可以省略，默认为`nil`。

```
CourseRow(course: course, note: nil, onTap: openCourse(course.id))
```

### screen

screen是拥有独立ViewModel的页面，成员可以按任意顺序书写。

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

| 成员 | 规则 |
| --- | --- |
| `inject name: Type` | repository、store、`settings`或`secure`类型。只有ViewModel能拿到，View无法访问 |
| `state` | 默认值必须是常量：字面量、列表字面量、枚举成员、`nil`与`now()`（V2011）。可选字段默认为`nil`。`@writable`允许绑定并生成setter |
| `computed` | 基于state与其他computed的纯表达式，不能循环依赖（V3007） |
| `actions` | 由ViewModel实现的命令。可以是`async`，不能有返回值（V3018）或`throws`（V3019） |
| `effects` | 导航等一次性事件，由ViewModel通过`emit`发出，由根视图或路由通过`onEffect`处理。不能有`async`、`throws`或返回值，View不能触发（V3033） |
| `lifecycle` | `onAppear`与`onDisappear`，各自分发一个action |
| `view` | 有且只有一个非空的view块（V3020、V3021） |

可写字段`query`会生成名为`setQuery`的setter，setter不能与action重名。

## 表达式

view与computed中的表达式都是纯表达式。它们读取state、computed、组件参数与闭包参数，不能调用action或函数。

| 优先级 | 运算符 | 说明 |
| --- | --- | --- |
| 1 | `\|\|` | 逻辑或 |
| 2 | `&&` | 逻辑与 |
| 3 | `==` `!=` `<` `<=` `>` `>=` | 比较 |
| 4 | `??` | 可选值的默认值，`teacher ?? "-"` |
| 5 | `+` `-` | 加减，`String`拼接 |
| 6 | `*` `/` `%` | 乘、除、取余 |
| 前缀 | `!` `-` | 取反、取负 |

`cond ? a : b`按条件选择值，结合得比所有二元运算符都松。括号用于分组。

| 成员 | 适用于 | 结果 |
| --- | --- | --- |
| `.count` | `List<T>`、`String` | `Int` |
| `.isEmpty` | `List<T>`、`String` | `Bool` |
| `.first` | `List<T>` | `T?` |
| `.contains(x)` | `String` | `Bool` |
| `.field` | model | 字段的值 |

| 函数 | 说明 |
| --- | --- |
| `loc(key, text, name: value...)` | 本地化文案，只能用于view |
| `plural(key, count, one: ..., other: ...)` | 本地化复数文案，只能用于view |
| `format(value, style)` | 按区域设置格式化为`String` |
| `now()` | 当前时间，也可作为state的默认值 |
| `bind(field)` | 绑定到`@writable`的state，只能作为绑定参数 |

## 视图

视图是一棵组件树。每个组件写成一次调用，可带参数、尾随闭包与修饰符链：

```
Column(spacing: 12, alignment: .leading) {
  Text(title).font(.headline)
  Button(loc("home.refresh", "Refresh"), action: refresh)
    .disabled(isRefreshing)
}
.padding(16)
```

实参按名称匹配，不带名称的实参按顺序填入位置参数。缺少必填参数（V2032）、未知的参数名（V2030）与多余的位置参数（V2035）都会报错。`.leading`、`.infinity`等隐式成员用于选择固定取值。

集合把每个元素作为参数传给闭包：

```
List(items, key: .id) { course in
  CourseRow(course: course, note: nil, onTap: openCourse(course.id))
}
```

`key`指定元素model中用于标识元素的字段（V2037）。

### 读取、绑定与分发

| 形式 | 示例 | 规则 |
| --- | --- | --- |
| 读取 | `Text(query)` | 任何类型匹配的纯表达式 |
| 绑定 | `TextField(..., text: bind(query))` | state必须标记`@writable`（V3001），类型必须与控件一致 |
| 分发 | `Button(..., action: openCourse(course.id))` | action名称，实参按其参数检查 |

没有参数的action可以直接写名称，例如`action: refresh`。实参按参数顺序传入，也可以带名称，例如`openCourse(id: course.id)`。async action会自动在Task中执行。组件的`Action`参数直接按名称转发，例如`.onTap(onTap)`。

### 条件

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

条件是`Bool`表达式。`if available(...)`在Apple平台上于运行时检查系统版本，Android总是使用第一个分支：

```
if available(ios: 26, macos: 26) {
  Text(title).swiftui.glassEffect(.regular, in: .capsule)
} else {
  Text(title)
}
```

### 内置组件

内置组件在所有平台上可用。标记为`_`的参数是位置参数。

| 组件 | 参数 | 内容 |
| --- | --- | --- |
| `Column` | `spacing`、`alignment: .leading \| .center \| .trailing` | 子组件 |
| `Row` | `spacing`、`alignment: .top \| .center \| .bottom` | 子组件 |
| `Stack` | `alignment: .topLeading ... .bottomTrailing` | 子组件，层叠排列 |
| `ScrollView` | `axis: .vertical \| .horizontal`、`indicators: Bool` | 子组件 |
| `List`、`ForEach` | `_: List<T>`、`key: .field` | `{ item in ... }` |
| `LazyColumn`、`LazyRow` | `_: List<T>`、`key: .field`、`spacing` | `{ item in ... }` |
| `LazyGrid` | `_: List<T>`、`key: .field`、`columns: Int`或`minWidth`、`spacing` | `{ item in ... }` |
| `Form` | | 子组件 |
| `Section` | `header`、`footer` | 子组件 |
| `GroupBox` | `_`标题 | 子组件 |
| `DisclosureGroup` | `_`标题、`isExpanded: bind(...)` | 子组件 |
| `Text` | `_`文本 | |
| `Image` | `_: String`资源名称 | |
| `Label` | `_`标题、`icon: String` SF Symbol名称 | |
| `Button` | `_`标题、`role: .destructive \| .cancel`、`action` | |
| `Link` | `_`标题、`url: String` | |
| `Menu` | `_`标题 | 按钮与分隔线 |
| `TextField`、`SecureField` | `_`标签、`text: bind(String)` | |
| `TextEditor` | `text: bind(String)` | |
| `Toggle` | `_`标签、`isOn: bind(Bool)` | |
| `Slider` | `value: bind(Double)`、`min`、`max` | |
| `Stepper` | `_`标签、`value: bind(Int \| Double)`、`min`、`max`、`step` | |
| `Picker` | `_`标签、`selection: bind(...)`、`style: .automatic \| .menu \| .segmented \| .inline \| .wheel \| .palette` | `Option`选项 |
| `Option` | `_`标签、`value` | |
| `DatePicker` | `_`标签、`selection: bind(Date)`、`components: .date \| .time \| .dateTime`、`style: .automatic \| .compact \| .graphical \| .wheel` | |
| `Gauge` | `value`、`min`、`max`、`label` | |
| `Progress` | `_`标签、`value`、`total`、`style: .linear \| .circular` | |
| `Spacer`、`Divider` | | |

`Picker`的selection可以是`Int`、`String`、`Bool`、枚举及其可选类型（V2046）。内容只能是`Option`，可以直接书写，也可以由`ForEach`生成（V2047），每个`value`都必须是selection的类型。`Option`只能用于`Picker`中（V2045）。

```
Picker(loc("tag.title", "Tag"), selection: bind(tag)) {
  Option(loc("tag.none", "None"), value: nil)
  ForEach(tags, key: .id) { t in
    Option(t.title, value: t.id)
  }
}
```

某个系统不提供的组件（例如tvOS上的`Stepper`、macOS上的`Picker(style: .wheel)`）会按系统报错（V6031），用`#if`限定到提供它的系统即可。

### 内置修饰符

| 修饰符 | 参数 | 适用于 |
| --- | --- | --- |
| `.padding` | 可选的间距 | 任意组件 |
| `.frame` | `width`、`height`、`maxWidth`、`maxHeight`，最大值可以是`.infinity` | 任意组件 |
| `.background` | 颜色 | 任意组件 |
| `.font` | `.largeTitle`、`.title`、`.title2`、`.title3`、`.headline`、`.subheadline`、`.body`、`.callout`、`.footnote`、`.caption` | `Text` |
| `.foreground` | 颜色 | `Text` |
| `.disabled` | `Bool` | 控件 |
| `.hidden` | `Bool` | 任意组件 |
| `.onTap` | action | 任意组件 |
| `.accessibilityLabel` | 文本 | 任意组件 |
| `.refreshable` | action、`isRefreshing: Bool` | 任意组件 |
| `.searchable` | `text: bind(String)`、`prompt` | 任意组件 |
| `.keyboard` | `.default`、`.email`、`.number`、`.decimal`、`.phone`、`.url` | `TextField`、`SecureField` |
| `.submitLabel` | `.done`、`.go`、`.next`、`.search`、`.send`、`.return`、`.continue`、`.join`、`.route` | `TextField`、`SecureField` |
| `.onSubmit` | action | `TextField`、`SecureField` |
| `.autocapitalization` | `.never`、`.words`、`.sentences`、`.characters` | `TextField`、`SecureField` |

颜色可选`.primary`、`.secondary`、`.accent`、`.red`、`.orange`、`.yellow`、`.green`、`.blue`、`.purple`、`.gray`、`.white`、`.black`与`.clear`。把修饰符用于不适用的组件会报错（V2042）。

## 条件编译

`#if`、`#elseif`、`#else`与`#endif`用于选择声明、成员、视图与修饰符。条件由名称与`!`、`&&`、`||`、括号组合而成。

| 条件 | 确定方式 |
| --- | --- |
| `apple`、`android`、`harmony` | 编译期按后端确定 |
| `[flags]`中的名称 | 编译期确定，可用`--flag name=bool`覆盖 |
| `ios`、`macos`、`watchos`、`tvos`、`visionos` | 启用多个系统时在Swift中保留为`#if os(...)`，Android上为假 |
| `debug` | Swift中为`#if DEBUG`，Kotlin中为`BuildConfig.DEBUG` |

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

| 位置 | `apple`与flag | 系统名称 | `debug` |
| --- | --- | --- | --- |
| 顶层声明 | 可以 | 仅screen与component | 不可以（V6002） |
| screen成员与字段 | 可以 | 可以，`view`与`lifecycle`块除外（V6005） | 不可以（V6002） |
| 视图与修饰符 | 可以 | 可以 | 可以 |

`.font`、`.keyboard`等在Compose中生成为组件参数的修饰符不能依赖`debug`（V6003）。启用多个系统时，不支持在互斥的系统分支中声明同名成员（V1003），请声明一个字段，并在view中按系统区分。

## 平台API

某个框架专属的组件与修饰符带有命名空间，只能用于对应的条件中（V6004）：

```
#if apple
swiftui.ShareLink(loc("course.share", "Share"), item: url)
Text(title).swiftui.navigationTitle(loc("course.title", "Course"))
#elseif android
compose.FloatingActionButton("+", action: add)
#endif
```

`swiftui.*`覆盖SwiftUI SDK目录中参数能映射到Veyra类型的视图与修饰符。重载按参数名称与类型解析，每个API都会按`[swiftui] deployment_targets`（V6030）与各系统的可用性（V6031）检查，已废弃的API给出警告（V6032）。闭包参数接收视图内容，例如`label: { Text("x") }`。

`compose.*`提供`FloatingActionButton`，以及修饰符`.compose.imePadding()`、`.compose.systemBarsPadding()`与`.compose.testTag(String)`。旧的`ios.*`与`android.*`写法会报V6020。

`native`代码块原样输出平台代码，不经过类型检查（V6010）：

```
#if apple
native swiftui {
  """
  Text("Rows: \({{count}})").monospacedDigit()
  """
}
#endif
```

`{{name}}`会替换为state或computed的读取、action的分发入口或组件的回调。`veyra.toml`中的`[lint] native`可把该警告设为`allow`、`warn`或`deny`。

## 本地化

```
Text(loc("schedule.title", "My Schedule"))
Text(loc("schedule.greeting", "Hello, {name}", name: user.name))
Text(plural("cart.items", count, one: "{count} item", other: "{count} items"))
Text(format(price, .currency("USD")))
Text(format(startsAt, .dateTime(.short)))
```

key由小写字母、数字与`_`组成，用`.`分段（V4002）。源文案使用`[l10n] source`指定的语言书写，每个`{占位符}`都必须以带名称的实参提供。`loc`与`plural`只能用于view（V4005），state保存原始数据，由View组装文案。

`plural`可用的类别为`zero`、`one`、`two`、`few`、`many`与`other`。`other`必填，文案只能使用`{count}`占位符。

| 格式样式 | 值类型 |
| --- | --- |
| `.number` | `Int`、`Double` |
| `.percent` | `Double` |
| `.currency("USD")` | `Int`、`Double`，需要ISO 4217货币代码 |
| `.date(length)`、`.time(length)`、`.dateTime(length)` | `Date`，长度为`.short`、`.medium`（默认）、`.long`或`.full` |

普通字符串字面量不做本地化，原样输出。
