# 桌面包

桌面包是一个目录, `package.json` 的 `acde` 字段 (类型 acde-api `manifest.d.ts`). 桌面要遵守的行为规则 (窗口、面板、锁屏页 …) 见 `docs/runtime/desktop.md`.

```json
"acdev": {
  "desktops": [{ "id": "dopame", "name": "Dopame", "entry": "dist/ui/desktop/index.html", "services": ["assistant"] }],
  "apps": [{ "id": "term", "label": "终端", "icon": "term", "entry": "/ui/terminal/term.js", "capabilities": { "terminal": true } }],
  "assets": { "/ui/": ["dist/ui"] },
  "services": [{ "name": "assistant", "module": "dist/services/assistant.js" }]
}
```

- **desktops**: `entry` 是包里的 HTML, 满屏在所有窗口后面; 桌面拿全部 `acde:` 模块. `heapMb` 默认 1024, 最小 32.
  `lock` (可选) 是锁屏页的入口, 写法同 `entry`. `Manifest.check` 对两者都要求是包里的相对路径 (不以 `/` 起头, 不含 `..`), 调用方给了
  `isFile` 时还查文件在不在 (acdev 打包检查给). 锁屏页只拿 `acde:lock` (`user()` / `unlock(password)`), 见 `docs/runtime/layout.md`「锁屏页」.
- **apps**: 见 `docs/runtime/webs.md`「应用清单」. **assets**: 同源 URL 前缀 → 包里的目录.
- **services**: 在 Broker isolate 里跑的 ES 模块, 默认导出 `ServiceFactory` (只用 `ServiceContext` 和 web 标准接口). 随桌面起停; 应用要在
  清单的 `services` 里写服务名才能调 (写在 capabilities 里是校验错误). 服务名不能用系统模块的名字 (`RESERVED_NAMES`). 重活放进 `ctx.spawn(argv, {cwd, env, onLine, onExit})` 起的进程 (`child.rs`: 自己的进程组, stderr 进日志加前缀
  `[<服务名>]`, 服务关闭后 SIGTERM, 3 秒后 SIGKILL).
  桌面清单的 `services` 项可以写成 `{name, modules}`, 列出这个服务能用的外壳模块; 服务经 `ctx.import('acde:<模块>')` 拿到 `{<模块>, default}`
  (Broker 侧 `broker/access/modules.ts` `ServiceModules.module(名字, 服务名)`), 没列的拒绝并说缺哪条声明. 服务还另有 `ctx.inspect`.
  `{name, modules, backend}` 的 `backend` 是这个服务连桌面后台时的角色名 (非空字符串, 否则校验错误); 写了角色而桌面没有 `backend` 时是警告.
  桌面的 `backend` 是 `{unit}` (systemd 系统单元模板名, 以 `@` 结尾、不含 `/`, 否则校验错误; `Manifest.desktopBackend`).
  **配置里怎么写这条授权** (`acde.config.jsonc`, SDK `DesktopConfig.services`: `Record<string, string | ServiceConfig>`, `config.schema.json` 同):
  每项是模块文件, 或 `{ "module": 文件, "modules": [外壳模块] }`. 读法只有一份: `Manifest.configServices(raw)` 给出 `{name, module, modules}[]`
  或一句错误; `modules` 交给清单那条规则 `Manifest.desktopServices` 判 (文字相同: `service "clock": modules lists shell modules (…)`), 没有 `module`
  (`a service is its module or {module, modules}`)、多出别的字段 (`"shell" is not a field of a service (module, modules)`) 也是错误.
  acdev 的 check / build 用它, build 把有 `modules` 的写成桌面清单的 `{name, modules}`. schema 里 `modules` 的枚举就是 `SHELL_MODULES` (测试对着).
  **服务拿到的是什么** (SDK `ServiceModuleTypes`, `ServiceContext.import` 按它定类型; 服务在 Broker isolate 里, 没有页面也没有 surface):
  `acde:windows` 是页面拿到的那一个 (`Required<AcdeWindows>`): `list()` 的行来自 `WindowRecords.rows()` (和页面的行同一个形状, 带 `state`), 调用走 `WindowCalls.windows`
  (调用者是 `WindowCalls.SERVICE`, 和桌面页面同一段检查; `launch` 的 `slot` 不算数, 服务没有窗口可嵌), `on(类型)` 收全局窗口事件 (不认识的类型抛 TypeError);
  `acde:packages` 同页面 (含 `on('changed')`); `acde:session`、`acde:desktops`、`acde:inspect`、`acde:setDefault` 同页面; `acde:desktop` 只有 `apps()` (钉面板、
  `setDoubleClick`、`setFrameTheme` 要页面或 surface, `download` / `notification` / `screenshot` / `shortcut` 事件只发给页面); `acde:permissions` 不给服务
  (`is not available to services here`); `acde:software` 同页面, 但没有 `on('changed')` (`Omit<AcdeSoftware, 'on'>`); `acde:voice` (语音识别: `listen(onEvent)` 回录音号, 事件 `partial` / `final` / `error`, `stop(号)` 结束录音等结果)
  只给服务, 在服务进程里直接调运行时的 `Voice` (同一时间一段录音, 新的取消旧的). SDK 里写成 Promise 的调用在服务里也回 Promise (`ServiceModules` 包一层 async). 事件: Broker 的 `#toDesktop` 同时交给
  `ServiceModules.emit`, 每个服务一份 `Notices` (`notices.ts`, 页面的 `on` 用的也是它), 处理函数在 Broker 线程上同步跑, 抛错只记日志; 服务停了 (或没起来)
  `release(服务名)` 丢掉它的订阅.

找包的顺序: acde-host 自己 (只有品牌资源)、`ACDE_PATH`、`$XDG_DATA_HOME/acde/packages/*`、`$XDG_DATA_DIRS/acde/packages/*`, 同 id 先到先得.
外部会话 (`src/foreign.ts`): `$XDG_DATA_DIRS/wayland-sessions/*.desktop`, id `session:<文件名>` (显示名去掉 "(Wayland)", KDE 的加 "KDE");
不列 acdev 自己的、`Hidden` / `NoDisplay`、程序找不到的.

**契约级别** (`apiLevel`): 每个包清单的 `acde` 字段带一个整数 `apiLevel`, 运行时的级别只在本仓库的 `acde-api/api-level` (只写一个数字) 定义 (acde-host 经
`@acde/api/manifest` 读, `<运行时> --version` 编译时从这个文件读). **最低支持级别** (2026-11-21, 接口只加不改、各协议声明最低版本与当前版本):
`acde-api/min-api-level` (同样只写一个数字, 1 ≤ 它 ≤ `api-level`, 现在也是 1), `@acde/api/manifest` 导出 `MIN_API_LEVEL`; 包的 `apiLevel` 在
`MIN_API_LEVEL..=API_LEVEL` 里就收 (`Manifest.supports`), 否则 `Manifest.refusal` 报 `needs level N, runtime is level M` (两者相同时) 或
`needs level N, runtime supports levels L–M`; acdes 的管家登记清单时按同一范围 (它编译时读这两个文件). 级别不在范围里或不是正整数的包 (`Packages.refused`) 不进应用列表、
桌面列表和打开方式, 启动它的应用报上面那句 (409, `api_level`), 选它的桌面同样报错. 没有 `apiLevel` 的包
(第 1 级之前构建的) 同样拒绝, 报 `Manifest.UNVERSIONED` (`no acde.apiLevel: ...; rebuild it with the current acdev (level 1)`), 要用现在的 acdev
重新构建; 校验器也把它当错误. 第 0 级 (契约级别之前的扩展期形态) 已全部去掉. acde-host 自己的包不查.
现在的契约是第 1 级.
**运行时还不是安全边界**: 按层级注入、能力和批准是契约, 不是隔离 (见 `docs/runtime/webs.md`「沙箱」);
第三方沙箱上线之前, 本地和开发构建的包都按开发者自己的代码运行.

**启动哪个桌面**: `--desktop <id|包目录>`, 否则 `ACDE_DESKTOP`, 否则上次进的 (`$XDG_STATE_HOME/acde/desktop`; `--greeter` 和 `app` /
`desktop` 子命令不记), 否则第一个 acde 桌面, 否则第一个外部会话. `<运行时> app` 没有桌面时用空的 (`Packages.bare`).
桌面经 `acde.desktops.choose(id, remember)` 换桌面.

**交出会话**: 选中外部会话时写交接文件 `$XDG_RUNTIME_DIR/acde/exec` (第一行是单引号引起来的 argv, 之后 `XDG_CURRENT_DESKTOP` /
`XDG_SESSION_DESKTOP` / `XDG_SESSION_TYPE=wayland` / `DESKTOP_SESSION`), 退出码 3; 会话入口 `/usr/bin/acdert` 读它并 `exec`.

**登录页** (`<运行时> --greeter`, greetd 的 greeter, acde-host `greet/`): 选账户、桌面、输密码; 会话是 `/usr/bin/acdert --desktop <id>` (外部会话是
它的 Exec 加那四个变量). 每个账户上次的桌面记在 `$XDG_STATE_HOME/acde/greeter.json` (greeter 的状态目录是 `/var/lib/acdev/greeter`). 别的显示管理器用
`<运行时> sessions` 生成的 `acdert-<id>.desktop`.
