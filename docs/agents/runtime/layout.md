# 目录

| 路径 | 职责 |
| --- | --- |
| `src/host` | `acde` lib: 内嵌 V8 (rusty_v8), 每个 isolate 的事件循环 (`rt.rs`), 模块加载 (`modules.rs`), web 线程 (`web.rs`), DOM 绑定 (`dom.rs`), 注入的能力, 声音, inspector |
| `crates/acompositor` / `crates/window` / `crates/paint` | 会话 (Wayland 合成器 acompositor、会话里 web 的 Wayland 客户端和光栅化) 已搬到 [acdes](http://gitea.dopamine/AC/acdes), 说明在它的 AGENTS.md |
| `src/render` | Blitz 包成的渲染引擎 (`Document` / `Dom` / `RenderHandle`), 窗框 `Decoration`. `Dom` 在 `dom.rs` (结构、加锁读写的辅助、小工具) 和 `dom/`: `types.rs` (`NodeType` / `Ns` / `Viewport` / `Quad`)、`values.rs` (控件种类、数值字符串、`RangeValue`)、`dom_tree.rs` / `dom_view.rs` (`Dom` 的公开方法: 树、属性、文本、选择器 / 表单、几何、样式、图片、样式表、画布)、`engine_tree.rs` / `engine_state.rs` (`Engine` 上对应的实现: 节点与树操作、解析、属性 / 控件值、焦点选区、滚动、图片状态、innerText)、`select.rs`、`semantics.rs` |
| `src/gpu` | 设备层: 共享设备 `Gpu` (包着 src/vello 的 `Shared`; WebGPU / WebGL / video / canvas 图都在它上面), 接口 `Compositor` (前端在发布 `Gpu` 时用 `Gpu::set_compositor` 装上) 交出 `Offscreen` (render 画 canvas、滤镜、阴影、截图用); 平铺导出 `acde-scene` 的 2D 场景 (`Scene` / `PaintScene` / 滤镜)、`Part` / `Pass` / `Content`、`Px` / `Backdrop` / `Shadow` / `Effect`、`GpuImage` |
| `src/vello` | wgpu 后端: vello 画场景 (`Vello`), 合成 (`Composer`, `compose.wgsl`), 离屏 (`Offscreen`: canvas、滤镜、阴影, `blur.wgsl` / `matrix.wgsl`), `GpuImage` / `ImageSet` / `Shared`, `VelloCompositor`; Linux 上 dma-buf 画布导入和 sync_file (`Device`, 会话的 `Raster`). acdev runtime 和会话用它. detour 后端以后放 `src/detour` (feature `detour`) |
| `scene/` | `acde-scene`: 2D 场景 (`Scene` / `PaintScene` / `RenderCommand`)、滤镜 (`Filter` / `FilterEffect`)、合成的部件 (`Part` / `Pass` / `Content` / `Px` / `Backdrop` / `Shadow` / `Effect`, unix 上 `Dmabuf` / `Submitted`). blitz fork、本库和后端共用, 只依赖 kurbo / peniko; 发到 ac |
| `wire/` | `acde-wire` (不发到 ac): 会话管家控制套接字的帧格式 (`channel`: JSON 帧、SCM_RIGHTS 描述符、超过 `FRAME_MAX` 经 memfd 的大帧、`Frame` / `Refusal` / `answer` / `event`、接口级别), 原样搬自 acdes `crates/manager/src/wire.rs`; feature `acompositor` 由下面 `protocol/acompositor/` 的 XML 生成 wayland-client 绑定 (`desktop`: 窗口表、`capture`、`window`; `inject`). acdes (管家、会话、合成器) 和 acdev 的客户机代理 `acdev-agent` 都用这一份; 只依赖 rustix 与 serde_json, 开 `acompositor` 才带 wayland-client |
| `dirs/` | `acde-dirs`: XDG 用户目录 (`Place`: desktop、documents、downloads、music、pictures、videos 与变量名、缺省英文名的对照) 和 `~/.config/user-dirs.dirs` 的读法 (`listed`、`$HOME` 展开的 `expanded`、Unix 上的取法 `user_dir`: 列的值是绝对路径且不是主目录本身才用, 否则用缺省名). 本库 `runtime/fs/workspace/userdirs.rs` 与 acdes 的管家 (给受限应用挂公共位置)、会话 (截图的默认目录) 都用这一份, 不另写; 只用标准库, 发到 ac |
| `protocol/acompositor/` | acompositor 的私有协议 `acompositor-desktop-v1.xml`、`acompositor-inject-v1.xml`, 合成器 (acdes, 服务端) 与客户端 (acdes 会话、acdev 代理, 经 `acde-wire`) 共用这一份. **协议的主人是 acdes**: 放在这里只因为两个仓都要用 (只此一份), 改协议要 acdes 一方同意, 改动和 acdes 的实现同一波推 |
| `src/webgl` | `acde-webgl`: wgpu 上的 WebGL 1 / 2 (见 `docs/agents/graphics/webgl.md`「WebGL」). 检查: `cargo test --release -p libacde webgl` |
| `src/runtime` | 系统接口 (napi 导出 + 注入用的 Rust 对象). 异步执行器 `rt.rs`: `acde-rt` 线程上的 ntex System (不用 tokio, TLS 用 rustls + ring (不用 aws-lc: Windows 要 NASM, 每个平台都多编一个 C 库; 用户定), 证书校验用 rustls-platform-verifier). 检查: `cargo test -p libacde runtime` (含真 pty / Windows 伪控制台上的终端、unix socket 上的 HTTP / WebSocket)、`ime-check.sh`、`portal-check.sh`、`atspi-check.sh` (经 `tests/runtime/run.sh`) |
| `src/model` | canvas / 窗口表、计划、快捷键、shell 事件 (`Event` / `Inbox`) |
| `src/log` | `acde-log`: 日志和计时 |
| `src/media` | `acde-media`: 解码音视频, 读摄像头和麦克风 (Linux GStreamer, macOS AVFoundation; 见 `docs/agents/web/web-apps.md`「视频」「摄像头和麦克风」). 检查: `cargo test --release -p libacde media` (`tests/fixtures` 是 ffmpeg 命令行生成的 1 秒小片段; `tests/media_capture.rs` 用假设备; `tests/media_devices.rs` 里 `--ignored` 的列出 / 读真设备、在 macOS 上问权限) |
| `src/storage` | `acde-storage`: 网页存储的分区和 SQLite (见 `docs/agents/web/storage.md`「网页存储」). 检查: `cargo test --release -p libacde storage` |
| `src/napi` | rusty_v8 实现的 Node-API (napi8), 让 `#[napi]` 导出在 Broker 里可用. 改自 Deno `ext/napi` 0.191.0 (MIT, LICENSE-MIT), 去掉了用不到的入口 |
| `js/dom` | 网页接口层: web 里的浏览器全局对象和 DOM (原 acde-dom) |
| `js/page` | 页面一侧的胶水: 在 web 里起页面、`acde:` 模块、和启动方的 rpc、输入与窗框 (原 acde-host 的 `worker.ts` 及其依赖) |
| `js/launcher` | 启动器: 一个进程开一个应用一个窗口, 或 (没有管家时) 一个桌面连同它的应用和服务 (临时代替原管理中枢, 见下). `src/` 下按职责分: 根上是入口 `main.ts`、`args.ts` 和窗口系统的类型 `shell.ts`; `local/` 宿主上的本地会话; `app/` 找应用、拼页面初始化数据、本进程回答的调用 (`calls.ts`) 和事件 (`events.ts`); `manager/` 与管家的连接和服务模式; `dev/` 调试通道、DevTools、语义树与操作; `backend/` 应用后端 |

## js: 运行时自带的 JS

网页接口层和页面胶水在 `js/` (原 `web/` 删除后从 git 历史 91e7dd1e^ 取回; "不出现 JS" 只针对 SDK), 是运行时的内部组件: 不发布、不进 SDK,
也不再有单独的 `web.js` 文件. `build.rs` 用 bun (`ACDEV_BUN`, 默认 `bun`) 把 `js/page/src/worker.ts` 打成一个 ES 模块
(`$OUT_DIR/page.js`, 压缩空白和语法、保留名字), 契约级别取 `acde-api/api-level`、最低支持级别取 `acde-api/min-api-level`、版本取包版本 (`ACDEV_API_LEVEL`、
`ACDEV_MIN_API_LEVEL`、`ACDEV_RUNTIME_VERSION` 三个 define), `src/host/web.rs` 用 `include_str!` 嵌进二进制, web 线程从 `acde-runtime:/page.js` 加载它
(`WebLoader` 认这个地址); 同样把 `js/page/src/webworker.ts` 打成 `webworker.js`, Web Worker 的线程从 `acde-runtime:/webworker.js` 加载它
(它只引 acde-dom 的 `core/` 和 `worker/`, 不带 DOM). 所以构建运行时库要有 bun. 打包命令只在 `js/bundle.ts` 一处 (`bun bundle.ts [输出目录] [版本]`, 默认 `js/dist`, `build.rs` 也调它).
**名字不能被打包改掉**: 一个包里两个同名的顶层符号, bun 把后一个改名 (`URL2`), 而 bun 1.4.4 的 `--keep-names` 对这种改名不补 `name`,
页面里 `URL.name` 就成了 `"URL2"`, WPT 和 idlharness 按名字认接口 (11-22 `DOMException2` 一次掉了约 160 个子测试). 测试 (`dom/test/harness.ts`) 用 esbuild 的 `keepNames` 打包,
看不出这个问题, 所以 `bundle.ts` 打完每个包都查一遍: 包里有 `class X<数字>`, 而源码里声明了 `X`、没声明 `X<数字>`, 就报错退出
(源码指 `js/dom/src`、`js/page/src`、`js/launcher/src` 和 `manifest/` 下的全部文件, 直接扫目录, 不用 `bun build --metafile`: 执行机和 Windows 上的 bun 版本不一); 函数只查暴露出去的
(包里有 `{X:X<数字>,…}`, 比如全局表里的 `atob`), 内部辅助函数被改名无所谓. 触发改名的三种写法和改法:
页面胶水、启动器里裸写全局 (`new URL(…)`、`atob(…)`, 指的是装好的全局) 写成 `globalThis.URL`; 只在类型位置用的值导入碰上循环导入 (`custom.ts` ↔ `html.ts`
的 `HTMLElement`, 删掉的代码也算) 改成 `type` 导入; 两个模块声明同名的类 (Broker 的批准表原来也叫 `Permissions`) 改掉其中内部的那个.
被摇掉的代码也会触发: `manifest/level.ts` 在没有 define 时才读 `acde-api/` 的文件, 用 `process.getBuiltinModule('node:fs')` 现取, 不静态导入
`node:fs` / `node:path` (bun 对浏览器目标保留没用到的 `node:` 导入, 曾把它的 `node:path` 实现整个打进运行时, 和 `js/page/src/sys/path.ts` 撞名).
调试版运行时设了 `ACDEV_JS_DIR` 时从那个目录读 `page.js`、`webworker.js`、`launcher.js`, 读不到才用内嵌的 (发布版只用内嵌的): 改 `js/` 后
`cd js && bun run bundle` 一两秒出新包, 重开页面即生效, 不重编 Rust; 进客户机时只推这几个文件. `js/tsconfig.json` 的 paths 把 `@acde/dom` 指向 `js/dom/src`,
`@acde/api` 指向 SDK 的声明 `acde-api/src`, `@acde/api/manifest` 指向清单模块 `manifest/`
(acdev 的命令行用同一份清单规则, 见 `docs/agents/acde-api.md`); 窗框主题的解析 (`FrameThemes`) 只有页面用, 在 `js/page/src/frametheme.ts`. 页面和启动方之间的类型
(`WebInit`、服务形状、菜单、权限等级) 在 `js/page/src/contract.ts`; 管理中枢 (原 `broker.js`) 没有取回: 管家部分归 acdes.

单应用启动器 (`js/launcher`, 用户定的临时做法) 和页面包一样由
`build.rs` 打成 `$OUT_DIR/launcher.js` 嵌进二进制 (`src/host/bundle.rs` 管这两个地址), 原来进管理中枢的入口 (`Entry::Broker`) 现在都从
`acde-runtime:/launcher.js` 起. 它认 `app <id|目录>` 和 `desktop [id]`, 都可带 `[--size WxH] [--scale N] [--seconds N] [--shot 文件]
[--shot-after 毫秒]`, 以及只在有管家时可用的 `service <名字>`. 有管家时一个进程一个页面 (一个应用一个窗口, 或一个桌面), 或一个后台服务;
没有管家时 `desktop` 是本地会话 (见下), 一个进程里跑桌面、它打开的应用和桌面服务.

- **有管家时** (会话里, 环境里有 `ACDE_MANAGER_FD` 且 `native.Manager` 在, acdes 的管家,
  协议见 acdes `docs/agents/manager.md`): `native.Manager.connect()` 拿 welcome, 应用的清单、包目录、资源、等级 (`tier`)、授予 (`grants`)、
  拉起时的请求 (`request`) 都取自它; 窗口大小按欢迎消息顶层的 `size` (`[宽, 高]`, 管家按 `apps.open` 的 `size` 给, 1–16384 的整数) 或 `outerSize` (含标题栏的外框, 逻辑像素, 和
  `size` 二选一; 原样交给 `grant({outerSize})`, 窗口客户端扣标题栏; 只在欢迎消息这条内部路径上, 页面的 `apps.open` 没有这个选项) → 清单 `size` →
  窗口系统默认 (800x600) 取第一个有的 (`windowGrant` 和 `windowSize`, 形状不对的欢迎消息值不用); 桌面的页面是 `welcome.dir` 加 `welcome.desktop.entry`. 子进程的环境里不再带 `ACDE_MANAGER_FD`.
  页面的调用除下面留在本进程的以外原样转给管家 (`manager.call`), 失败的 `Error` 带 `status`、`code` 抛回页面
  (`native.Manager.call` 当场抛的错, 如参数转不成 JSON, 经 `rejectingCalls` 也变成这次调用的失败, 不会让页面或服务一直等). 交给管家之前参数按 JS 习惯
  处理一次 (`nativeArgs`, 用户选 A): 末尾为 undefined 的参数去掉, 普通对象里值为 undefined 的字段逐层省略 (同 `JSON.stringify`), 中间位置
  或数组里的 undefined 让这次调用以 `TypeError` 失败 (`lock.unlock: argument 2 is undefined; leave it out or pass null`); 服务交给管家的应答和回调的参数
  也一样 (`service.ts`): 应答里省不掉的 undefined 让调用方收到 500 `internal` (`desk.list: the answer.desks[0] is undefined; …`), 回调参数的在服务代码里抛 `TypeError`;
  页面与启动器之间的 rpc 照旧原样传; `window.ready`、`page.shown`
  也转过去 (管家据此记冷启动). 管家推来的 `request` 交给页面 `window.request`, 桌面进程收到的其余推送原样交给桌面页, `managerclosed` 时以 0 退出.
- **没有管家时** (宿主窗口跑法): `app` 是单个应用 (这一条), `desktop` 是本地会话 (下一条); 目录取其 package.json 里第一个应用, id 在 `ACDE_PATH` 的各目录及其下一层找; 资源前缀按
  `acde.assets`, 没有 `/` 时 `/` 指包目录; 没有服务、最低权限等级、存储在 `$ACDE_STORAGE` 或 `~/.local/share/acde/storage` 的 `app-<id>`.
  文件 (用户定): 清单声明了 `fs` 就授予 `{places, paths: false}` (形状同客户机管家的 `grants.fs`): 只在设了 `ACDE_DEV_PLACES` (`acdev dev` 设,
  它同时用 `XDG_<名>_DIR` 把公共位置换成会话自己的隔离目录) 时给清单 `fs.roots` 列的位置, 否则只有应用私有目录; 不给任意路径.
- **本地会话** (`desktop [id]` 没有管家时, `js/launcher/src/local/`, AC/acdev#110, 用户选「本地开发会话管家」): `acdev dev` 带桌面、
  几个工程一起跑或桌面模板都走这里. 桌面、它打开的每个应用和桌面的服务都在这一个进程里, 共用一个宿主窗口; 每一方拿一条和 acdes 管家同形的
  `ManagerLink` (`local/link.ts`), 所以启动器走的是设备上的那几条路, 只是管家在本进程:
  - 登记 (`local/registry.ts`): 包的根照 acdes `Registry::roots`: `ACDE_PATH` 的各目录 (`dev-<序号>`), `XDG_DATA_DIRS` (默认 `/usr/local/share:/usr/share`)
    各目录下的 `acde/packages` (`system-<序号>`), `$XDG_DATA_HOME/acde/packages` (`user`; `acdev install` 和文档里装 dopame 桌面包都放这里); 根本身或它下一层
    有 package.json 的是包 (下一层按名字排序). 只有桌面 (给了 id 按 id, 没给取第一个) 所在的包是系统包, 应用等级 `system`, 别的 `third-party`: acdes 把
    系统根里归 root、只有 root 能写的包也算系统包, 启动器的文件接口看不到属主, 宿主上就不认 (一个谁都能写的 `/usr/local/share/acde/packages` 不该给出 terminal 等授予);
    同一个应用 id 只认第一个 (别的打一行 `acdev: …`); 桌面清单 `services` 列的、包里 `services` 有模块的服务才起.
    桌面的资源每个根多一条 `/acde-packages/<根名>/`; `apps.list` 里包内的图标文件 (`/…`) 换成这个前缀下的地址, 同 acdes.
  - 欢迎消息照 acdes `base/welcome.rs` 拼: 桌面 `tier.shell` 是全部外壳模块、`session: {own: true}`; 应用的 `tier.shell` 是系统应用声明了的外壳模块;
    服务的 `module`、`modules`、`stateDir` (`$XDG_STATE_HOME/acde/<包名>/<服务名>`, 建好、700). 授予按宿主窗口的规矩, 比会话里紧: 文件只给
    `acdev dev` 的隔离位置 (`ACDE_DEV_PLACES` 时; 桌面给全部公共位置), `paths` 一律 false; 第三方应用不给 terminal、system、bluetooth、devices、power、
    input、display、office (同单应用), 桌面和系统应用照声明给.
  - 调用的准入照 acdes `access.rs`: 外壳模块只给桌面和声明了它的系统应用 (`session`、`desktops` 先过这一关); `openPath` / `handlers` 要 `openFiles`;
    `capabilities.request` 只问声明过的; `apps.open` 点名要 `open` 列着它 (带 `open-link` 请求也一样), 按类型开要 `openFiles`; `inspect` / `debug` / `dev`
    只给桌面; 别的名字是桌面服务 (应用要在清单 `services` 列出那个服务, 否则 403, 没起回 503). 被拒的形状同 acdes (`status`、`code`).
  - 本地会话自己回答: `apps.list`、`apps.open` (在跑的单例应用提到前面并收到请求 `{type: 'request', request, lent: []}`, 回 `running: true`;
    `newWindow` 只对 `multiple` 的应用开新的; 新起的回 `{kind: 'acde', id, app, window, pid, instance}`, 实例号逐应用递增; 请求要有清单 `requests` 里的
    `kind`, 否则 400 `untyped_request` / 409 `not_accepted`; `open-link` 按 scheme 找应用), `window.setTitle` / `setMenus` (告诉桌面
    `windowtitle` / `windowmenus`), `window.openUrl`、`page.notify` (桌面收 `notification`)、`windows.screenshot` (整个宿主窗口)、
    `windows.invokeMenu` (按窗口号给应用 `menuinvoke`)、`capabilities.request`、`permissions.list` / `pending` (空)、`desktops.list`、`session.others` (空)、
    `session.logout` (结束会话); 别的回 501 `unsupported`.
  - 按类型打开 (`local/open.ts`, 照 acdes `mime.rs` / `apps/defaults.rs`): 类型是文件夹 `inode/directory`, 文件先按 shared-mime-info 的 `mime/globs2`
    (`$XDG_DATA_HOME`、`XDG_DATA_DIRS`, macOS 另加 `/opt/homebrew/share`、`/opt/local/share`; 权重高的、模式长的先, `cs` 才分大小写), 没有就按和 acdes 同一张扩展名表,
    再没有是 `application/octet-stream`. 选谁: 记住的 (`$XDG_CONFIG_HOME/acde-mimeapps.list`、`mimeapps.list`、`XDG_CONFIG_DIRS`、各 `applications/mimeapps.list`;
    `acde-<应用>.desktop` 指 acde 应用) → 清单 `mimeTypes` 列了它 (或 `type/*`) 的第一个应用 → 原生程序 (`applications/` 下列了这个类型的 `.desktop`, `gio launch` 打开,
    只给桌面和系统应用) → 只给桌面和系统应用: macOS / Windows 交给系统默认 (`Shell.openDefault`, 回 `{kind: 'native', id: null}`) → 404 `no_app`.
    交给 acde 应用时先 `lendPaths` 把文件或文件夹借给它 (进程里的借出表, 同单应用的 `acdev open`), 请求是 `{kind: 'open-file', file, entry}` (acdes 的形状);
    启动器坐在本地会话里时请求原样交给页面, 不走 `Lending.entered` (那是收管家的文件描述符用的). `apps.openPath(path, {app?})` (`app` 可以是应用 id 或 `.desktop`)、
    `apps.open({kind: 'open-file', path})`、`apps.handlers(path)` (`{mime, association, handlers: [{kind, id, label, icon, default, system?}]}`)、
    `apps.setDefault(mime, 应用 id 或 .desktop)` (写 `acde-mimeapps.list`)、调试通道 `open {path, name?}` (绝对路径且存在) 都走这里.
    宿主上比 acdes 紧 (2026-11-24 的安全审查): 页面 (桌面和应用) 交来的路径解析软链后必须在 `ACDE_DEV_PLACES` (acdev dev 的隔离位置) 里, 或在借给这个调用方的文件夹里,
    否则 403; 只有调试通道 (开发者自己的 `acdev open`) 不受限. 带 `open-file` 请求的 `apps.open` 不论点没点名都要 `openFiles`.
  - 借文件夹 (`apps.open(id, request, {root})`, 只给桌面, 应用要声明 `fs`, 否则 403 / 409): `lendPaths` 借给它, 请求带 `root: <借出名>`, 再给它的启动器推 `lend`,
    启动器给页面发 `fsroots` (加一个根); 在跑的应用同样先借再交请求. `apps.unlend(dir)` 给借过这个文件夹的应用推 `unlend` (启动器 `unlendRoot` 并发 `fsroots` 去掉), 回 `{revoked}`.
  - 应用开在桌面之上的 `free` 窗口里 (`grant({mode: 'free', size?})`, 尺寸同上), 起来后告诉桌面 `windowopen {window, appId, title, web, instance}`,
    退出时放掉它的窗口并告诉 `windowclose`; 页面名 `<应用 id>#<实例号>`. 页面重起 (重载、重建后的 `devreload`) 时启动器带着原来的窗口号再要窗口,
    会话把宿主窗口删掉旧表项时发的 `windowgone` (正常状态的位置; 不交给桌面, acdes 不发这个) 或最后一次正常状态的 `windowgeometry` 作 `rect` 给它,
    所以窗口号、位置都不变 (要之前先把宿主窗口积压的事件取完). 位置、状态、焦点的变化由宿主窗口自己发 (`windowgeometry`、`windowstate`、
    `windowactivate`), 启动器折成 `window` 快照交给桌面 (下面「窗口」).
  - 服务: 先起服务 (`ServiceHost`, 本进程 `import()` 它的模块), 至多等 5 秒它们 `ready`, 再起桌面; 服务提供的命名空间 (保留名和系统能力名丢掉, 同 acdes)
    进桌面和列了它的应用的 `services`, 变了推 `services`. 调用转成 `{type: 'call', id, service, method, args, caller}` 给服务, 30 秒没回答回 504;
    回调参数 (`__acdeFn`) 换成会话里的全局号, 服务的 `invoke` 换回调用方自己的号, `release` 两头转, 同 acdes `desktop/services.rs`. 服务结束时等它回答的
    调用回 503. 桌面包重建后的 `devreload` (没有 `app`): 先关掉所有服务 (`managerclosed`, 照常 `close`、结束它起的进程), 重扫 `ACDE_PATH`, 再按新清单起服务,
    模块地址带 `?generation=<n>` 让 `import()` 不用缓存里的旧模块 (acdev 把服务打成单文件; 别的多文件服务里的相对导入仍是旧的), 然后才重载桌面.
  - 宿主窗口的事件 (`local/hub.ts`): 一个 `Shell`, 每个启动器一个视图, 视图授予的窗口记在它名下. 带窗口号的 `windowcloserequest`、`menuselect`、
    `devtools` 和作用域是 `app` / `menu` 的 `shortcut` 给窗口的主人, `devrequest` / `devreload` / `quit` 由会话处理, 其余给桌面.
  - 调试通道: `webs` 问每个页面再合并 (每个限 1.5 秒, 不答的列 `stalled: true`); 带 `web` 的 (页面名、应用 id 取实例号最小的、`desktop`) 交给那个页面的启动器,
    不带的给桌面, 不带 `web` 的 `snapshot` 给拿着焦点的窗口的主人; `open {name|url}` 会话自己答 (`path` 回 501); `console` / `network` 带 `pages: true`
    时每个页面各答一份 (键 `<应用 id 或 desktop>:<pid>#<序号>`). 各启动器的 Inspect 经会话看到所有窗口 (`Seat.windows`: 页面的 rpc、记录、关窗、
    统一的 `w<n>` 编号), 所以 `hit`、`act`、`snapshot` 落在别的应用的窗口上也能做. `devreload {app}` 先重扫 `ACDE_PATH`, 再交给那个应用的启动器 (它按新清单重起页面),
    没有 `app` 时先重起服务 (见上) 再给桌面.
  - DevTools: 进程里只有一个端点 (`DevTools.targets` 每次整张替换), 所以每个启动器拿到的是会话包过的 `NativeDevTools` (`Seat.devtools`): 各自登记自己页面的目标,
    会话合起来交给端点 (`/json/list` 同时有桌面和各应用; 页面结束时它的目标去掉), 端口同一个.
  - SIGTERM / SIGINT / SIGHUP、`quit`、`session.logout`、`--seconds` 或桌面退出结束会话: 先关所有应用, 再断服务和桌面的连接; `--shot` 截整个宿主窗口.
- **锁屏页预览** (`lock <桌面 id|目录>`, 只在没有管家时, `acdev dev --lock` 用): 按 `ACDE_PATH` 或目录找桌面包, 打开它清单 `lock`
  写的页面 (桌面窗口, 等级最低, 授予全无, 页面初始化数据带 `lock: true`, 所以只有 `acde:lock` 和 `acde:services`). `lock.user` / `lock.unlock` / `lock.cancel` 由
  `js/launcher/src/app/lock.ts` 的 `FakeLock` 回答: 固定用户 (头像 null); `cancel` 直接成功; `right-password` 解锁 (第二个参数不看), `wrong-password` auth (403)、`busy-password` busy (409)、
  `throttled-password` throttled (429, `retryAfter` 3), 别的 auth. 有管家时 `lock` 命令报错, `lock.*` 照常交给管家.
  `window.ready`、`page.shown` 直接应答, `desktops.list` 回 `{desktops: [], current: ''}`, 其余 (含 `session.*`、
  `desktops.choose`、`lock.*`) 回 `unsupported` (501).
  `window.setTitle` 记下标题 (语义树、`webs`、DevTools 目标用它); `window.setMenus` 交给宿主窗口 (`Shell.setMenus`, 菜单画在框上), 带 `shortcut`
  的菜单项注册成 `menu` 范围的快捷键, 宿主推来的 `menuselect` 和按下的菜单快捷键转成页面的 `window.menu` (`app/menus.ts`); `page.open` / `page.save`
  (页面的打开、保存文件选择器) 用 `sys.pickFiles` / `sys.pickSave` (见 `docs/agents/web/fs.md`「打开和保存文件」), `page.notify` 用 `Shell.notify` (`app/page.ts`); `page.download` 仍回 501.
- **密钥输入框**: `type=password` 的 `<input>`, 以及 `autocomplete` 里有 `current-password` / `new-password` /
  `one-time-code` 的 `<input>` / `<textarea>`, 算密钥输入框 (`TextEntry.secret`, `js/page/src/text.ts`): 所有页面里都不启用输入法 (`InputMethod.focus`
  不发 `focusIn`, 即不发 text-input 的 enable; 从普通输入框切过来时发 `focusOut`), 没有预编辑和候选面板, 也不能复制、剪切出去; 普通输入框不变.
- **会话与桌面包** (`acde:session`、`acde:desktops`, libacde 工单 04): 页面胶水 (`RemoteBridge`) 给桌面页定义这两个模块, 应用页在
  `tier.shell` 含 `session` / `desktops` 时定义. `session.logout/reboot/poweroff/setLanguage(tag)`、`desktops.list()` (回
  `{desktops: DesktopInfo[], current}`)、`desktops.choose(id, remember?)` 经 rpc 转出, 有管家时原样 `manager.call`. 同步的
  `session.lock()` 同样经 rpc 转出 (管家调 logind 的 Lock). `session.own()` 取 `WebInit.session.own`: 启动器把 welcome 顶层的 `session: {own}` 带进去, 没有这一项或没有管家时为 false. `session.others()`  回别的用户还开着的会话 `[{user, uid, place, remote}]` (SDK 类型 `SessionOther`, `place` 是座位或终端, 可为 null), 关机、重启前先问它; `session.switchUser()` 锁定本会话并切到登录页去换人 (登录页在别的终端上按需起, 原会话留在后台可切回). 两者都由管家 (acdes) 实现, 页面胶水原样转发; 旧版管家以 RPC 错误拒绝, 页面要 catch.
  管家 (acdes) 推来的服务里若也有 `session` / `desktops`, 会盖掉这两个模块; acdes 在本工单后去掉登录模式里的这条绕法.
- **锁屏页** (`acde:lock`; 管家一侧见 acdes `docs/agents/manager.md`「锁屏」): 管家以锁屏模式起的进程收到的 welcome 是桌面的形状
  (`kind: 'desktop'`) 加顶层 `lock: true`, 启动器打开 `join(dir, desktop.lock ?? desktop.entry)` (桌面没写 `lock` 时管家给 acdes 自带的锁屏包),
  页面初始化数据带 `lock: true`. 页面胶水对锁屏页只定义 `acde:lock` 和 `acde:services`, 不给外壳模块; 锁屏页的电源菜单用的 `acde:session` 来自管家在 welcome 的 `services` 里给的 `session` 服务 (acdes 只列 `poweroff`、`reboot`、`suspend`, 系统确认页不给), 同登录页的做法.
  `lock.user()` 回 `{name, fullName, avatar}` (`avatar` 是 `data:` URL 或 null), `lock.unlock(password)` 失败时的错误带 `code`: `auth` (403,
  密码错)、`busy` (409, 上一次验证还没完)、`throttled` (429, 失败后的延迟期内, 另带 `retryAfter` 秒数); 这些字段由 rpc 原样带回页面
  (SDK 类型 `LockError`). `lock.status()`  回 `{battery, network, pending}` (SDK 类型 `LockStatus`: 电池 `{percent, charging}` (`percent` 是 0 到 100 的整数, 管家取整)、网络 `{online, kind}`、等用户处理的件数, 不知道的为 null), 锁屏页拿来显示网络、电量和件数; 锁屏页自己发起的关机、重启要系统确认时, 页面经 `lock.answer({allow})` 回答 (acdes 的关机确认 power-admin; 登录页走管家服务的 `greeter.answer`, 不经胶水); 胶水总会转出 `status`、`cancel` 与 `answer`, 不支持它们的管家 (旧版 acdes, 或锁屏页调 `cancel`) 以 RPC 错误拒绝, 页面要 catch, 不能靠方法在不在判断; `lock.user()` 的应答另带起页那一刻的 `status` 与 `render` (`'gpu'` 或 `'cpu'`, CPU 画法下页面不用模糊). 这些都由管家定, 页面胶水原样转发. 管家按调用方拒绝: 锁屏页只能调 `lock.*`、`session` 的电源方法、`window.ready`、`page.shown`.
  系统确认页 (管家以确认模式开同一个锁屏进程, 欢迎消息和 `lock.user()` 的应答多一项 `confirm`, 见 acdes `docs/agents/manager.md`) 另有两处,
  都只是加法: `lock.unlock(password, {remember?})` 的第二个参数 (`'once' | 'task' | 'scope' | 'until'`, 这次批准记多久, 原样转给管家;
  确认页不要密码时 `password` 传空串), 和 `lock.cancel()` (拒绝, 管家立刻回成功, 随后会话进程让页面放开锁屏并退出). 页面胶水按原样转发参数,
  不看 `confirm` 的内容; 它的形状是 SDK 的 `LockConfirm` (`{id, agent, waiting: {title, detail, expires, password, judge, remember?: {may, default, max_days?},
  tries_left?}}`, `waiting` 里照系统总管协议用下划线名), 只在确认页里有, 锁屏页没有.
- **服务模式** (`service <名字>`, `js/launcher/src/manager/service.ts`):
  不开窗口, 在启动方的 isolate 里 `import` welcome 的 `module` (绝对路径), 用 ServiceContext (`name`、`package` 即包目录、`state` 即管家建好的
  `stateDir`、`env`、`log`、`spawn`, `import('acde:<模块>')` 只放行 welcome 的 `modules`, 其方法经管家调用; `acde:voice` 例外, 在服务进程里直接调 napi `Voice`;
  服务拿不到 napi 导出本身, 2026-11-20 起没有 `native`) 调它的默认导出;
  起好后向管家发 `{ready: {provides: {命名空间: {methods, values, sync, callable}}}}` (形状同旧管理中枢). 管家转来的调用
  (`{type: 'call', id, service, method, args}`, `method` 为空串时调命名空间本身) 里的 `{__acdeFn: g}` 换成回调, 回调发
  `{invoke: {fn: g, args}}`; 收到 `release` 后这个回调不再发. 服务代码要知道调用方放手了 (如关窗后清掉订阅) 时用
  `ctx.releaseSignal(回调)`: 给这个回调一个标准 `AbortSignal`, 同一个编号的回调共用一个, 收到它的 `release` (页面释放或断开时管家发)
  就 abort, 已释放的直接给已 abort 的; 传的不是调用方给的回调时抛 `TypeError`. 应答 `{id, ok}` 或 `{id, error: {status, code, message}}`.
  `managerclosed` 时调 `close()`、停掉它起的子进程 (最多 3 秒) 后以 0 退出. 从开始答调用到 `managerclosed` 之间挂着一个计时器保活
  (`holdTheLoop`): 服务模块里没有别的挂起任务时, 事件循环也不会空了就结束 (2026-11-18 大联测里 5 个服务 ready 后十几秒内自己退出过). 需要 system 的 `native.Manager` 提供 `send(帧)`, 并把转来的调用和
  `release` 从 `pump()` 交出 (`{type: 'call', ...}`、`{type: 'release', fn}`, 也认不带 `type` 的原帧).
  **连桌面后台** (系统代办连接, 用户定): 桌面清单里写了 `backend` 角色的服务,
  welcome 带 `backend: {role, up}`, ServiceContext 就有 `backend` (`BackendState`; 没有角色时 `ctx.backend` 是 undefined): `role`、`up` (初值取
  welcome, 之后随管家推的 `{type: 'backend', up}` 变), `onChange(回调)` 只在 `up` 真变了时回调 (回调抛错只记日志), 返回取消订阅的函数;
  `connect()` 转给 `native.Manager` 的 `connectBackend()`, 由管家代为连上后台服务, 得到 `BackendConnection` (`role`、`write`、`onData` 原样的字节块、
  `onClose`、`close`), 后台服务没起来时以 503 `service_not_running` 拒绝, 服务没有角色时 403 `forbidden`; `native.Manager` 没有
  `connectBackend` 时以 501 `unsupported` 拒绝..
  原来的令牌交接已过时 (R3): `backendToken()` 永远回 null, `onBackendToken()` 永不回调 (返回的取消函数照常可调), 下个大版本删.
- **应用与桌面里的服务**: welcome 的 `services` (命名空间到形状) 进 `WebInit.services`, 页面据此生成代理; 管家推 `{type: 'services', services}`
  时整份交给页面替换代理 (`RemoteBridge.replaceServices`). 页面对这些命名空间的调用不经启动方的 rpc 解开, 转给
  `manager.call`, 结果或错误直接回给页面; 管家推来的 `{type: 'invoke', fn, args}` 转成页面 rpc 的 `invoke`.
  回调编号换号 (`js/launcher/src/manager/lent.ts` `LentCallbacks`): 页面的 rpc 每次加载都从 1 编号, 管家按 "连接 + 编号" 登记借出的回调,
  重载后新旧页面撞号, 服务调旧回调会落到新页面里同编号的别的函数上 (总管推送收到怪值). 所以启动器转发前把参数里的 `{__acdeFn: n}`
  换成本进程内只增不复用的编号 (同一次加载里同一个 n 换成同一个号, 管家只登记一份), 管家推来的 `invoke` 按这张表换回页面的编号, 表里没有的
  (上一次加载的) 丢掉; 重载 (`#respawn`, `reload`、开发重载都经过它) 时把表里的号全部向管家发 `{release: {fn}}` 再清表, 服务那边对应的回调作废、
  `releaseSignal` abort. 页面自己发的 `{rpc: 'release', fn}` 是它放掉启动器借给它的函数, 照旧由启动器的 rpc 收, 不转管家.
  调用追踪 (AC/acdev#115): `LentCallbacks` 带加载代次 (`load`, 从 1 起, 每次 `releaseAll` 加 1),
  释放时把号和它所属的代次记进一张短表 (最近 `RELEASED_KEPT` = 1024 个). 服务调用经 `LentCallbacks.call` 转出, `acde_host::calls` 开到 debug 时每次调用记一行
  `event=call load= service= method= lent=[号,…] ok= ms=`, 每次送达页面的回调记 `event=invoke load= fn= page=`. 管家推来的 invoke 查不到号时不再静默丢弃:
  记一行 `event=stale-invoke code=<stale-load|unknown-callback> fn= lent-by=<借出时的代次|none> load=<现在的代次>` (开发模式下 warn, 正式系统 debug); 开发模式 (`ACDE_DEV`) 下另给页面发
  `{rpc: 'violation', code, text}`, 页面的 rpc 把 `text` 写到自己的 `console.error`, 于是 `acdev console` 当场看得到 (StrictMode 式的当场报错). 违规种类只有这两个固定编号, 测试可以断言.
- **服务列表变化** (`acde:services`, 应用与桌面都有, 不用声明): `names()` 给当前的命名空间, `on('changed', ({names, added, removed}) => …)`
  在推来的列表 (含形状) 和上一份不同时通知. 通知前新命名空间已能 `import('acde:<名字>')`, 形状变了的重新 import 拿新代理, 去掉的再 import
  会被拒 (`modules.rs` 的 `define` / `refuse` 会丢掉缓存的模块; 已拿到手的旧代理仍在, 调用由管家报错). 桌面先于服务起来时 (客户机上服务要
  几十秒), 页面在 `changed` 里重新 import 并初始化, 不用轮询. 命名空间 `services` 留给这个模块, 服务提供同名命名空间会被忽略.
- **两种都留在本进程的调用** (`js/launcher/src/app/calls.ts` 的 `route`): `window.close` 和以自己窗口号调的 `windows.close` 关掉窗口 (结束页面线程,
  进程随之退出); `window.minimize`、`window.requestState('minimized')` 让页面自己最小化 (其余状态回 501); `window.reload([url])` 结束页面线程、
  对同一个窗口重新授予后再起 (url 须在 `http://acde.local/` 下); `capabilities.get/list` 按清单声明回 `available` / `not-declared`
  (临时, 不看平台与逐次同意). 桌面另答 `panel.open/place/anchor/close` (钉住的面板, 同旧管理中枢; 锚定弹出时 `open` 带第二个参数 `anchor`
  `{window, x, y, width, height, edge, align, adjust, visible}` 交给窗口系统的 `grant({mode: 'pinned', rect, anchor})`, `anchor` 转 `reanchor(id, anchor)`;
  父窗口关了或这次被拒时窗口客户端在桌面页的界面事件里发 `panelended` (`code` 是开面板时的面板号, `key` 是 `parent-closed` / `refused`,
  同 `panelmove` 的路子, 不经启动器), 页面里的面板收尾并兑现 `closed`; 弹出窗口 (`pin` 的 `popup: true`) 被合成器收起时发 `panelclosed` (`code` 是面板号, `key` 是 `dismissed`), 同一条路, `closed` 以 `'dismissed'` 兑现;
  原生 `openPanel(id, token, x, y, w, h, exclusive, userOnly, popup, parent)` 的 `parent` 是父面板号 (0 即桌面主层面, 交给窗口系统时为 `None`),
  返回这次实际是不是弹出窗口 (`Surface::open_panel` → 窗口接口 `platform::Window::open_panel(SurfaceCommand::OpenPanel {.., popup, parent})`,
  缺省实现照常发命令并回 false, acdes 的窗口客户端按实际结果回); 窗口系统的只读 `anchors` 为真时页面初始化数据带
  `anchors: true`, 否则页面的 `pin` 带 `anchor` 抛 `NotSupportedError`) 和 `windows.*` (`close`、`kill`、`focus`、`hide`、
  `show`、`requestState`、`screenshot(path)` (PrintScreen 那条, 参数是路径)、`animate`、`stats`、`shortcut`、`unshortcut`、`place`、虚拟桌面的 `addDesk`、`removeDesk`、
  `showDesk`、`moveToDesk` 交给窗口系统对象的同名方法 (acdes 里是 `native.Windows.connect()`, acdev 宿主窗口里是 `native.Shell.broker()`; `animate`、`stats` 在 acdes 里不做事); `launch` 有管家时转给 `launch.start`;
  多屏方案 A 的副屏壁纸 `setWallpaper({color, image?})` 只给桌面页, 先查 `color` 是 `#rrggbb` / `#rrggbbaa` (否则 400 `bad_color`)、`image` 是绝对路径或 null
  (否则 400 `bad_request`), 再交给窗口系统对象的 `setWallpaper(color, image?)` (acdes 里是 `native.Windows`, 值由桌面进程记住, 重载后保持; 宿主窗口没有这个方法);
  不是桌面或窗口系统对象没有这个方法时 501 `unsupported`); 带选项的 `windows.screenshot(options)` (SDK 的那个) 有管家时原样转给管家
  (acdes 在会话主进程里截), 没有管家时只截整个宿主窗口 (`shell.screenshot(path)`, 路径缺省同 PrintScreen: `sys/os.ts` 的 `screenshotFile()`).
- 窗口: 应用向窗口系统要一个占满宿主窗口的窗口; 桌面 `grant({desktop: true})` (窗口号 0). 窗口系统的关窗请求 (`windowcloserequest`, 如 xdg
  的 close) 先问页面 `window.beforeclose` (最多等 3 秒, 页面答 `false` 就不关), 再同样关掉. 桌面进程把窗口系统的其余事件
  交给桌面页 (`js/launcher/src/app/events.ts` 的 `WindowSnapshots`): acdes 给的是 acompositor 形状的快照 `window` (每个窗口一批变化后一条, `first` 表示新窗口,
  `changed` 列出变了的字段, `previous` 是上一个状态), 原样转; acdev 宿主窗口还发旧的 `windowopen` / `windowtitle` / `windowstate` / `windowactivate` /
  `windowparent` / `windowdesk` / `windowgeometry`, 这里按窗口记下快照, 转成同样的 `window` 再交; `windowclose`、`windowcrash`、`currentdesk` 原样转. 页面的窗口列表 (`js/page/src/remote.ts` 的 `WindowList`) 记下 `rect`、`instance`、`desk`.
- 启动方的 isolate 里先装好核心全局对象 (定时器、console 等, `installCore`). 宿主窗口关闭、页面退出或 `--seconds` 到时退出, 退出码是页面的; 收到 SIGTERM / SIGINT / SIGHUP 时 (`acdev dev` 停运行时就是 SIGTERM) 也照常收尾 (停后端、删 socket 目录) 再以 0 退出 (同原 Broker: 按要求停下不算出错), 同一信号再来一次就直接退出 (SIGINT 130, 其余 143). 不收尾的话 macOS 上后端会成孤儿: 那里没有 Linux 的父进程退出信号.
  `--shot` 在 `--shot-after` (默认 2000) 毫秒后存一张宿主窗口截图, 无头 (`ACDE_HEADLESS=1`) 也行; 首帧没画完时截到的是空屏 (WSL 的 Dozen 上
  调试版首帧要 5～6 秒, 验证时用 `--shot-after 15000`).
- **调试通道** (`js/launcher/src/dev/dev.ts` 的 `DevChannel`): 宿主窗口的 `devrequest` 事件 (hostwin `dev.rs`
  转来的 `acdev dev` 请求, 协议见 `docs/agents/dev/dev-ipc.md`) 由本进程按单页回答, 结果经 `Shell.devRespond` 回去; 页面发来的 `{devlog}` (console、
  错误、请求、对话框) 收进 `DevLog`, `{devroute}` 记下假回应用掉一次. `dev-ipc.md` 的请求类型都回答: `snapshot` / `act` / `run` / `query` /
  `input` 由 `inspect.ts` (删 `web/` 前的 `inspect.ts` 取回, 只登记本进程的窗口; 它用的类型和常量在 `dev/inspect/types.ts`, 快照文字的窗口行、窗框菜单节点在 `dev/inspect/text.ts`, 2026-11-23 为行数门禁拆出) 经页面的 `inspect` 服务做; `route` / `init` 记在启动方,
  当场推给页面并在重载时随 `WebInit.devRoutes` / `devInit` 带上; `reload` 和 `acdev dev` 改了文件后的 `devreload` (非热替换时) 先取页面状态
  (`dev.state`, 最多 500 毫秒) 再重新找应用、重开页面并带上 `devState`, 热替换只发 `devhot`. 单应用的特例: `open` 报“没有别的应用”;
  `permissions` 恒为空、`answer` 找不到提示、`revoke` 回 `revoked: false` (单应用不问用户); `resize` 改宿主窗口大小 (`resizeScreen`);
  `emulate` 的系统配色只认 `ACDE_COLOR_SCHEME=dark`.
  页面线程结束 (重载、跳到应用里另一个 HTML) 时, 等在它上面的 rpc 调用一律以“页面已离开”失败, `act` / `run` 把这当成点击引起的跳转照常回答, 不再卡到超时.
- **DevTools** (`js/launcher/src/dev/devtools.ts` 的 `DevToolsEndpoint`): 运行时的 `DevTools.open()` 开了端点 (设了 `ACDE_DEV` 时) 就每 250 毫秒
  把本页登记成 CDP 目标 (id 是 web 名, 列表没变不重登); `devtools` 请求回 `{port}`; 重载前调 `reloading(web)` 让连着的 DevTools 接上新页面;
  宿主窗口的 F12 (`devtools` 事件) 经 `Shell.devEvent` 发 `{type: 'event', event: 'devtools', web, app, window}`, 由 `acdev dev` 打开 DevTools.
- **启动时交给页面的端口** (launcher `manager/pageport.ts`, page `launchport.ts`,
  acde-dom `linePort`): 欢迎消息带 `port: {data}` 时 (由管家决定给谁、连到哪里; 目前是 acdes 的插件窗口, `data` 由开窗的一方给, 可为 null),
  启动器每开一次页面 (含重载) 调一次 `native.Manager.connectBackend()`, 不传参数; 页面运行时用 `linePort` 造一个标准 `MessagePort`,
  页面 `load` 之后的下一个任务在 `window` 上派发一次 `MessageEvent('message', {data, ports: [端口]})` (`source` 为 null, `origin` 为空串).
  线路一行一个 JSON (UTF-8, `\n` 结尾, 单行最多 262144 字节): 页面发的经 rpc 帧 `{portline}` 到启动器写到连接上 (连上之前先排着),
  连接来的按行切开以 `{type: 'portline', line}` 推给页面 (超长的行丢掉, 页面那端收到 `messageerror`). 页面关端口发 `{portclosed: true}`,
  启动器关连接; 连接断了或被拒 (503 `service_not_running`、403、没有 `connectBackend` 时 501 `unsupported`, 都记一行日志) 推
  `{type: 'portclosed'}`, 页面那端收到 `close`. 重载、退出时关掉旧连接. 页面一侧的约定见 `docs/agents/web/web-apps.md`「启动时交给页面的端口」.
- **应用后端** (`js/launcher/src/backend/backend.ts` 的 `AppBackend`; 用户定由启动器起, 宿主和客户机同一份): 应用清单有 `backend`
  时随应用起, 地址 (`sockets` / `channel`) 每次开页面都并进 `WebInit`; `acdev dev` 的 `devreload` (非热替换) 和页面的 `window.reload` 都先停后端
  再起 (`devreload` 用重新找到的清单), 页面的请求在后端就绪前重试 10 秒; 启动器退出前停掉它并删本进程的 socket 目录. 起停、崩溃重起和日志见
  `docs/agents/web/backend.md`.
- 没有的: `desktops` / `sessions` / `applications` / `open`.
  测试在 `js/launcher/test`.

在 `js/`: `bun install` (只装测试用的 happy-dom、React、CodeMirror 等和 esbuild), `bun test` (`js/dom/test` 和
`js/page/test`, 预加载 `dom/test/gc.ts`), `bun run typecheck`. 原 acde-host 的测试只带来了只涉及页面一侧的那几个.

以下按原包名说明 (acde-dom 即 `js/dom`, acde-host 的页面一侧即 `js/page`).

- **acde-dom**: 经 `DomNative` / `GpuNative` / `AudioNative` / `HostNative` (`src/native.ts`) 调 src/host, 打进页面包.
  编辑只有 `dom/editing.ts` 一份. 文本框 (`<input>` 文本类型和 `<textarea>`) 各有一份编辑历史 (`dom/history.ts`): 键盘、粘贴、剪切、输入法提交和 execCommand 的改动
  记成一步, 连续打字 (含换行) 和连续的同向删除合成一步, 选区或值在中间变过就另起一步; 快捷键 Ctrl / Cmd+Z 撤销, Ctrl / Cmd+Shift+Z 和 Ctrl / Cmd+Y
  重做, 先发可取消的 `beforeinput`, 再发 `input` (inputType `historyUndo` / `historyRedo`, data 为 null), 恢复值和那一步的选区; execCommand('undo' / 'redo')
  不发 beforeinput, queryCommandEnabled 看有没有可撤销 / 重做的一步. 脚本把值改成别的 (value、defaultValue、setRangeText、表单 reset) 丢掉这份历史,
  赋同样的值不丢 (React 受控组件). 还没有: contenteditable 的撤销, 拖放文字进文本框.
  测试在 happy-dom 的 MockDom 上跑. 布局类的读 (rect、metrics、计算样式) 缓存到下一次写入、下一个指针事件 / 帧 / 滚动, 或这个任务结束 (CSS 过渡和动画在 render 线程上跑, JS 不知道, 下一个任务要重新读). Web Animations
  (`dom/animation.ts`): 关键帧变成 `@keyframes acde-animation-<n>` 规则加元素内联的 `animation-*`, 由 render 跑; 时间、事件、`finished` 在 JS
  里按 host 的时钟; 效果的 `composite` 写成内联的 `animation-composition`, 关键帧上不是 `auto` 的 `composite` 写成那一帧里的 `animation-composition`
  (css-animations-2, 以前丢掉, acss 构建里 `add` 没有叠到底值上; 取值不对的抛 TypeError); 近似: easing 作用在每段关键帧上、endDelay 只算进结束时间、
  没有 `iterationComposite`.
  SVG 几何 (`dom/svg.ts`, `dom/svgbox.ts`): `getBBox` 的基本图形和容器按属性在 JS 里算, 文字、图片、use 这类没有几何的子元素取 render
  画出来的框 (`rect` 经 `svg_box`) 换回元素的用户坐标; `SVGTextContentElement` 的 `getComputedTextLength` / `getSubStringLength` /
  `getNumberOfChars` 按 xml:space 默认规则整理文字, 用计算出的字体 `measureText` 量前进宽度, 加上 letter-spacing.
  popover 和模态 dialog (`dom/popover.ts`): `showPopover` / `hidePopover` / `togglePopover` 按 HTML 发 `beforetoggle` (打开时可取消) 和排队的
  `toggle`, auto / hint popover 打开时关掉栈里不是它祖先的 auto popover, manual 不动; 文档上按下 (pointerdown, capture) 关掉不含落点的 auto
  popover, Escape 关最上面的 auto popover 或请求关闭最上面的模态 dialog (`requestClose`); 按钮和 `input` 的 `popovertarget` /
  `popovertargetaction` 在激活时做 (表单里的提交 / 重置按钮仍先提交 / 重置), `popoverTargetElement`; 状态经 `dom.setOpenState` 给 render.
  还没有: popover 的 hint 栈单独管理、焦点回到调用者、`::backdrop`.
  CSS 动画和过渡的事件 (`dom/cssevents.ts`): 页面第一次给这 8 种事件 (或 webkit 前缀的 4 种) 加监听或 on 处理器时调 `wantAnimationEvents`
  (render 从这时起留下事件, 之前的丢掉), 之后每帧开头 `takeFrame(true)` 一次锁里取滚动、加载和动画事件 (没有监听时 `takeFrame(false)`, 不取动画事件)
  (文档改过、样式待算时 render 先算到最新; 只是动画在走时不重算, 拿出帧线程上一次重算记下的事件, 最多晚一帧), 照规范在 rAF 回调之前在元素上发 `AnimationEvent` / `TransitionEvent` (冒泡, 不可取消, 伪元素的发在宿主元素上带 `pseudoElement`);
  Web Animations 自己的 `acde-animation-<n>` 关键帧不发. 出帧线程的重算记下新事件时经 `wantAnimationEvents` 挂上的回调给页面要一帧 (`webnative::request_frame_now`), 页面不再因为动画在跑每帧都要一帧, 只在刚有事件时再要一帧; 改了 DOM 的任务结束时也要一帧, 好让新开始的动画被看到.
  还没有: 只有 webkit 前缀监听时改发旧名 (DOM 的 legacy event types), `getAnimations()` 里的 CSS 动画.
  拖滚动条 (`dom/scrollbar.ts`, `Input` 在别的指针处理之前调, AC/dopame#20): 左键按下时问原生 `scrollbarAt()` (render 里指针下、看得见的覆盖式滚动条滑块,
  `hovered_scrollbar`; 回 `[元素 id, 轴 0 横 1 竖, 每像素滑块对应的内容像素]`, 没有就是空), 按在滑块上就开始拖: 移动时按起点的滚动位置加位移乘比例
  `scrollTo` 那个元素 (不小于 0, 上限由原生夹住), 松开结束. 拖滑块的按下、移动、松开都不发给页面, 也不动选区和焦点. 滑块只在滚动后显示 (悬停不唤出), 看不见时按不到.
  用指针选文字 (`dom/textselect.ts`, `Input` 调): 左键按下 (pointerdown / mousedown 都没取消) 在 `caretFromPoint` 处折叠选区, 先发可取消的
  `selectstart`; 按住移动时把焦点端扩到指针处; 第二次按下 (500 ms、5 px 内) 选一个词 (`Intl.Segmenter`, 中文按词典分词), 第三次选所在
  块 (向上找到 `display` 不是行内的祖先), 拖动时按同样的粒度扩展; Shift 按下从原锚点扩展. 按在表单控件、按钮、label、可编辑区域、链接、
  图片、`draggable` 元素或 `user-select` 用到的值是 none 的地方 (自己是 `auto` 就往上看祖先, 到第一个不是 `auto` 的为止) 不动选区.
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

**会话里的应用菜单** (和 acdes 对齐): 应用的 `window.setMenus` 经管家推给桌面 (`windowmenus`); 桌面进程的启动器把 `windows.invokeMenu`
交给 `manager.call('windows', 'invokeMenu', args)`, 管家按 pid 给应用进程推 `{type: 'menuinvoke', menu, item}`, 应用进程的启动器照菜单快捷键那条路调页面的 `window.menu(menu, item)`.

**会话里的可见性** (acdes 暂停网页壁纸用): 管家给应用进程 (含后台开的窗口) 推 `{type: 'visibility', visible}`, 启动器调页面的
`window.visible(visible)` (`WindowSelf.onVisible` → `setVisible`): `document.visibilityState` 跟着变并发 `visibilitychange`. 页面 hidden 时
`requestAnimationFrame` 的回调不跑, 帧跑完也不再为它们要下一帧, 和浏览器的后台标签页一样; 回到 visible 时等着的回调在下一帧跑 (acde-dom `index.ts` 的 `frame`).

**图标主题变了**: 管家给桌面推 `{type: 'iconschanged', theme}` 并重发服务表; 页面里是 `acde:icons` 的 `on('changed', ({theme}) => …)`
(`Host.NOTICES` → `icons:changed`), `acde:icons.theme()` 读的是服务表里最新的同步值, 事件先到时先按事件里的 `theme` 改掉 (`RemoteBridge.SERVICE_EVENTS`
列出带事件的服务, 由 `RemoteBridge.THEMES` 生成: `icons`、`sounds`).
**声音主题变了**: 同法, 管家推 `{type: 'soundschanged', theme}` → `sounds:changed`; `acde:sounds` (类型在 `acde-api/src/types/sounds/`, 以模块扩充给 `AcdePage` 加 `sounds`, 为 page.d.ts 的行数门禁) 有 `theme()`、`lookup(names)` (返回声音文件的绝对路径, 页面经 `/acde-sounds/<路径>` 播放) 和 `on('changed')`.

**会话里的菜单快捷键**: 宿主上菜单快捷键经 `shell.shortcut(combo, 'menu', window)` 在合成器登记; 会话里应用进程没有桌面协议, 登记不上.
所以有管家时, 应用的 `window.setMenus` 照旧交给管家, 启动器另外记下菜单的快捷键表 (`AppMenus.remember`), 以 `{type: 'menukeys', keys}` 发给页面线程;
页面的 Host 在 keydown 时先按 `chordMatches` 比对 (`mod` 在 macOS 是 Meta, 别处是 Ctrl), 命中就不再派发给页面, 改发 `{menukey}` 给启动器, 启动器调页面的 `window.menu(menu, item)`.

**重载时交还快捷键**: 页面经 `windows.shortcut` 登记的会话级和桌面级快捷键都登记在启动器这个进程的 Shell 上, 页面重载 (`window.reload`、开发重载) 时进程和连接都不变,
旧的登记会留着, 新页面再登记同一组合就被 acompositor 以 "another holds it" 拒掉. 启动器记下本页登记过的快捷键, 重起页面前逐个 `unshortcut` 交还.
