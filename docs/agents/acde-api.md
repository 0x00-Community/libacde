# SDK `@acde/api` (`acde-api/`)

> 路径相对仓库根。

`@acde/api` 是运行时删掉的 `web/` 的替代品 (群里结论): 学 workerd, npm 包是薄壳, **只有类型声明, 没有 JS 逻辑** (plugin-api 副本例外, 见下「plugin-api 副本」).
它描述运行时给应用的东西: `acde:*` 模块 (运行时注入页面, 包里只是 `declare module`)、包清单、`acde.config.jsonc`、服务、能力、页面对象和事件的类型.
来源是删除前的 `web/packages/acde-api` (acdert `91e7dd1e` 的父提交, 本仓库的历史), 只取纯类型声明. 它和清单规则 `manifest/` 原在 acdev 的
`npm/` 里, 2026-11-18 随运行时库一起拆进本仓库: 运行时编译时要读它们, acdes 不该为此检出 acdev.

## 怎么引用

- 仓内: `js/tsconfig.json` 的 paths 把 `@acde/api` 指到 `acde-api/src/index.d.ts`; `manifest/` 用相对路径 `import type`.
- acdev 的命令行 (并排检出): `"@acde/api": "file:../../libacde/acde-api"` (acdev 的 `npm/package.json`), 用 `import type { … } from '@acde/api'`.
- 别的仓库或工程在本机: `"@acde/api": "file:<libacde 检出>/acde-api"`.
- 入口 (`package.json` 的 `exports`, 只有 `types` 条件, 没有 `default`): `.` (全部类型和全部 `acde:*` 模块)、`./app` (应用能导入的模块)、
  `./shell` (桌面和它自己的应用另外能导入的外壳模块). 页面里用 `/// <reference types="@acde/api/app" />` 拿到 `acde:*` 模块的类型.
  只能 `import type`: 包里没有 JS, 运行时 `import '@acde/api'` 找不到入口.
- 一个工程只用一个入口: 应用的类型从 `./app` 取, 桌面从 `./shell` 取; 和 `.` 混用时 `acde:*` 模块的声明重复, 类型检查报冲突 (原包就是这样).
- 还没发到 npm (发版要用户同意); 包现在 `"private": true`.

## plugin-api 副本 (`acde-api/vendor/plugin-api`)

- 声明式操作和插件工具同形 (见 `docs/agents/runtime/webs.md`「声明式操作」): `AppAction`、`ActionAnnotations`、`Caller`、`ToolRisk`、`JsonSchema` 只在
  plugin-api (AC/plugin-api, Dopame 与 form 共用的插件接口) 里定义一次, `acde-api/src/types/actions` 从副本 re-export; 清单规则 `manifest/actions.ts` 的
  名字规则 `TOOL_NAME` 和档位 `TOOL_RISKS` 也从副本取, `manifest/index.ts` 原样导出. 页面一侧专用的 `ActionCall`、`ActionResult`、`ActionError`、
  `ActionHandler`、`AcdeActions`、`ActionTarget` 仍在本仓库.
- 副本只读, 放在 `acde-api/` 里 (`package.json` 的 `files` 含 `vendor`), acdev 拷 acde-api 时一并带走. 只放类型和校验用得到的源码: 从 `src/tools.ts`
  出发顺着相对 import 取到的文件 (现在 4 个), 不带测试、例子和一致性用例. 这几个是 `.ts` 源码, 本包因此不再是纯 `.d.ts`, 但仍只供 `import type` 和清单规则用.
- `acde-api/vendor/plugin-api/SYNCED.json` 记源提交. 更新: 在 plugin-api 检出里提交好 (没有未提交改动), 然后 `bun scripts/sdk/plugin-api.ts sync`
  (源目录取 `PLUGIN_API_DIR`, 默认仓库旁边的 `../plugin-api`, 即里的检出), 跑类型检查和 page、launcher 的单测后连同 `SYNCED.json` 一起提交.
  `bun scripts/sdk/plugin-api.ts check` 核对副本和记下的提交一致 (源目录不在就跳过); 不一致说明有人直接改了副本: 改 plugin-api, 再 sync. 只复制 git 跟踪的文件,
  跳过 `AGENTS.md` 和 `.gitignore`.

## 版本与兼容 (用户定)

- 版本号跟 `acde-api/package.json` 的 `version`; 小版本 (含补丁) 只增不删: 新增类型、方法、事件、字段可以, 不删、不改名、不收窄已有的.
- 要替换的接口先标过时并保留可用: 运行时照旧实现, 旧事件照旧发出 (可以由新事件推出来). 标法两处都要:
  - 声明文件里, 在它上方写一行 `/** @deprecated 改用 <替代者> */`, 让编辑器划线. 这是代码不写注释的唯一例外 (用户定), 只限 `acde-api/` 的 `.d.ts`,
    只写这一行质量检查只放行这种单行写法;
  - 登记在下面的「已过时」清单: 替代者、从哪个版本起过时.
- 升大版本时统一清理已标过时的接口, 清理清单写进发版说明.
- 从来没有发出过、也没有实现的声明不算接口, 可以在小版本里删, 在这里记一笔.

### 已过时

| 接口 | 替代者 | 起于 |
| --- | --- | --- |
| `ServiceContext.backendToken()`、`onBackendToken()` | `ServiceContext.backend` 的 `connect()`、`up`、`onChange()` (连接由管家代办, 不再交令牌) | 下一个小版本 (系统代办连接 R3, 用户定) |
| `acde:windows` 事件 `open`、`title`、`state`、`activate`、`parent`、`desk`、`geometry` | `window` (每个窗口一批变化后的完整快照, `first` 表示新窗口, `changed` 列出变了的字段, `previous` 是上一个状态) | 0.1.0 之后的下一个小版本 (repo-split 04) |

运行时照旧发这 7 个事件: page 一层收到 `window` 快照后按 `first` / `changed` 推出来再发. `backendToken()` 运行时保留, 但永远回 null, `onBackendToken()` 永不回调.

从没发出过、在小版本里删掉的: `acde:windows` 的 `minsize` (`WindowMinSizeEvent`) 与 `animated` 事件 (只有已删的旧合成器会发, acompositor 下从来没有).

## 结构

`src/index.d.ts`、`src/app.d.ts`、`src/shell.d.ts` (入口); `src/modules/` (`declare module 'acde:…'`); `src/types/` (清单、配置、服务、能力、
页面、事件、权限、软件包 (`software.d.ts`, `acde:software`)、office; `acde:inspect` 的类型 (`InspectNode`、`InspectWindow`、`InspectAction` 等) 在子目录 `types/inspect/` (2026-11-23 从 `page.d.ts` 原样搬出, 为行数门禁; `page.d.ts` 照旧转出, 引用路径不变); 窗口外观 `WindowLook` 在子目录 `types/window-look/` (它用模块扩充给 `AcdeDesktop` 加 `setWindowLook`, `page.d.ts` 不动)). 删除前是由 JS 常量推出来的两组类型, 现在直接写成字面量联合: `types/names.d.ts` (`IconName`、`ShellModule`,
原来是 `validate.ts` 里 `SHELL_MODULES` 的元素类型; `IconName` 是任意 freedesktop 图标名的 `string`, 旧的 Dopame 私有名表 `ICON_NAMES`、`LegacyIconName` 2026-11-23 删掉, 过渡只留 `legacy-icons.json`)、`types/places.d.ts` (`SharedPlace`、`PlaceCapability`, 原来出自
`runtime/places.js` 的 `SHARED_PLACES`); `types/modules.d.ts` 的 `AcdeModules` 原在 `runtime/load.d.ts`.

## 没有进这个包的 (不是纯类型声明)

命令行要用的那部分搬进了命令行自己的源码 (用户定): Vite 插件在 acdev 的 `npm/src/vite-plugin.ts`; 清单校验原在 acdev 的
`npm/src/manifest/`, 运行时 (`js/page`) 也用它, 2026-11-18 拆仓时搬到本仓库的 `manifest/`, acdev 的 `npm/src/manifest/` 只剩转出它的薄壳.

| 原文件 | 原来做什么 | 现在 |
| --- | --- | --- |
| `src/runtime/load.js` (`load()`, 及各入口里 `load` 的声明) | 可选的 `acde:*` 模块: `import('acde:<name>')` 的默认导出, 拿不到时是 null | 没有: 应用直接写 `import('acde:<name>')`, 拿不到时 reject; `acdev check` 只拦可选模块的静态导入 |
| `src/runtime/level.js` (`API_LEVEL`) | 契约级别 (值 1); 运行时编译时读它, Broker 和 `acdev check/build/dev/doctor` 也用 | 值移到本包的 `api-level` (只写一个数字的纯数据, 随包发布); `src/host/mod.rs` 编译时 `include_str!` 读, `manifest/level.ts` 读同一文件 |
| `src/runtime/places.js` (`SHARED_PLACES`) | 共享位置的列表 (类型留下了) | `manifest/places.ts` |
| `src/runtime/platforms.js` (`PLATFORM_SUPPORT`、`RUNNING_MODES`、`platformSupport()`) | 各平台、各跑法下有哪些模块 | `manifest/platforms.ts` |
| `src/vite.js` (`@acde/api/vite`) | Vite 插件: 构建时把 `acde:*` 留作外部模块, dev 时换成会 reject 的替身 | acdev 的 `npm/src/vite-plugin.ts` (`acdev build` 用) |
| `src/validate.ts` (`@acde/api/manifest`) | 清单校验 (`Manifest`、`LEGACY_ICONS` (过渡用, 读 `legacy-icons.json`)、`SHELL_MODULES`、`APP_MODULES`、能力列表、导入规则), Broker 和 `acdev check/build` 共用 | `manifest/index.ts` |
| `src/frame.ts` (`FrameThemes`) | 窗框主题的颜色、样式解析成传给原生的格式 | 没有 (运行时一侧的逻辑) |
| `src/index.js` | 上面那些值的再导出 | 没有 |
| `scripts/build.mjs` | esbuild 打 `dist/` 并检查产物 | 没有 |
| `config.schema.json` | `acde.config.jsonc` 的 JSON Schema (编辑器补全) | acdev 的 `npm/templates/config.schema.json`, `acdev new` 拷进工程成 `acde.config.schema.json`, 模板的 `$schema` 指向它 |
| `test/` | 上面逻辑的测试 | 没有 |

`acde-dom` (DOM, 改用 blitz, 属系统侧) 和 `acde-host` (窗口表与策略归 acompositor; 应用登记、权限、服务宿主、应用互相打开归 acdes)
是实现, 不搬.

## 现在因此不通的

- 发版: acdev 的 `npm/scripts/dist.ts` 要求 `--api` 是带 `src/index.js` 的构建产物, 和只有声明的包对不上. 这个包暂不发 npm, 按路径引用
  (用户定).

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
- `AcdeWindow.setBackdrop`: 模糊窗口某块区域后面的东西; `effect.shape` 是角形 (`round` / `squircle` / `superellipse(k)`, 类型 `BackdropEffect`). 平台画不准时可以近似 (见 `acde:capabilities` 的 `window.backdrop`).
- `AcdeWindow.setMaterial(material, options?)` / `material()` (类型在 `types/window-look/` 用模块扩充加到 `AcdeWindow`): 整扇窗口 (或 `region: 'titlebar'` 只有标题栏) 的背景材质, 画在窗口内容之下.
  `MaterialName` 内置 `glass` (液态玻璃, `role` 是 `regular` / `clear`, 可用 `group` + `spacing` 跨窗口融合)、`blur` (背景模糊加着色, 没有透镜和高光)、`none`; 其余名字留给以后的自定义效果,
  所以类型是 `'glass' | 'blur' | 'none' | (string & {})`. 名字不合规 (`[a-z0-9._-]`, 至多 64 字节)、`role` / `region` / `spacing` / `group` 写错抛 `TypeError`; 合规但没有这个材质时按 `none` 画并记一次警告,
  `material()` 回实际生效的名字 (此时是 `none`) 和给的选项. `overrides` 是"参数名 → CSS 写法"的表, 参数名同 `--acde-glass-*` 去掉前缀 (`blur`、`tint`、`outline` …), 档位、深浅、
  减少透明度仍由系统定 (减少透明度时一律不透明). 清单 `window.material` 写默认值 (名字或 `{name, role, group, spacing, overrides, region}`), 窗口一开就用.
  给开发者的建议: 长时间存在的主窗口背景用 `blur` (开销中等), 浮在内容上的工具栏和小面板窗口用 `glass` (最贵), 临时弹出用 `blur`; 页面背景要透明, 材质才透得出来.
- `AcdeWindow.accent()` / `on('accent', handler)` (类型同在 `types/window-look/`): 系统强调色的原值 `#rrggbb`, 系统没设 (门户的 `accent-color` 是 -1 或越界) 或没有门户 (宿主会话) 时是 `null`;
  原值变了发 `accent`, 参数是新值. 用来分清"没设"和"用户正好选了默认色": CSS 的 `AccentColor` 在没设时是默认的 `#0075ff`, 区分不开. 来源只有一条, 同 `AccentColor`
  (门户 → launcher → 页面消息 `{type: 'accent', color}`, 见 `docs/agents/dev/build-run.md` 的强调色); 页面初始化参数 `accent` 和这条消息都原样保留 `null`, 渲染收空串表示没设.
- `AcdeWindow.wallTone()` / `on('wallTone', handler)` (用户选定的公开方法, 类型 `WallTone = 'light' | 'dark' | null` 同在 `types/window-look/`): 窗口所在屏的壁纸整体偏亮还是偏暗,
  `light` 表示壁纸偏亮、字该用深色; 还没收到壁纸图、算不出来或没有门户 (宿主会话) 时是 `null`. 壁纸变了或窗口换到别的屏、值变了才发 `wallTone`.
  来源是 acdes 算好经门户 `org.acde.appearance` 推的 `wall-tone` (`a{su}`, 键是屏幕名, 值 0 未知、1 亮、2 暗) 和 `primary-output` (`s`, 主屏名);
  启动器按窗口表行的 `output` (窗口中心所在屏) 取值, 取不到用主屏, 都没有是 `null`, 经页面消息 `{type: 'walltone', tone}` 和初始化参数 `wallTone` 交给页面.
- `acde:actions` (J2, 类型在 `types/actions/`): `handle(name, handler)` 给清单 `acde.apps[].actions` 里声明的操作注册处理函数,
  `handler(args, {caller, target?, signal, progress})` 回 `{ok: true, output?}` 或 `{error: {code, message}}`. 声明的形状 `AppAction` 和插件的工具同形,
  `Caller`、`ToolRisk`、`JsonSchema` 也在这里 (dopame 的插件接口以后改为从这里导出, 不再各写一份). 见 `docs/agents/runtime/webs.md`「声明式操作」.
- `AcdeWindow.on('suspend' | 'resume')` (J3): 系统挂起、恢复这扇窗口的应用时发; 收到 `suspend` 后页面的计时器和动画帧被节流, 清单
  `lifecycle.suspendable: true` 的应用会被冻结 (这之后不再跑任何代码, 所以要存的东西在处理函数里同步存好). 清单的 `lifecycle: {suspendable?, endsAnytime?}`
  (缺省都为 false) 见 `docs/agents/runtime/webs.md`「挂起与恢复」; `endsAnytime` 告诉系统这个应用可以不经询问直接结束.
- `AcdeWindow.on('beforeclose')`: 原来是一个签名 `() => boolean | void | Promise<boolean | void>`, 拆成返回 `boolean` 和返回 `void` 的两个重载, 能传的函数不变.

## 系统模块的错误与事件

- **统一错误** (`AcdeError`, `AcdeErrorCode`): `acde:network`、`acde:system`、`acde:bluetooth`、`acde:audio`、`acde:devices`、`acde:power`、`acde:input`、
  `acde:display` 的方法失败时 (同步抛出或 Promise 拒绝) 都是 `name === 'AcdeError'` 的 `Error`, 带 `status`、`code`、`module`, 原错误放在 `cause`;
  原错误自己的代码 (如 napi 一侧 `network_failed`) 放在可选的 `reason`. 这个包只有类型, 判别用 `error.name === 'AcdeError'`.
  页面胶水 `js/page/src/sysmodules.ts` 的 `checkedModule` 统一换: 原错误的消息是 `{"code", "message"}` 的 JSON (runtime `failure()` 和 `bad_request()`
  这样编) 时先拆开, 消息用里面的原文. 归类顺序:
  1. 显式带的代码是下面五种之一就照用 (Rust 一侧参数校验用 `crate::runtime::bad_request` 带上 `bad_request`, 如主机名、时区、电源设置、亮度、
     蓝牙 / 声音设备号、输入法; offload 的 `settle_json` 把这种 JSON 拆成消息加 `code`);
  2. `forbidden` (403): 消息含 `not given to` / `not in the capabilities` (没授予);
  3. `unsupported` (501): 这个运行时或平台做不了; `service_not_running` (503): 系统服务或总线不在 (NetworkManager、BlueZ、UPower、fcitx5 等);
  4. `bad_request` (400): 原错误是 `TypeError` (宿主一侧的参数检查);
  5. `failed` (500): 其余. 原错误带数字 `status` 时照用.
- **导入被拒** (`src/host/modules.rs` 的 `import_error`): `acde:*` 模块因为清单没声明而不给时, 动态 `import()` 拒绝、静态 `import` 抛出的都是
  同一个 `AcdeError` (`code: 'forbidden'`、`status: 403`、`module` 是去掉 `acde:` 的名字, 消息保留原文); 提供它的服务停了而被拒时是
  `service_not_running` / 503. 别的模块解析错误照旧是 `TypeError`.
- **`acde:network` 的 `on('changed', handler)`**: 返回取消函数, 和别的系统模块一样; `watch(callback)` 保留作兼容写法.
- **`acde:power` 的事件载荷**: `PowerPromptEvent` (`prompt`)、`PowerCriticalEvent` (`critical`, `action` 和 `seconds`) 具名导出.
- **`acde:system` 的 `on('changed', handler)`**: 载荷 `SystemChange {storage, hostname, timezone, ntp}`, 每次是完整的一份. 有订阅者时页面胶水
  (`SystemWatch`) 每 30 秒一起读 `storage()`、`info().hostname`、`time()` 的 `timezone` 与 `ntp` (只比较这几项, 运行时间之类会变的字段不算),
  任一和上一次不同才发; 本页面 `setHostname` / `setTimezone` / `setNtp` 成功后立刻再查一次, 不等 30 秒. 最后一个订阅者取消就停,
  推送频率不会高过这个.
