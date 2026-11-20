# 目录

| 路径 | 职责 |
| --- | --- |
| `src/host` | `acde` lib: 内嵌 V8 (rusty_v8), 每个 isolate 的事件循环 (`rt.rs`), 模块加载 (`modules.rs`), web 线程 (`web.rs`), DOM 绑定 (`dom.rs`), 注入的能力, 声音, inspector, 主进程 (`supervise.rs`, Linux), 自检 (`doctor.rs`) |
| `crates/shell` / `crates/window` / `crates/paint` | 会话 (Wayland 合成器、会话里 web 的 Wayland 客户端和光栅化) 已搬到 [acdebin-system](http://gitea.dopamine/AC/acdebin-system), 说明在它的 AGENTS.md |
| `src/render` | Blitz 包成的渲染引擎 (`Document` / `Dom` / `RenderHandle`), 窗框 `Decoration`. `Dom` 在 `dom.rs` (结构、加锁读写的辅助、小工具) 和 `dom/`: `types.rs` (`NodeType` / `Ns` / `Viewport` / `Quad`)、`values.rs` (控件种类、数值字符串、`RangeValue`)、`dom_tree.rs` / `dom_view.rs` (`Dom` 的公开方法: 树、属性、文本、选择器 / 表单、几何、样式、图片、样式表、画布)、`engine_tree.rs` / `engine_state.rs` (`Engine` 上对应的实现: 节点与树操作、解析、属性 / 控件值、焦点选区、滚动、图片状态、innerText)、`select.rs`、`semantics.rs` |
| `src/gpu` | 设备层: 共享设备 `Gpu` (包着 src/vello 的 `Shared`; WebGPU / WebGL / video / canvas 图都在它上面), 接口 `Compositor` (前端在发布 `Gpu` 时用 `Gpu::set_compositor` 装上) 交出 `Offscreen` (render 画 canvas、滤镜、阴影、截图用); 平铺导出 `acde-scene` 的 2D 场景 (`Scene` / `PaintScene` / 滤镜)、`Part` / `Pass` / `Content`、`Px` / `Backdrop` / `Shadow` / `Effect`、`GpuImage` |
| `src/vello` | wgpu 后端: vello 画场景 (`Vello`), 合成 (`Composer`, `compose.wgsl`), 离屏 (`Offscreen`: canvas、滤镜、阴影, `blur.wgsl` / `matrix.wgsl`), `GpuImage` / `ImageSet` / `Shared`, `VelloCompositor`; Linux 上 dma-buf 画布导入和 sync_file (`Device`, 会话的 `Raster`). acdev runtime 和会话用它. detour 后端以后放 `src/detour` (feature `detour`) |
| `scene/` | `acde-scene`: 2D 场景 (`Scene` / `PaintScene` / `RenderCommand`)、滤镜 (`Filter` / `FilterEffect`)、合成的部件 (`Part` / `Pass` / `Content` / `Px` / `Backdrop` / `Shadow` / `Effect`, unix 上 `Dmabuf` / `Submitted`). blitz fork、本库和后端共用, 只依赖 kurbo / peniko; 发到 ac |
| `src/webgl` | `acde-webgl`: wgpu 上的 WebGL 1 / 2 (见 `docs/graphics/webgl.md`「WebGL」). 检查: `cargo test --release -p libacde webgl` |
| `src/runtime` | 系统接口 (napi 导出 + 注入用的 Rust 对象). 异步执行器 `rt.rs`: `acde-rt` 线程上的 ntex System (不用 tokio, TLS 用 rustls + ring (不用 aws-lc: Windows 要 NASM, 每个平台都多编一个 C 库; 用户定 2026-11-18), 证书校验用 rustls-platform-verifier). 检查: `cargo test -p libacde runtime` (含真 pty / Windows 伪控制台上的终端、unix socket 上的 HTTP / WebSocket)、`ime-check.sh`、`portal-check.sh`、`atspi-check.sh` (经 `tests/run.sh`) |
| `src/model` | canvas / 窗口表、计划、快捷键、shell 事件 (`Event` / `Inbox`) |
| `src/log` | `acde-log`: 日志和计时 |
| `src/media` | `acde-media`: 解码音视频, 读摄像头和麦克风 (Linux GStreamer, macOS AVFoundation; 见 `docs/web/web-apps.md`「视频」「摄像头和麦克风」). 检查: `cargo test --release -p libacde media` (`tests/fixtures` 是 ffmpeg 命令行生成的 1 秒小片段; `tests/capture.rs` 用假设备; `tests/devices.rs` 里 `--ignored` 的列出 / 读真设备、在 macOS 上问权限) |
| `src/storage` | `acde-storage`: 网页存储的分区和 SQLite (见 `docs/web/storage.md`「网页存储」). 检查: `cargo test --release -p libacde storage` |
| `src/napi` | rusty_v8 实现的 Node-API (napi8), 让 `#[napi]` 导出在 Broker 里可用. 改自 Deno `ext/napi` 0.191.0 (MIT, LICENSE-MIT), 去掉了用不到的入口 |
| `js/dom` | 网页接口层: web 里的浏览器全局对象和 DOM (原 acde-dom) |
| `js/page` | 页面一侧的胶水: 在 web 里起页面、`acde:` 模块、和启动方的 rpc、输入与窗框 (原 acde-host 的 `worker.ts` 及其依赖) |
| `js/launcher` | 单应用启动器: 一个进程开一个应用一个窗口 (临时代替原管理中枢, 见下). `src/` 下按职责分: 根上是入口 `main.ts` 和 `args.ts`; `app/` 找应用、拼页面初始化数据、本进程回答的调用 (`calls.ts`) 和事件 (`events.ts`); `manager/` 与管家的连接和服务模式; `dev/` 调试通道、DevTools、语义树与操作; `backend/` 应用后端 |

## js: 运行时自带的 JS

`web/` 于 acdert 91e7dd1e 删除 (CTO 决定) 后, 网页接口层和页面胶水按 system 第一个里程碑 (gitea `AC/acdebin-system#15`, 工作项 05;
用户 2026-11-17: "不出现 JS" 只针对 SDK) 从 git 历史 (91e7dd1e^) 取回, 放在 `js/`, 是运行时的内部组件: 不发布、不进 SDK,
也不再有单独的 `web.js` 文件. `build.rs` 用 bun (`ACDEV_BUN`, 默认 `bun`) 把 `js/page/src/worker.ts` 打成一个 ES 模块
(`$OUT_DIR/page.js`, 压缩空白和语法、保留名字), 契约级别取 `acde-api/api-level`、版本取包版本 (`ACDEV_API_LEVEL`、
`ACDEV_RUNTIME_VERSION` 两个 define), `src/host/web.rs` 用 `include_str!` 嵌进二进制, web 线程从 `acde-runtime:/page.js` 加载它
(`WebLoader` 认这个地址); 同样把 `js/page/src/webworker.ts` 打成 `webworker.js`, Web Worker 的线程从 `acde-runtime:/webworker.js` 加载它
(它只引 acde-dom 的 `core/` 和 `worker/`, 不带 DOM). 所以构建运行时库要有 bun. 打包命令只在 `js/bundle.ts` 一处 (`bun bundle.ts [输出目录] [版本]`, 默认 `js/dist`, `build.rs` 也调它).
调试版运行时设了 `ACDEV_JS_DIR` 时从那个目录读 `page.js`、`webworker.js`、`launcher.js`, 读不到才用内嵌的 (发布版只用内嵌的): 改 `js/` 后
`cd js && bun run bundle` 一两秒出新包, 重开页面即生效, 不重编 Rust; 进客户机时只推这几个文件. `js/tsconfig.json` 的 paths 把 `@acde/dom` 指向 `js/dom/src`,
`@acde/api` 指向 SDK 的声明 `acde-api/src`, `@acde/api/manifest` 指向清单模块 `manifest/`
(acdev 的命令行用同一份清单规则, 见 `docs/acde-api.md`); 窗框主题的解析 (`FrameThemes`) 只有页面用, 在 `js/page/src/frametheme.ts`. 页面和启动方之间的类型
(`WebInit`、服务形状、菜单、权限等级) 在 `js/page/src/contract.ts`; 管理中枢 (原 `broker.js`) 没有取回: 管家部分归 system (工作项 09).

单应用启动器 (`js/launcher`, 用户 2026-11-17 定的临时做法, 总仓 `.scratch/acdev/issues/09-temporary-launcher-and-old-js.md`) 和页面包一样由
`build.rs` 打成 `$OUT_DIR/launcher.js` 嵌进二进制 (`src/host/bundle.rs` 管这两个地址), 原来进管理中枢的入口 (`Entry::Broker`) 现在都从
`acde-runtime:/launcher.js` 起. 它认 `app <id|目录>` 和 `desktop [id]`, 都可带 `[--size WxH] [--scale N] [--seconds N] [--shot 文件]
[--shot-after 毫秒]`, 以及只在有管家时可用的 `service <名字>`. 一个进程一个页面 (一个应用一个窗口, 或一个桌面), 或一个后台服务.

- **有管家时** (会话里, 环境里有 `ACDE_MANAGER_FD` 且 `native.Manager` 在, acdebin-system 的管家, 总仓 `.scratch/system/issues/09-manager.md`,
  协议见 acdebin-system `docs/manager.md`): `native.Manager.connect()` 拿 welcome, 应用的清单、包目录、资源、等级 (`tier`)、授予 (`grants`)、
  拉起时的请求 (`request`) 都取自它; 桌面的页面是 `welcome.dir` 加 `welcome.desktop.entry`. 子进程的环境里不再带 `ACDE_MANAGER_FD`.
  页面的调用除下面留在本进程的以外原样转给管家 (`manager.call`), 失败的 `Error` 带 `status`、`code` 抛回页面; `window.ready`、`page.shown`
  也转过去 (管家据此记冷启动). 管家推来的 `request` 交给页面 `window.request`, 桌面进程收到的其余推送原样交给桌面页, `managerclosed` 时以 0 退出.
- **没有管家时** (宿主窗口跑法): 只认 `app`; 目录取其 package.json 里第一个应用, id 在 `ACDE_PATH` 的各目录及其下一层找; 资源前缀按
  `acde.assets`, 没有 `/` 时 `/` 指包目录; 没有服务、最低权限等级、存储在 `$ACDE_STORAGE` 或 `~/.local/share/acde/storage` 的 `app-<id>`.
  文件 (用户 2026-11-18): 清单声明了 `fs` 就授予 `{places, paths: false}` (形状同客户机管家的 `grants.fs`): 只在设了 `ACDE_DEV_PLACES` (`acdev dev` 设,
  它同时用 `XDG_<名>_DIR` 把公共位置换成会话自己的隔离目录) 时给清单 `fs.roots` 列的位置, 否则只有应用私有目录; 不给任意路径.
- **锁屏页预览** (`lock <桌面 id|目录>`, 只在没有管家时, `acdev dev --lock` 用, 日常桌面工作项 23): 按 `ACDE_PATH` 或目录找桌面包, 打开它清单 `lock`
  写的页面 (桌面窗口, 等级最低, 授予全无, 页面初始化数据带 `lock: true`, 所以只有 `acde:lock` 和 `acde:services`). `lock.user` / `lock.unlock` 由
  `js/launcher/src/app/lock.ts` 的 `FakeLock` 回答: 固定用户 (头像 null); `right-password` 解锁, `wrong-password` auth (403)、`busy-password` busy (409)、
  `throttled-password` throttled (429, `retryAfter` 3), 别的 auth. 有管家时 `lock` 命令报错, `lock.*` 照常交给管家.
  `window.ready`、`page.shown` 直接应答, `desktops.list` 回 `{desktops: [], current: ''}`, 其余 (含 `session.*`、
  `desktops.choose`、`lock.*`) 回 `unsupported` (501).
  `window.setTitle` 记下标题 (语义树、`webs`、DevTools 目标用它); `window.setMenus` 交给宿主窗口 (`Shell.setMenus`, 菜单画在框上), 带 `shortcut`
  的菜单项注册成 `menu` 范围的快捷键, 宿主推来的 `menuselect` 和按下的菜单快捷键转成页面的 `window.menu` (`app/menus.ts`); `page.open` / `page.save`
  (页面的打开、保存文件选择器) 用宿主的 `FileChooser`, `page.notify` 用 `Shell.notify` (`app/page.ts`); `page.download` 仍回 501.
- **会话与桌面包** (`acde:session`、`acde:desktops`, libacde 工单 04): 页面胶水 (`RemoteBridge`) 给桌面页定义这两个模块, 应用页在
  `tier.shell` 含 `session` / `desktops` 时定义. `session.logout/reboot/poweroff/setLanguage(tag)`、`desktops.list()` (回
  `{desktops: DesktopInfo[], current}`)、`desktops.choose(id, remember?)` 经 rpc 转出, 有管家时原样 `manager.call`. 同步的
  `session.lock()` 同样经 rpc 转出 (管家调 logind 的 Lock). `session.own()` 取 `WebInit.session.own`: 启动器把 welcome 顶层的 `session: {own}` 带进去, 没有这一项或没有管家时为 false.
  管家 (acdes) 推来的服务里若也有 `session` / `desktops`, 会盖掉这两个模块; acdes 在本工单后去掉登录模式里的这条绕法.
- **锁屏页** (`acde:lock`, 日常桌面工作项 18; 管家一侧见 acdes `docs/manager.md`「锁屏」): 管家以锁屏模式起的进程收到的 welcome 是桌面的形状
  (`kind: 'desktop'`) 加顶层 `lock: true`, 启动器打开 `join(dir, desktop.lock ?? desktop.entry)` (桌面没写 `lock` 时管家给 acdes 自带的锁屏包),
  页面初始化数据带 `lock: true`. 页面胶水对锁屏页只定义 `acde:lock` 和 `acde:services`, 不给 `session` (锁屏页不能关机、重启) 及别的外壳模块.
  `lock.user()` 回 `{name, fullName, avatar}` (`avatar` 是 `data:` URL 或 null), `lock.unlock(password)` 失败时的错误带 `code`: `auth` (403,
  密码错)、`busy` (409, 上一次验证还没完)、`throttled` (429, 失败后的延迟期内, 另带 `retryAfter` 秒数); 这些字段由 rpc 原样带回页面
  (SDK 类型 `LockError`). 管家按调用方拒绝: 锁屏页只能调 `lock.*`、`window.ready`、`page.shown`.
- **服务模式** (`service <名字>`, system 工作项 12, 帧格式见总仓 `.scratch/system/issues/12-services.md`, `js/launcher/src/manager/service.ts`):
  不开窗口, 在启动方的 isolate 里 `import` welcome 的 `module` (绝对路径), 用 ServiceContext (`name`、`package` 即包目录、`state` 即管家建好的
  `stateDir`、`native`、`env`、`log`、`spawn`, `import('acde:<模块>')` 只放行 welcome 的 `modules`, 其方法经管家调用) 调它的默认导出;
  起好后向管家发 `{ready: {provides: {命名空间: {methods, values, sync, callable}}}}` (形状同旧管理中枢). 管家转来的调用
  (`{type: 'call', id, service, method, args}`, `method` 为空串时调命名空间本身) 里的 `{__acdeFn: g}` 换成回调, 回调发
  `{invoke: {fn: g, args}}`; 收到 `release` 后这个回调不再发. 应答 `{id, ok}` 或 `{id, error: {status, code, message}}`.
  `managerclosed` 时调 `close()`、停掉它起的子进程 (最多 3 秒) 后以 0 退出. 从开始答调用到 `managerclosed` 之间挂着一个计时器保活
  (`holdTheLoop`): 服务模块里没有别的挂起任务时, 事件循环也不会空了就结束 (2026-11-18 大联测里 5 个服务 ready 后十几秒内自己退出过). 需要 system 的 `native.Manager` 提供 `send(帧)`, 并把转来的调用和
  `release` 从 `pump()` 交出 (`{type: 'call', ...}`、`{type: 'release', fn}`, 也认不带 `type` 的原帧).
- **应用与桌面里的服务**: welcome 的 `services` (命名空间到形状) 进 `WebInit.services`, 页面据此生成代理; 管家推 `{type: 'services', services}`
  时整份交给页面替换代理 (`RemoteBridge.replaceServices`). 页面对这些命名空间的调用不经启动方的 rpc 解开, 原样 (含 `{__acdeFn: n}`) 转给
  `manager.call`, 结果或错误直接回给页面; 管家推来的 `{type: 'invoke', fn, args}` 转成页面 rpc 的 `invoke`.
- **服务列表变化** (`acde:services`, 应用与桌面都有, 不用声明): `names()` 给当前的命名空间, `on('changed', ({names, added, removed}) => …)`
  在推来的列表 (含形状) 和上一份不同时通知. 通知前新命名空间已能 `import('acde:<名字>')`, 形状变了的重新 import 拿新代理, 去掉的再 import
  会被拒 (`modules.rs` 的 `define` / `refuse` 会丢掉缓存的模块; 已拿到手的旧代理仍在, 调用由管家报错). 桌面先于服务起来时 (客户机上服务要
  几十秒), 页面在 `changed` 里重新 import 并初始化, 不用轮询. 命名空间 `services` 留给这个模块, 服务提供同名命名空间会被忽略.
- **两种都留在本进程的调用** (`js/launcher/src/app/calls.ts` 的 `route`): `window.close` 和以自己窗口号调的 `windows.close` 关掉窗口 (结束页面线程,
  进程随之退出); `window.minimize`、`window.requestState('minimized')` 让页面自己最小化 (其余状态回 501); `window.reload([url])` 结束页面线程、
  对同一个窗口重新授予后再起 (url 须在 `http://acde.local/` 下); `capabilities.get/list` 按清单声明回 `available` / `not-declared`
  (临时, 不看平台与逐次同意). 桌面另答 `panel.open/place/close` (钉住的面板, 同旧管理中枢) 和 `windows.*` (`close`、`kill`、`focus`、`hide`、
  `show`、`requestState`、`screenshot`、`animate`、`stats`、`shortcut`、`unshortcut`、`place`、虚拟桌面的 `addDesk`、`removeDesk`、
  `showDesk`、`moveToDesk` 交给窗口系统对象的同名方法 (acdes 里是 `native.Windows.connect()`, acdev 宿主窗口里是 `native.Shell.broker()`; `animate`、`stats` 在 acdes 里不做事); `launch` 有管家时转给 `launch.start`;
  多屏方案 A 的副屏壁纸 `setWallpaper({color, image?})` 只给桌面页, 先查 `color` 是 `#rrggbb` / `#rrggbbaa` (否则 400 `bad_color`)、`image` 是绝对路径或 null
  (否则 400 `bad_request`), 再交给窗口系统对象的 `setWallpaper(color, image?)` (acdes 里是 `native.Windows`, 值由桌面进程记住, 重载后保持; 宿主窗口没有这个方法);
  不是桌面或窗口系统对象没有这个方法时 501 `unsupported`).
- 窗口: 应用向窗口系统要一个占满宿主窗口的窗口; 桌面 `grant({desktop: true})` (窗口号 0). 窗口系统的关窗请求 (`windowcloserequest`, 如 xdg
  的 close) 先问页面 `window.beforeclose` (最多等 3 秒, 页面答 `false` 就不关), 再同样关掉. 桌面进程把窗口系统的其余事件
  交给桌面页 (`js/launcher/src/app/events.ts` 的 `WindowSnapshots`): acdes 给的是 winman 形状的快照 `window` (每个窗口一批变化后一条, `first` 表示新窗口,
  `changed` 列出变了的字段, `previous` 是上一个状态), 原样转; acdev 宿主窗口还发旧的 `windowopen` / `windowtitle` / `windowstate` / `windowactivate` /
  `windowparent` / `windowdesk` / `windowgeometry`, 这里按窗口记下快照, 转成同样的 `window` 再交; `windowclose`、`windowcrash`、`currentdesk` 原样转. 页面的窗口列表 (`js/page/src/remote.ts` 的 `WindowList`) 记下 `rect`、`instance`、`desk`.
- 启动方的 isolate 里先装好核心全局对象 (定时器、console 等, `installCore`). 宿主窗口关闭、页面退出或 `--seconds` 到时退出, 退出码是页面的; 收到 SIGTERM / SIGINT / SIGHUP 时 (`acdev dev` 停运行时就是 SIGTERM) 也照常收尾 (停后端、删 socket 目录) 再以 0 退出 (同原 Broker: 按要求停下不算出错), 同一信号再来一次就直接退出 (SIGINT 130, 其余 143). 不收尾的话 macOS 上后端会成孤儿: 那里没有 Linux 的父进程退出信号.
  `--shot` 在 `--shot-after` (默认 2000) 毫秒后存一张宿主窗口截图, 无头 (`ACDE_HEADLESS=1`) 也行; 首帧没画完时截到的是空屏 (WSL 的 Dozen 上
  调试版首帧要 5～6 秒, 验证时用 `--shot-after 15000`).
- **调试通道** (`js/launcher/src/dev/dev.ts` 的 `DevChannel`, acdev-single-app 工作项 10、11): 宿主窗口的 `devrequest` 事件 (hostwin `dev.rs`
  转来的 `acdev dev` 请求, 协议见 `docs/dev/dev-ipc.md`) 由本进程按单页回答, 结果经 `Shell.devRespond` 回去; 页面发来的 `{devlog}` (console、
  错误、请求、对话框) 收进 `DevLog`, `{devroute}` 记下假回应用掉一次. `dev-ipc.md` 的请求类型都回答: `snapshot` / `act` / `run` / `query` /
  `input` 由 `inspect.ts` (删 `web/` 前的 `inspect.ts` 取回, 只登记本进程的窗口) 经页面的 `inspect` 服务做; `route` / `init` 记在启动方,
  当场推给页面并在重载时随 `WebInit.devRoutes` / `devInit` 带上; `reload` 和 `acdev dev` 改了文件后的 `devreload` (非热替换时) 先取页面状态
  (`dev.state`, 最多 500 毫秒) 再重新找应用、重开页面并带上 `devState`, 热替换只发 `devhot`. 单应用的特例: `open` 报“没有别的应用”;
  `permissions` 恒为空、`answer` 找不到提示、`revoke` 回 `revoked: false` (单应用不问用户); `resize` 改宿主窗口大小 (`resizeScreen`);
  `emulate` 的系统配色只认 `ACDE_COLOR_SCHEME=dark`.
  页面线程结束 (重载、跳到应用里另一个 HTML) 时, 等在它上面的 rpc 调用一律以“页面已离开”失败, `act` / `run` 把这当成点击引起的跳转照常回答, 不再卡到超时.
- **DevTools** (`js/launcher/src/dev/devtools.ts` 的 `DevToolsEndpoint`, 工作项 12): 运行时的 `DevTools.open()` 开了端点 (设了 `ACDE_DEV` 时) 就每 250 毫秒
  把本页登记成 CDP 目标 (id 是 web 名, 列表没变不重登); `devtools` 请求回 `{port}`; 重载前调 `reloading(web)` 让连着的 DevTools 接上新页面;
  宿主窗口的 F12 (`devtools` 事件) 经 `Shell.devEvent` 发 `{type: 'event', event: 'devtools', web, app, window}`, 由 `acdev dev` 打开 DevTools.
- **应用后端** (`js/launcher/src/backend/backend.ts` 的 `AppBackend`, 工作项 13; 用户 2026-11-18 定由启动器起, 宿主和客户机同一份): 应用清单有 `backend`
  时随应用起, 地址 (`sockets` / `channel`) 每次开页面都并进 `WebInit`; `acdev dev` 的 `devreload` (非热替换) 和页面的 `window.reload` 都先停后端
  再起 (`devreload` 用重新找到的清单), 页面的请求在后端就绪前重试 10 秒; 启动器退出前停掉它并删本进程的 socket 目录. 起停、崩溃重起和日志见
  `docs/web/backend.md`.
- 没有的: `desktops` / `sessions` / `applications` / `open`.
  测试在 `js/launcher/test`.

在 `js/`: `bun install` (只装测试用的 happy-dom、React、CodeMirror 等和 esbuild), `bun test` (`js/dom/test` 和
`js/page/test`, 预加载 `dom/test/gc.ts`), `bun run typecheck`. 原 acde-host 的测试只带来了只涉及页面一侧的那几个.

以下是删除前的说明, 文件名照旧 (acde-dom 即 `js/dom`, acde-host 的页面一侧即 `js/page`); acde-api 一条只留作历史.

- **acde-api**: 入口在 `src/` 顶层 (`index`、`app.d.ts` / `shell.d.ts`、`vite`、`validate.ts`), `src/types/` 是接口声明, `src/modules/` 是 `acde:*` 模块声明, `src/runtime/` 是运行时的 JS. `API_LEVEL` (`src/runtime/level.js`, 契约级别, 包根和 `./manifest` 都导出)、`load(name)` (`src/runtime/load.js`: `import('acde:<name>')` 的默认导出, 拿不到时是 null; 说明符带 `@vite-ignore` 等)、
  `@acde/api/vite` (build 时 `acde:*` 留作外部模块, dev 时换成会 reject 的替身); 声明 `types/` 的 `manifest.d.ts` / `service.d.ts` / `page.d.ts` (加 `events.d.ts`、`office.d.ts`) /
  `config.d.ts` (+ `config.schema.json`) 和 `modules/`; `src/validate.ts` (导出 `./manifest`) 是清单校验和 `ICON_NAMES`, 测试
  `test/validate.test.ts`. acdev 仓库用 `file:../../libacde/acde-api`, acde-dopame 拷一份 (`scripts/sync-acde-api.sh`).
  发布的 SDK 归档不是这个目录: 它的 package.json 是 `"private": true`, 防止把源码发出去. `bun run build` (`scripts/build.mjs`)
  生成 `dist/`, 归档打包的是 `dist/`: `.js` / `.ts` 用 esbuild 压缩 (标识符也压, 没有 source map,
  不留注释), `validate.ts` 的声明由 tsc 生成成 `validate.d.ts`, `./manifest` 改指向 `validate.js` / `validate.d.ts`; 手写的 `.d.ts` 和
  `config.schema.json` 原样拷贝 (对外接口); 去掉 `private` / `scripts` / `devDependencies`; `LICENSE` 是根目录 `LICENSE` 的版权段
  (不含第三方那段). 构建最后检查 `dist/`: 有 `.map`、非 `.d.ts` 的 `.ts`、`sourceMappingURL` / `sourcesContent` 或意料外的文件就失败.
  esbuild 装在 web 根的 devDependencies: 放在 acde-api 自己的 devDependencies 里, bun 会把它装进用 `file:` 引用它的 acde.
- **acde-dom**: 经 `DomNative` / `GpuNative` / `AudioNative` / `HostNative` (`src/native.ts`) 调 src/host, 打进页面包.
  编辑只有 `dom/editing.ts` 一份. 文本框 (`<input>` 文本类型和 `<textarea>`) 各有一份编辑历史 (`dom/history.ts`): 键盘、粘贴、剪切、输入法提交和 execCommand 的改动
  记成一步, 连续打字 (含换行) 和连续的同向删除合成一步, 选区或值在中间变过就另起一步; 快捷键 Ctrl / Cmd+Z 撤销, Ctrl / Cmd+Shift+Z 和 Ctrl / Cmd+Y
  重做, 先发可取消的 `beforeinput`, 再发 `input` (inputType `historyUndo` / `historyRedo`, data 为 null), 恢复值和那一步的选区; execCommand('undo' / 'redo')
  不发 beforeinput, queryCommandEnabled 看有没有可撤销 / 重做的一步. 脚本把值改成别的 (value、defaultValue、setRangeText、表单 reset) 丢掉这份历史,
  赋同样的值不丢 (React 受控组件). 还没有: contenteditable 的撤销, 拖放文字进文本框.
  测试在 happy-dom 的 MockDom 上跑. 布局类的读 (rect、metrics、计算样式) 缓存到下一次写入、下一个指针事件 / 帧 / 滚动, 或这个任务结束 (CSS 过渡和动画在 render 线程上跑, JS 不知道, 下一个任务要重新读). Web Animations
  (`dom/animation.ts`): 关键帧变成 `@keyframes acde-animation-<n>` 规则加元素内联的 `animation-*`, 由 render 跑; 时间、事件、`finished` 在 JS
  里按 host 的时钟; 近似: easing 作用在每段关键帧上、endDelay 只算进结束时间、没有 composite.
  SVG 几何 (`dom/svg.ts`, `dom/svgbox.ts`): `getBBox` 的基本图形和容器按属性在 JS 里算, 文字、图片、use 这类没有几何的子元素取 render
  画出来的框 (`rect` 经 `svg_box`) 换回元素的用户坐标; `SVGTextContentElement` 的 `getComputedTextLength` / `getSubStringLength` /
  `getNumberOfChars` 按 xml:space 默认规则整理文字, 用计算出的字体 `measureText` 量前进宽度, 加上 letter-spacing.
  popover 和模态 dialog (`dom/popover.ts`): `showPopover` / `hidePopover` / `togglePopover` 按 HTML 发 `beforetoggle` (打开时可取消) 和排队的
  `toggle`, auto / hint popover 打开时关掉栈里不是它祖先的 auto popover, manual 不动; 文档上按下 (pointerdown, capture) 关掉不含落点的 auto
  popover, Escape 关最上面的 auto popover 或请求关闭最上面的模态 dialog (`requestClose`); 按钮和 `input` 的 `popovertarget` /
  `popovertargetaction` 在激活时做 (表单里的提交 / 重置按钮仍先提交 / 重置), `popoverTargetElement`; 状态经 `dom.setOpenState` 给 render.
  还没有: popover 的 hint 栈单独管理、焦点回到调用者、`::backdrop`.
  CSS 动画和过渡的事件 (`dom/cssevents.ts`): 页面第一次给这 8 种事件 (或 webkit 前缀的 4 种) 加监听或 on 处理器之后, 每帧 `takeAnimationEvents`
  (render 先把样式算到最新), 在元素上发 `AnimationEvent` / `TransitionEvent` (冒泡, 不可取消, 伪元素的发在宿主元素上带 `pseudoElement`);
  Web Animations 自己的 `acde-animation-<n>` 关键帧不发. 还有动画在跑或刚有事件就再要一帧; 改了 DOM 的任务结束时也要一帧, 好让新开始的动画被看到.
  还没有: 只有 webkit 前缀监听时改发旧名 (DOM 的 legacy event types), `getAnimations()` 里的 CSS 动画.
  用指针选文字 (`dom/textselect.ts`, `Input` 调): 左键按下 (pointerdown / mousedown 都没取消) 在 `caretFromPoint` 处折叠选区, 先发可取消的
  `selectstart`; 按住移动时把焦点端扩到指针处; 第二次按下 (500 ms、5 px 内) 选一个词 (`Intl.Segmenter`, 中文按词典分词), 第三次选所在
  块 (向上找到 `display` 不是行内的祖先), 拖动时按同样的粒度扩展; Shift 按下从原锚点扩展. 按在表单控件、按钮、label、可编辑区域、链接、
  图片、`draggable` 元素或 `user-select: none` 上不动选区.
- **acde-host**: 测试 `bun test` (只测不碰原生接口的模块, 如 `mimedb.ts`). esbuild 打成 `dist/broker.js` (`src/main.ts`, Broker) 和 `dist/web.js` (`src/worker.ts`, 每个 web). release 构建是 `build:release` (出 `dist-release/`). 基准 `bash scripts/release-perf/bench.sh` (Windows, 无头, 每边 5 轮中位数): 加载到 `shell running` 开发 1106 ms、release 941 ms; DOM 和布局负载 95 ms 对 107 ms; 纯 JS 负载 29.3 ms 对 29.4 ms. 多一层字符串数组混淆时加载慢约 470 ms, 所以预设不用它. 打包开着 `keepNames`: 有静态块的类会被 esbuild 改名 (`class _Blob`), 不留名字的话 `Blob.name`、`Object.prototype.toString` 都是错的. 文件按本仓文档的章节命名
  (`broker.ts`、`worker.ts` / `host.ts`、`remote.ts` / `rpc.ts`、`inspect.ts`、`devhot.ts` …); `sys/` 是原生接口的 node 风格包装.
  `broker/` 是从 Broker 拆出的部分, 每块经一个小的宿主接口 (`…Host`) 拿 Broker 的私有状态: `types.ts` (Broker 的接口和窗口记录)、`web.ts`
  (`Web`: 一个 web 的线程和 rpc, 它的授权、环境变量和 403)、`dev.ts` (`DevRequests`: acdev dev 的请求、假回应规则、找 web)、`preferences.ts`
  (配色和减少动态, 含模拟)、`records.ts` (`WindowRecords`: 窗口表、原生窗口的事件、应用的窗口位置)、`approvals.ts` (`Approvals`: 批准的接线, `acde:permissions` / `acde:capabilities` 的调用、给 web 的 `permit`; 决定文件和日志由 Broker 经 `ApprovalsHost` 给)、`listing.ts`
  (`AppListing`: 桌面知道的应用列表, 变了发 `appschanged`); `calls/` 是 web 经 rpc 来的调用: `apps.ts` (`acde.apps` / `acde.packages`)、`windows.ts` (`acde.windows` /
  `acde.launch` / `acde.window`)、`page.ts` (页面调用和钉住的面板)、`services.ts` (交给 web 的服务形状、内置和桌面包服务的调用)、`desktops.ts`
  (`acde.desktops`); `access/` 是谁能用什么: `tier.ts` (按调用者层级的模块)、`family.ts` (打开窗口时的父窗口)、`launchable.ts` (应用能不能
  在这里起: 找不到 404 / 级别不对 409、缺系统服务 503、宿主窗口里要 slots 404、第三方声明外壳模块 403; 列表只给能起的)、`modules.ts`
  (桌面服务 `ctx.import` 拿到的外壳模块和它们的事件)、`capabilities.ts` (必需与可选能力), `access/approval/` 是批准: `permissions.ts` (决定)、`permits.ts` (web 侧的守卫)、
  `dialog.ts` (单应用模式的原生对话框). `broker.ts` 留着会话循环和 web 的起停. `greet/` 是登录页,
  `brand/` 的 `mark.mjs` / `art.mjs` 是 agentos 仓库 `branding/` 的原样拷贝 (只在那边改).

**会话里的应用菜单** (和 acdes 对齐, 2026-11-20): 应用的 `window.setMenus` 经管家推给桌面 (`windowmenus`); 桌面进程的启动器把 `windows.invokeMenu`
交给 `manager.call('windows', 'invokeMenu', args)`, 管家按 pid 给应用进程推 `{type: 'menuinvoke', menu, item}`, 应用进程的启动器照菜单快捷键那条路调页面的 `window.menu(menu, item)`.

**会话里的菜单快捷键** (2026-11-20): 宿主上菜单快捷键经 `shell.shortcut(combo, 'menu', window)` 在合成器登记; 会话里应用进程没有桌面协议, 登记不上.
所以有管家时, 应用的 `window.setMenus` 照旧交给管家, 启动器另外记下菜单的快捷键表 (`AppMenus.remember`), 以 `{type: 'menukeys', keys}` 发给页面线程;
页面的 Host 在 keydown 时先按 `chordMatches` 比对 (`mod` 在 macOS 是 Meta, 别处是 Ctrl), 命中就不再派发给页面, 改发 `{menukey}` 给启动器, 启动器调页面的 `window.menu(menu, item)`.

**重载时交还快捷键** (2026-11-20): 页面经 `windows.shortcut` 登记的会话级和桌面级快捷键都登记在启动器这个进程的 Shell 上, 页面重载 (`window.reload`、开发重载) 时进程和连接都不变,
旧的登记会留着, 新页面再登记同一组合就被 winman 以 "another holds it" 拒掉. 启动器记下本页登记过的快捷键, 重起页面前逐个 `unshortcut` 交还.
