# SDK `@dopame/acde-api` (`acde-api/`)

> 路径相对仓库根。

`@dopame/acde-api` 是运行时删掉的 `web/` 的替代品 (群里结论, 2026-11-17): 学 workerd, npm 包是薄壳, **只有 `.d.ts` 声明, 没有 JS 逻辑**.
它描述运行时给应用的东西: `acde:*` 模块 (运行时注入页面, 包里只是 `declare module`)、包清单、`acde.config.jsonc`、服务、能力、页面对象和事件的类型.
来源是删除前的 `web/packages/acde-api` (acdert `91e7dd1e` 的父提交, 本仓库的历史), 只取纯类型声明. 它和清单规则 `manifest/` 原在 acdev 的
`npm/` 里, 2026-11-18 随运行时库一起拆进本仓库: 运行时编译时要读它们, acdes 不该为此检出 acdev.

## 怎么引用

- 仓内: `js/tsconfig.json` 的 paths 把 `@dopame/acde-api` 指到 `acde-api/src/index.d.ts`; `manifest/` 用相对路径 `import type`.
- acdev 的命令行 (并排检出): `"@dopame/acde-api": "file:../../libacde/acde-api"` (acdev 的 `npm/package.json`), 用 `import type { … } from '@dopame/acde-api'`.
- 别的仓库或工程在本机: `"@dopame/acde-api": "file:<libacde 检出>/acde-api"`.
- 入口 (`package.json` 的 `exports`, 只有 `types` 条件, 没有 `default`): `.` (全部类型和全部 `acde:*` 模块)、`./app` (应用能导入的模块)、
  `./shell` (桌面和它自己的应用另外能导入的外壳模块). 页面里用 `/// <reference types="@dopame/acde-api/app" />` 拿到 `acde:*` 模块的类型.
  只能 `import type`: 包里没有 JS, 运行时 `import '@dopame/acde-api'` 找不到入口.
- 一个工程只用一个入口: 应用的类型从 `./app` 取, 桌面从 `./shell` 取; 和 `.` 混用时 `acde:*` 模块的声明重复, 类型检查报冲突 (原包就是这样).
- 还没发到 npm (发版要用户同意); 包现在 `"private": true`.

## 版本与兼容 (用户 2026-11-20 定)

- 版本号跟 `acde-api/package.json` 的 `version`; 小版本 (含补丁) 只增不删: 新增类型、方法、事件、字段可以, 不删、不改名、不收窄已有的.
- 要替换的接口先标过时并保留可用: 运行时照旧实现, 旧事件照旧发出 (可以由新事件推出来). 标法两处都要:
  - 声明文件里, 在它上方写一行 `/** @deprecated 改用 <替代者> */`, 让编辑器划线. 这是代码不写注释的唯一例外 (用户 2026-11-20 定), 只限 `acde-api/` 的 `.d.ts`,
    只写这一行, 总仓质量检查只放行这种单行写法;
  - 登记在下面的「已过时」清单: 替代者、从哪个版本起过时.
- 升大版本时统一清理已标过时的接口, 清理清单写进发版说明.
- 从来没有发出过、也没有实现的声明不算接口, 可以在小版本里删, 在这里记一笔.

### 已过时

(暂无; repo-split 04 合入后登记 `acde:windows` 的 open、title、state、activate、parent、desk、geometry 事件, 替代者为 `window`)

## 结构

`src/index.d.ts`、`src/app.d.ts`、`src/shell.d.ts` (入口); `src/modules/` (`declare module 'acde:…'`); `src/types/` (清单、配置、服务、能力、
页面、事件、权限、office). 删除前是由 JS 常量推出来的两组类型, 现在直接写成字面量联合: `types/names.d.ts` (`IconName`、`ShellModule`,
原来是 `validate.ts` 里 `ICON_NAMES`、`SHELL_MODULES` 的元素类型)、`types/places.d.ts` (`SharedPlace`、`PlaceCapability`, 原来出自
`runtime/places.js` 的 `SHARED_PLACES`); `types/modules.d.ts` 的 `AcdeModules` 原在 `runtime/load.d.ts`.

## 没有进这个包的 (不是纯类型声明)

命令行要用的那部分搬进了命令行自己的源码 (用户 2026-11-17 定): Vite 插件在 acdev 的 `npm/src/vite-plugin.ts`; 清单校验原在 acdev 的
`npm/src/manifest/`, 运行时 (`js/page`) 也用它, 2026-11-18 拆仓时搬到本仓库的 `manifest/`, acdev 的 `npm/src/manifest/` 只剩转出它的薄壳.

| 原文件 | 原来做什么 | 现在 |
| --- | --- | --- |
| `src/runtime/load.js` (`load()`, 及各入口里 `load` 的声明) | 可选的 `acde:*` 模块: `import('acde:<name>')` 的默认导出, 拿不到时是 null | 没有: 应用直接写 `import('acde:<name>')`, 拿不到时 reject; `acdev check` 只拦可选模块的静态导入 |
| `src/runtime/level.js` (`API_LEVEL`) | 契约级别 (值 1); 运行时编译时读它, Broker 和 `acdev check/build/dev/doctor` 也用 | 值移到本包的 `api-level` (只写一个数字的纯数据, 随包发布); `src/host/mod.rs` 编译时 `include_str!` 读, `manifest/level.ts` 读同一文件 |
| `src/runtime/places.js` (`SHARED_PLACES`) | 共享位置的列表 (类型留下了) | `manifest/places.ts` |
| `src/runtime/platforms.js` (`PLATFORM_SUPPORT`、`RUNNING_MODES`、`platformSupport()`) | 各平台、各跑法下有哪些模块 | `manifest/platforms.ts` |
| `src/vite.js` (`@dopame/acde-api/vite`) | Vite 插件: 构建时把 `acde:*` 留作外部模块, dev 时换成会 reject 的替身 | acdev 的 `npm/src/vite-plugin.ts` (`acdev build` 用) |
| `src/validate.ts` (`@dopame/acde-api/manifest`) | 清单校验 (`Manifest`、`ICON_NAMES`、`SHELL_MODULES`、`APP_MODULES`、能力列表、导入规则), Broker 和 `acdev check/build` 共用 | `manifest/index.ts` |
| `src/frame.ts` (`FrameThemes`) | 窗框主题的颜色、样式解析成传给原生的格式 | 没有 (运行时一侧的逻辑) |
| `src/index.js` | 上面那些值的再导出 | 没有 |
| `scripts/build.mjs` | esbuild 打 `dist/` 并检查产物 | 没有 |
| `config.schema.json` | `acde.config.jsonc` 的 JSON Schema (编辑器补全) | acdev 的 `npm/templates/config.schema.json`, `acdev new` 拷进工程成 `acde.config.schema.json`, 模板的 `$schema` 指向它 |
| `test/` | 上面逻辑的测试 | 没有 |

`acde-dom` (DOM, 改用 blitz, 属系统侧) 和 `acde-host` (窗口表与策略归 winman; 应用登记、权限、服务宿主、应用互相打开归 acdebin-system)
是实现, 不搬.

## 现在因此不通的

- 发版: acdev 的 `npm/scripts/dist.ts` 要求 `--api` 是带 `src/index.js` 的构建产物, 和只有声明的包对不上. 这个包暂不发 npm, 按路径引用
  (用户 2026-11-17 定).

## 成员说明 (原声明里的注释)

本仓的代码不写注释, 删除前 `types/page.d.ts` 里的说明搬到这里:

- `AcdeWindows.animate`: 在窗口上放一段短动画. 运行时做不了动画时可以立即完成, 完成事件 `windowanimated` (`on('animated')`) 照样发.
- `OpenHandler.id`: acde 应用 id; 原生程序是它的 desktop entry (`x.desktop`), Windows 上是注册名 (路径或打包应用 id).
- `OpenHandler.icon`: 图标名或 URL; Windows 程序给 `data:image/png` URL 或 `''`.
- `OpenHandler.default`: 没指定应用时 `openPath` 用它打开这个文件 (用户记住的选择, 否则第一个合适的).
- `OpenHandler.system`: Windows: 这个程序是系统自己对这个扩展名的默认程序 (只读, acdev 从不改).
- `AcdeOpenFiles.handlers`: 能打开这个文件的应用. 系统不知道它的类型时 `mime` 是 `''` (Windows); `association` 是 `setDefault` 记住选择用的键:
  MIME 类型, Windows 上是扩展名 (`.md`).
- `AcdeOpenFiles.setDefault`: 为一个 MIME 类型 (Windows 上为一个扩展名, 如 `.md`) 记住 `handler`; 不动系统自己的默认.
- `AcdeWindow.setBackdrop`: 模糊窗口某块区域后面的东西. 平台画不准时可以近似 (见 `acde:capabilities` 的 `window.backdrop`).
- `AcdeWindow.on('beforeclose')`: 原来是一个签名 `() => boolean | void | Promise<boolean | void>`, 拆成返回 `boolean` 和返回 `void` 的两个重载, 能传的函数不变.

## 系统模块的错误与事件 (libacde 单子 12)

- **统一错误** (`AcdeError`, `AcdeErrorCode`): `acde:network`、`acde:system`、`acde:bluetooth`、`acde:audio`、`acde:devices`、`acde:power`、`acde:input`、
  `acde:display` 的方法失败时 (同步抛出或 Promise 拒绝) 都是 `name === 'AcdeError'` 的 `Error`, 带 `status`、`code`、`module`, 原错误放在 `cause`.
  这个包只有类型, 判别用 `error.name === 'AcdeError'`. 由页面胶水 `js/page/src/sysmodules.ts` 的 `checkedModule` 统一换, 按原错误归类:
  - `bad_request` (400): 参数不对 (原来抛 `TypeError` 的地方);
  - `forbidden` (403): 这个模块没有授予这个页面 (`not given to this web`);
  - `unsupported` (501): 这个运行时或平台做不了 (如没有合成器时的 `acde:input`、别的平台的 `acde:network`);
  - `service_not_running` (503): 系统服务不在 (NetworkManager、BlueZ、UPower、fcitx5 等不在, 或没有系统 / 会话总线);
  - `failed` (500): 其余. 原错误已经带 `status` / `code` 时 (经管家来的) 照用.
- **`acde:network` 的 `on('changed', handler)`**: 返回取消函数, 和别的系统模块一样; `watch(callback)` 保留作兼容写法.
- **`acde:power` 的事件载荷**: `PowerPromptEvent` (`prompt`)、`PowerCriticalEvent` (`critical`, `action` 和 `seconds`) 具名导出.
- **`acde:system` 的 `on('changed', handler)`**: 载荷 `SystemChange {storage, hostname, timezone, ntp}`, 每次是完整的一份. 有订阅者时页面胶水
  (`SystemWatch`) 每 30 秒一起读 `storage()`、`info().hostname`、`time()` 的 `timezone` 与 `ntp` (只比较这几项, 运行时间之类会变的字段不算),
  任一和上一次不同才发; 本页面 `setHostname` / `setTimezone` / `setNtp` 成功后立刻再查一次, 不等 30 秒. 最后一个订阅者取消就停,
  推送频率不会高过这个.
