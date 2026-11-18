# acdev dev: IPC、调试请求、dev 日志和热重载

IPC: 运行时是服务端, 在 `ACDE_DEV` 上听 (hostwin `dev.rs`, 线程 `acde-dev`, 每条连接一条 `acde-dev-conn`), acdev dev 和工具 (acdev 的命令、
MCP、driver) 都直连它, 中间没有转发. unix 上是 unix socket (地址和 0700 的目录由 acdev dev 定好、旧 socket 它先删, 运行时 bind 后设 0600,
退出后 acdev dev 删), Windows 上是命名管道服务端 (`FILE_FLAG_FIRST_PIPE_INSTANCE` 防别人抢先占名、`PIPE_REJECT_REMOTE_CLIENTS`、SDDL
`D:P(A;;GA;;;OW)` 只给当前用户, 接一个再开下一个实例). 这条 IPC 能在页面里求值, 所以只给自己. 一行一个 JSON. 发 `{"type":"hello","role":"dev"}`
的那条连接是 acdev dev 的: 它断了运行时就退出, 运行时主动发的事件也只发给它; 别的 hello 忽略. Windows 上读之前先 `PeekNamedPipe`, 有数据才 `ReadFile`: 同一个同步句柄上
挂着的读会让写 (回答) 一直等着. 工程的构建结果经
`ACDE_PATH` (Windows 用 `;` 分隔) 交给运行时, 不装进包目录; 日志就是运行时的 stderr.

运行时主动发的事件 (`Shell.devEvent(json)` → hostwin `dev::event`, 写给每条 acdev dev 的连接, 没有就丢; 一行
`{"type":"event","event",…}`):

- `{"type":"event","event":"devtools","web","app","window"}`: 宿主窗口里按了 F12 (#196). hostwin `App::key` 前面的
  `dev::DevtoolsKey` (只在有 `ACDE_DEV` 时开) 拦下不带修饰键的 F12: 按下推一个 `devtools` 事件给 Broker (`window` 是有焦点的窗口),
  按住的重复和松开都吞掉, 页面收不到 (宿主窗口失焦时清掉「按住」: 打开的浏览器抢走焦点, 松开常常到不了这里); 带修饰键的 F12、没有 `ACDE_DEV` 时的 F12 照常给页面. Broker 把窗口换成 web (那个窗口的 web,
  原生窗口的 owner, 都没有时桌面), `DevRequests.devtools` 发事件: `web` 是 web 名 (即 `/json/list` 的目标 id), `app` 是应用 id
  (桌面是 null), `window` 是窗口号. 按键不进页面线程, 所以页面停在断点上时 F12 也照样到. acdev dev 收到后和 `acdev devtools <web>`
  一样打开 DevTools (见 acdev 的 `dev.ts`).

收到的消息 (`reload` / `quit` 哪条连接发都照做):

- `{"type":"reload","app"?,"hot"?}` → Broker 的 `devreload`: 有 app 时关掉它所有窗口 (不问 `beforeclose`) 再按记住的 rect 打开 (清单的 `size` 变了时按新尺寸, 中心不动, 不超出屏幕), 没有 app 时
  重起 desktop web (服务不重起). `hot: true` 时先试热替换 (下面).
- `{"type":"quit"}` (dev 连接断开也一样) → Broker 的 `quit` 事件 (结束原因 `quit`, 退出码 0; `exit` 是意外退出, 退出码 1), 照常收尾 (关页面、停后端、删 socket 目录) 后退出; 6 秒没退完
  直接 `Cmd::Exit(0)`.
- `{"type":"request","id","op",…}` (id 是数字或字符串, 只在这条连接里有意义) → id 换成进程内递增的号, 整行作为 `devrequest` 推给 Broker
  (`broker/dev.ts` `DevRequests.request`), 做完调 `Shell.devRespond`, 按号找回连接、换回原 id 写出 `{"type":"response","id","ok":true,"result"}` 或
  `{…,"ok":false,"error":"<文字>"}`, 每个请求恰好一次 (超时在工具一侧); 连接断了, 它还没回答的请求作废. `web` 参数是 web 名
  (`ledger#1048576`)、应用 id (window 号最小的那个 web) 或 `desktop`:
  - `webs` → `{webs: [{name, app, kind, window, title}]}`.
  - `devtools` → `{port}`: 这个会话的 DevTools 端点 (下面 **DevTools 端点**); 没有端点 (不是 dev 模式、端口没开成) 时回错.
  - `eval {web, code, node?, arg?}` → `{value}`: 和 `ACDE_DEBUG_DIR` 一样求值 (promise 等落定), 结构化克隆回来再 JSON (undefined 是 null, JSON 不了的回错).
    给了 `node` (render 节点 id, 即 ref 里点后面的数) 时 `code` 是一个函数的源码, 以 `(元素, arg)` 调用 (rpc `debug.evalOn`).
  - `shot {web}` → `{png: <base64>, width, height}` (`Shell.capture`, 设备像素, 桌面的含面板).
  - `snapshot` / `act` / `run` / `open {name}` / `query {selector, window?, ref?, limit?, attributes?}` → `Broker.inspect.api` 的同名方法 (`docs/web/inspect.md`「看和操作桌面」).
  - `console {web?, since?, limit?, level?}` → `{entries, next, dropped}`: 页面的 `console.*` 和没人接的错误 (`devlog.ts`, 见下), `level` 是
    log / info / warn / error / debug, 或 errors (console.error 和错误); `web` 是 web 名、应用 id 或 desktop; `since` 是上次的 `next`, `dropped`
    是 since 之后已经丢掉的条数. 条目 `{seq, time (ms, 和日志同一个零点), web, app, kind: console | error | rejection, level, text}`.
  - `network {web?, since?, limit?, failed?}` → 同样的形状, 条目 `{seq, time, web, app, id, kind: fetch | xhr | websocket | eventsource, method,
    url, requestBytes, status, statusText, responseBytes, ms, error, done}`; `failed` 只要出错的和 status ≥ 400 的.
  - `screen {path}` → `Shell.screenshot(path)` 加 `path` (绝对路径; 整个屏幕, 所有窗口合在一起).
  - `emulate {colorScheme?: light | dark | null}` → `{colorScheme, emulated}`: 所有页面 (和以后开的) 的 `prefers-color-scheme`
    (CSS 和 `matchMedia`); null 回到系统的 (portal / `ACDE_COLOR_SCHEME`). 模拟配色时系统配色的变化只记下, 回到系统时再生效.
  - `choose {folder | open | save}` (恰好给一个) → 原样回: 事先答复页面下一次的这种选择器, 不弹窗, 三种各排各的队列, null 当作用户取消.
    `folder: 绝对路径` 答 `showDirectoryPicker`; `open: [绝对路径, …]` (不空) 答 `showOpenFilePicker` / `<input type=file>`, 多于一个而这次
    不许多选时这次回错; `save: 绝对路径` 答 `showSaveFilePicker`. napi `Shell.answerFolder` / `answerOpen` / `answerSave` → hostwin 的队列,
    acdev runtime 的 `src/chooser.rs` 先取队列再弹窗, 有窗口、headless、Linux 的 portal 都一样; 会话没有这个, 回错.
  - `route {url, method?, web?, times?, status?, statusText?, headers?, body?, abort?}` → `{id}`: 让页面的 fetch / XHR 拿到假的回应, 不到应用的
    文件、后端或网络 (`devroutes.ts`). `url` 是 glob (`**` 任意, `*` 不含 `/`; `/` 开头的比路径加查询, 否则比整个 URL) 或 `{regex, flags}`;
    `body` 是 base64; `status` 200–599 (默认 200), `statusText` 默认按状态码; `abort: true | 文字` 让请求失败 (不能和回应一起给); `times` 用完就删.
    后加的先匹配. Broker 存着规则, 推给匹配 `web` 的页面 (`{type: 'devroutes'}`, 新起的页面在 `WebInit.devRoutes` 里拿); 页面按 acde-dom 的
    `observe({route})` 先问规则再走 `route()`, 带 `times` 的用一次 `port.post({devroute: id})` 给 Broker 减一 (各页面自己也减, 几个页面同时用可能多
    用一次). 不管 CORS、cookie; WebSocket / EventSource 不经它. network 的条目带 `mocked: true`.
  - `unroute {route?}` → `{removed, left}`: 删 id 是 `route` 的那条, 不给全删 (请求自己的 `id` 是请求号, 所以不叫 id).
  - `drop {files, web?, x?, y?}` → `{web, window, files}`: 模拟把这些文件 (绝对路径, 不空) 从系统拖到 web (默认 desktop) 的窗口上, `x` / `y` 是页面坐标
    (一起给, 默认页面中心), 按这一点命中 (被别的窗口盖住就落到那个窗口), 和真的系统拖放同一段 (`dndenter` + `dnddrop`); 只排进主线程就回答,
    页面收到 drop 要自己等. 会话没有, 回错.
  - `pointer {x, y}` → `{x, y}`: 把指针移到屏幕坐标 (会话是全局逻辑坐标, 宿主窗口是内容区坐标), 同真的移动 (悬停、屏幕边缘事件都照常发).
  - `clipboard {text?}` → `{text}`: 给 `text` 先写, 再读回宿主剪贴板 (napi `Shell.readClipboard` / `writeClipboard` → hostwin; headless 是进程里那份,
    有窗口是系统剪贴板); 会话没有, 回错.
  - `input {web, events}` → act 的回答: 按页面的 CSS 坐标注入一串原始输入 (`Inspect.input`, 加窗口原点和 `contentOffset` 后 `Shell.inject`):
    `{type: move | down | up | wheel, x, y, button?: left | right | middle, dx?, dy?}`、`{type: keydown | keyup, key}` (键名同 act 的 key)、
    `{type: text, text}` (走输入法提交那条路, 给焦点元素)、`{type: wait, ms}` (停 0–10000 ms, 中间按住的键和按钮不放: 游戏里按住方向键). down / up / wheel 发在它们的 x、y 上: 指针不在那里时先补一个 move (注入的按钮和滚轮只认指针所在). 先给窗口焦点; 连着的 move 之间隔 16 ms (页面的 pointermove 按帧合并), 最后等页面安静.
    act 的 click / double / right / hover 的 arg 是要按住的修饰键 (`shift`、`ctrl+shift`, 还认 `option` / `cmd`).
  - `clock {web, action, time?, ms?}` → `{installed, paused, now, performanceNow, fixed, timers}`: 接管这个页面的时间 (rpc `dev.clock`, acde-dom
    `core/clock.ts`, 和 Playwright 的 clock 一样): `install {time?}` 之后 `Date` (换成同原型的包装, `Date()` / `Date.now()` / 无参 `new Date()`
    和 `Intl.DateTimeFormat` 的无参 format / formatToParts)、`performance.now`、`Event.timeStamp`、rAF 的时间戳、Web Animations 和页面的
    `setTimeout` / `setInterval` / `requestIdleCallback` 都走虚拟时间 (照常流动, 定时器排在虚拟队列里, 一个宿主定时器叫醒最早的); `pauseAt {time}`
    跳到那时停住 (已经过了就在此刻停住、`Date` 改成那个时间, performance.now 不倒退); `fastForward {ms}` 跳过去, 到期的定时器各响一次 (间隔
    的也一次); `runFor {ms}` 一步步走, 每个定时器在它自己的时刻响 (间隔的每次都响), 每响一个让出一个任务 (微任务跟着跑); `resume`;
    `setFixedTime {time}` 钉住 `Date`; `setSystemTime {time}` 只挪 `Date`; `state`. 装了不能卸 (页面重载就没了). CSS 动画和过渡走 render 的
    时钟, 不受它管. acde-dom 和 acde-host 自己的计时 (任务队列 `queueTask`、双击判断、select 的打字跳转、`settle`、输入法、面板测量、dev
    状态恢复、GPU 计时) 用真实时间: acde-dom 导出 `hostTime` (`now` / `wallNow` / `setTimeout` / `clearTimeout`) 给宿主代码用.
  - `dialog {web, accept?, text?}` → `{web, accept}`: 事先答复这个页面下一个 `alert` / `confirm` / `prompt` (页面里按顺序排, `devdialog` 消息):
    `accept: false` 是取消, `text` 是 prompt 的回答 (不给用默认值). 没有答复时照旧 (`alert` 画在窗口里, `confirm` false, `prompt` null).
    每个对话框都记进 console (`kind: dialog`, `level: info`, 带 `dialog: {type, message, defaultValue, answered, accept, text}`).
  - `init {code, web?, once?}` → `{id}`: 页面加载前要跑的脚本 (`WebInit.devInit`, 在页面的 HTML 解析之前, 全局都在、`document` 还空),
    `web` 是 web 名或应用 id, 不给是所有页面; `once` 的只给下一个起来的页面. `uninit {init?}` → `{removed, left}`.
  - `reload {web}` → `{web}`: 和 IPC 的 `reload` 一样重开这个应用的窗口 (或重起桌面), 等它重开.
  - `resize {web, width, height}` → `{width, height}`: 页面 (不含标题栏) 改成这么大. `free` 窗口经 `Shell.place`; 铺满的 (单应用的 acdev dev、
    桌面) 改宿主窗口 (`Shell.resizeScreen` → hostwin `Cmd::Resize`: headless 直接换输出尺寸, winit `request_inner_size`, Windows 还没有);
    会话里回错.
- **DevTools 端点**: 见 `docs/dev/devtools.md`.
- **dev 日志** (只在有 `ACDE_DEV` 时): acde-dom `core/observe.ts` 的 `observe(observer)` 是页面里唯一的观察点, 没注册时零开销. `console.*`
  (照打日志之外) 报 `console(level, text)`; 有 inspector 的 dev 页面 (`host.inspected()`) 里 acde-dom 的 console 还把原参数交给 V8 自带的
  `console` (装上前的 `globalThis.console`, 同名方法, 每次一份), 由它报给 inspector (`Runtime.consoleAPICalled`, 真的 RemoteObject 和调用栈);
  V8 的 console 不打日志, 所以 dev 日志不重复; 没人接的 `error` / `unhandledrejection` (交给 `host.reportError` 的那些) 报 `error(kind, text)`
  (Error 的 stack, 别的 realm 来的按 `stack` / `name` + `message` 认; 栈和 console 文字里 `http(s)://…/x.js:行:列` 的帧, 包里有这个文件的 source map 时
  换成源码位置, 如 `src/main.js:8:9`: acde-host `devlog.ts` `SourceMaps` 读文件末尾的 `sourceMappingURL` (同源文件或 data: URL), 按文件缓存,
  map 经 host 的 `readSourceMap` 读 (src/host `dom.rs`: 页面的 `readAsset` / fetch 仍拿不到 `.map`); 没有 map 的帧原样留. 交给
  `host.reportError` 的 "page error" 日志也用换过的栈, #45); `fetch` / XHR / EventSource (都经 `exchange`, 加上本地资源和够不着的地址)
  和 WebSocket 报 `request`: start → response (WebSocket 是 101) → end (收到的字节) 或 error (中止是 `aborted`; 够不着时说清是「应用里没有这个
  文件, 也没有后端」还是「不是这个应用的地址」). 页面 (`worker.ts`) 把它们单向 `port.post` 成 `{devlog, time, …}` 给 Broker (不是 rpc,
  页面重启、崩溃了记录还在); Broker 的 `DevLog` 两种各留最后 5000 条、文字最长 16 KB, 请求按 web + id 合成一条, web 没了时还开着的记成
  `the page went away`.

**热重载** (acdev 构建时不压缩, `NODE_ENV` 是 development). acdev 比较构建前后的产物 (acdev 仓库 `src/changes.ts`): 只有 `.css` / `.js` 变了、
HTML 去掉样式和脚本引用后一样时发 `hot: true`. Broker 给那个应用的窗口 (或桌面) 发 `devhot`, web (acde-host `devhot.ts`) 重读入口 HTML,
都能热替换才动手, 否则整页重载:

1. 样式: 新旧 HTML 的 `<style>` / `<link rel=stylesheet>` 按顺序和页面里的对上, 换文字或 `href` (加 `acde-dev=<n>` 让 render 重读).
2. JS 函数体: 内容变了的 module 脚本经 V8 inspector 的 `Debugger.setScriptSource` 换源码 (src/host `inspector/`: 有 `ACDE_DEV` 时每个
   isolate 一个 inspector session, V8 带 `--inspector-live-edit`; 性能采样 `profile.rs` 也用这个 session, 同步 `inspector::send`).
   同一个 inspector 还能开别的 session (#182, 给开发者工具用): 页面线程上 `inspector::remote()` 拿到可跨线程的 `Remote`,
   `connect(sink)` → session 号, `send(session, 协议消息 JSON)`, `disconnect(session)`. 投递进一个队列 (`Pending`, 带 Condvar,
   断点暂停时的消息循环 (#174) 也从它取), 队列由空变非空时给页面事件循环发一个任务 `dispatch_posted`, 所以页面空闲时也立刻处理;
   这些 session 的回复和通知都原样交给 `sink` (在页面线程上调用, `sink` 自己转到别的线程); 页面的上下文登记时带 auxData `{isDefault: true, type: "default", frameId: <web 名>}`,
   所以不带 `contextId` 的 `Runtime.evaluate` (DevTools 控制台默认这样发) 落在它上面. dev 模式下模块源码末尾加 `"\n;"` (`inspector::module_source`), 换源码时也加:
   V8 LiveEdit 的 diff 把文件末尾追加的代码当成没变. 改了模块顶层 (`BlockedByTopLevelEsModuleChange`)、函数在栈上、classic 脚本变了都整页重载.
   运行时自己的脚本 (`web.js`、`broker.js`) 在 DevTools 里是忽略列表里的脚本 (#184), 前端在控制台消息、异常和单步时跳过它们的帧, 位置落在页面代码上:
   dev 构建 (`scripts/bundle.mjs`) 给 `web.js.map` / `broker.js.map` 的每个源加 `ignoreList` / `x_google_ignoreList`; 没有自己 source map 的
   (正式构建) 在 dev 模式下由 `inspector/ignore.rs` 在源码末尾接一个 data: URL 的 source map, 整个文件映射到一个忽略列表里的源 `acde-runtime`.
   正式构建的文件本身不变. 前端的“自动把已知第三方脚本加入忽略列表”关掉时不起作用.
3. 整页重载保留状态 (所有 reload): 关窗口前问旧页面要快照 (`dev.state`, 最多 0.5 秒, `devstate.ts`): 表单值、滚动位置、焦点和选区, 元素按 id
   或 `nth-child` 路径认; 新页面加载完在 0 / 50 / 200 / 600 / 1500 ms 各放回一次, 值经原型上的 setter 写再发 `input` / `change`.

**客户机里 (有会话管家时)** (单应用 15, 和 acdes 管家对齐, 2026-11-19): 运行时不听 `ACDE_DEV`, 调试请求由 acdes 管家的调试服务端
(`$XDG_RUNTIME_DIR/acde/dev.sock`, 只在开发客户机里建, 判断依据是 `/sys/firmware/qemu_fw_cfg/by_name/opt/acdev/key/raw` 存在; 启动器自己不判断,
管家不开就收不到) 按 `web` 分给各进程的启动器, 经管家通道 (`ACDE_MANAGER_FD`) 往来 (`dev/relay.ts` `managerRelay`):

- 管家 → 启动器: `{type:'devrequest', key}`, `key` 是一行请求原文, id 已换成管家的号, 走和宿主同一个 `DevChannel.request`;
  `{type:'devreload', hot?}` 走 `devReload` (管家按 `app` 只发给那个应用的进程, 没有 app 时发给桌面).
- 启动器 → 管家: `{devresponse:{id, ok, payload}}` 代替 `Shell.devRespond` (管家按号找回连接、换回原 id); `{devevent:'<json>'}` 代替
  `Shell.devEvent` (F12 等事件, 管家转给发过 hello role=dev 的连接); DevTools 端点开成时主动报一次 `{devtools:{port}}`, 管家按进程记下,
  客户机里 `devtools` 请求由管家回 `{ports:[{web, app, port}]}`. `webs` 由管家向各进程逐个发再合并, 不另加消息.
- 锁屏页 (welcome 顶层 `lock: true`): 页面名固定为 `lock`, `webs` 里带 `lock: true` (kind 仍是 desktop), `web: "lock"` 找到它, `desktop` 不再指它.
  宿主上 `acdev dev --lock` 的预览没有管家, 页面名仍是 `desktop`, 也带 `lock: true`.
- 页面和运行时的开发能力 (`debug.eval`/`evalOn`、`dev.clock`、devInit、devroute、DevTools 端点与页面域、V8 live-edit) 只看 `ACDE_DEV` 在不在.
  客户机里由会话总管在开发客户机 (fw_cfg 文件存在) 时设 `ACDE_DEV=guest`, 启动器把 `ACDE_*` 传给页面线程; 正式系统里没有它, 即使有, 没有管家转发请求也到不了页面.
- 窗口表 (和 acdes 对齐, 2026-11-20): 管家转发 snapshot、act、query、run、open 时在请求里附 `windows: [{id, app_id, pid, title, x, y, width, height,
  activated, hidden, state, desktop}]` (winman 的窗口表, 上层在前; `desk` 是虚拟桌面号; 最后一行是桌面 `{id: 0, desktop: true, pid}`, 没有坐标). 启动器的 Inspect 有它时用它列窗口 (`dev/guest.ts` `GuestWindows`): 窗口号就是 winman 的 id
  (`w<id>`), 桌面是 `w0`; pid 等于本进程的那一行是自己的窗口 (桌面进程认 `desktop: true` 的那行), 有元素, 映射回本地窗口号; 别的进程的窗口列成 native, 没有元素.
  所以管家按 ref 的窗口号 (w0 给桌面进程, 其余按 pid 找进程) 转发 act 和带 ref 的 snapshot / query.
- 页面名: 管家的 app welcome 带 `instance` (同一应用从 1 起的编号, 和管家 `instances` / `instancegone` 里的一致) 时, 应用的页面名是 `<应用 id>#<instance>`;
  没有时照旧用 `<应用 id>#<本地窗口号>` (宿主上的单应用).
- 指针和按键 (2026-11-20): 有窗口表时, Inspect 的 act (click、double、right、hover、scroll、drag、key) 不经本进程的 shell, 而是发
  `{devinject: {type: move | down | up | wheel | keydown | keyup, x?, y?, button?, dx?, dy?, source?, code?}}` 给管家, 由管家交 winman 注入成真实输入;
  坐标是 winman 的屏幕坐标 (窗口表的 x、y 加元素在页面里的位置), `button` 和 `code` 是 evdev 码 (左键 0x110, 右键 0x111).
- 锁屏页的窗口 (2026-11-20): 锁着时管家在窗口表里加一行锁屏页 `{id, lock: true, pid: <锁屏进程>, title: "lock"}` (`id` 是 winman 给锁屏面的号, 没有时管家另取一个不和别的窗口撞的号; 只在一次锁定期间不变, 每次锁定可能换号, 用例每次锁定后重新查 `webs`).
  锁屏进程认 `lock: true` 且 pid 是自己的那一行, 叠在最上面, ref 是 `w<id>`; 有窗口表时 `webs` 报的 `window` 也换成窗口表里的号 (桌面 0, 锁屏页和应用是 winman 的号),
  所以 `acde.window({id: web.window})` 能拿到锁屏页. 管家转发 `webs` 时也附 `windows`.
- 跨进程拖放 (2026-11-20): 客户机里目标在别的进程的窗口时, 拖放的源进程量不到目标元素; act drag 的 `arg` 也收屏幕坐标 `"x,y"`,
  acdev driver 在客户机会话里用目标窗口的原点加目标元素的 rect 中心算出坐标再发.
- 窗口操作 (2026-11-20): 有窗口表时, Inspect 对 winman 窗口 (窗口表里有、不是桌面 0) 的最小化、还原、铺满、全屏、聚焦和关别的进程的窗口, 不经本进程的 shell
  (应用进程的 shell 不认识 winman 的号), 改发 `{devwindow: {window, state?, focus?: true, close?: true}}` 给管家, 由管家交 winman_window_v1 去做; 桌面和面板仍走本进程.
