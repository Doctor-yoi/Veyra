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

声明：

| 声明 | 用途 |
| --- | --- |
| `enum`、`model` | 各层共享的值类型 |
| `repository` | 数据访问契约，由各平台原生实现 |
| `screen` | 页面，包含`inject`、`state`、`computed`、`actions`与`view` |

条件编译用于选择平台专属内容。平台条件与`[flags]`中声明的自定义flag在编译期确定，每个平台单独做类型检查：

```
state {
  #if ios
  useFaceID: Bool = false @writable
  #endif
}
```

本地化文案通过`loc(key, 源文案, 参数...)`显式声明。翻译存放在`locales/<locale>.toml`中，由`veyra build`自动同步：新增的key写入空占位，已有翻译原样保留，翻译中的占位符与源文案逐一核对。普通字符串字面量原样输出，不参与本地化。

## 生成的代码

| | SwiftUI（iOS 17+） | Jetpack Compose |
| --- | --- | --- |
| 状态 | `@Observable`类 | 放在`StateFlow`中的不可变`data class` |
| 契约 | `XxxViewModelContract`协议 | `XxxActions`接口与`XxxViewModelBase` |
| 视图 | 只依赖契约的泛型View | 无状态的`XxxContent` |
| 入口 | `XxxRoot`，依赖来自`Environment` | `XxxRoute`，依赖来自`CompositionLocal` |
| 文案 | `L10n`访问器与String Catalog | `res/values*/veyra_strings.xml` |

以`.generated.swift`或`.generated.kt`结尾的文件每次构建都会重新生成。ViewModel骨架只在首次构建时创建，之后不会被覆盖，业务逻辑始终写在开发者自己的文件中。绑定的写入一律经过生成的setter，ViewModel可以重写setter来校验输入。

## 命令

| 命令 | 说明 |
| --- | --- |
| `veyra init [path]` | 创建项目 |
| `veyra check` | 检查源码，不写入文件 |
| `veyra build` | 检查源码并生成代码 |
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
