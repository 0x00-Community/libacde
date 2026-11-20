# DevTools 端点 (Chrome DevTools Protocol)

acdev dev 的 IPC 和调试请求见 `docs/dev/dev-ipc.md`.

- **DevTools 端点** (只在开发模式下: 没有管家的进程看 `ACDE_DEV`, 有管家的进程只看 acdev 客户机的固件配置项, 见 `dev-ipc.md` 的“开发模式”; Chrome DevTools Protocol, #172): runtime 模块 `devtools/` (napi `DevTools.open()` /
  `DevTools.targets(list)`), 宿主窗口和 Linux 会话一样. Broker 建 `DevRequests` 时 `open()`: 在 `127.0.0.1` 上听系统挑的端口,
  线程 `acde-devtools`, 每条连接一条 `acde-devtools-conn`; 没有 `ACDE_DEV` 时 `open()` 回 null, 不开任何端口. 没有令牌 (#199, 同 Chrome 自己的
  调试端口): 防护见下面的 `Host` 和 `Origin`. `GET /json/version` (`Browser`、`Protocol-Version`、
  浏览器的 `webSocketDebuggerUrl`), `GET /json` 和 `/json/list` (每个目标 `{id, type, title, url, webSocketDebuggerUrl, devtoolsFrontendUrl,
  description}`), 别的 404. 目标是 Broker 的 webs 加上服务 (#193): web 的 `id` 是 web 名 (`alpha#7`, 桌面 `desktop`, 地址里按百分号编码),
  `type` 是 `page`, `title` 是 `webs` 的标题加括号里的应用 id (桌面是 `(desktop)`), `url` 是页面地址; 服务都跑在 Broker 的 isolate 里, 合成
  一个目标: `id` 是 `broker` (Broker 的 isolate 名, 它和别的 isolate 一样在 inspector 里登记), `type` 是 `service`, `title` 是
  `services: <服务名, …>`, `url` 空, 有服务在跑时才列; 后端以后定. `DevRequests` 每 250 ms 算一次 (`publish()`), 列表变了才推给
  `DevTools.targets`, 从列表里消失的目标的 WebSocket 以 1001 关掉. `webSocketDebuggerUrl` 是 `ws://127.0.0.1:<port>/devtools/page/<id>`,
  `devtoolsFrontendUrl` 是 `devtools://devtools/bundled/devtools_app.html?ws=<地址>` (不是 Chrome 给远程设备用的 `inspector.html`: 那个左边留一块投屏区, 我们不答 `Page.startScreencast`, 只剩一片空白), 但 `ws` 的值整个再百分号编码一次 (前端读它时解码一次; 目标 id 里的 `#` 在路径里本来就是 `%23`, 不再编码会被还原成 `#` 当片段丢掉), 所以 `chrome://inspect`
  里配上 `127.0.0.1:<port>` 就能发现目标并打开. 不在列表里的目标升级前回 404. WebSocket 上: `Browser.getVersion` 自己答, 别的
  方法交给 `acde_runtime::devtools::forward` 装上的 `Forward` (`attach(client)` / `message(client, text) -> bool` / `detach(client)`, 在连接
  线程上调用; `Client::target()` 是 web 名, 浏览器端点是 None; `Client::send(text)` 任意线程发一条文本帧, 连接断了回 false), 没装或它回
  false 时答 `-32601 '<method>' wasn't found`; 请求带 `sessionId` 的回答原样带回. 只收文本帧 (16 MB 以内), 二进制帧关连接;
  控制帧不超过 125 字节、不分片. 防护 (同 Chrome 的调试端口): `Host` 头只认 `127.0.0.1`、`localhost`、`[::1]` (可带端口), 别的 (含没有) 先回 400
  (防 DNS 重绑定); 然后 HTTP 和 WebSocket 请求带了 `Origin` 而不是 `devtools://devtools` 的回 403 (浏览器里的网页连本机端口时一定带自己的
  `Origin` 且改不了), 不带 `Origin` 的 (命令行工具、`acdev devtools`、`chrome://inspect` 和从它打开的 DevTools) 照常. 只在 dev 模式开、只听
  127.0.0.1; 同一台机器上的其他本地用户能连到, 和 Chrome、Node 的调试端口一样; 同时最多 32 条连接, 多的接下就关; 请求头从建连起 10 秒内读完, 否则断开. `Client::send` 只把帧放进
  这个客户端的有界队列 (256 帧) 就返回, 不阻塞调用方 (页面线程也能调); 每个客户端一条 `acde-devtools-send` 线程按序写出, 套接字写超时
  10 秒; 队列满、写失败或超时都断开这个客户端.
  页面目标接到页面的 V8 inspector (#184, src/host `inspector/devtools.rs` 的 `Pages`, 第一次登记时经 `forward` 装上): dev 模式下每个 isolate
  `inspector::start` 后把 `inspector::remote()` 按 web 名登记 (`devtools::opened`), `inspector::stop` 时 `closed`. 客户端 `attach` 时在
  它的目标页面上开一个 inspector session (`Remote::connect`), 回复和通知直接 `Client::send` 回去; `message` 原样投给这个 session
  (目标还没登记时回 false, 端点答 -32601), `detach` 断开. 页面关了: 给它的客户端发 `Inspector.detached {reason: "target_closed"}` 再关
  WebSocket; 重载例外 (#202, 见下). 有 session 连着时页面事件循环不退出 (`keep_alive`).
  **断点和单步** (#194): V8 暂停 (断点、`debugger`、单步) 时调 inspector client 的 `run_message_loop_on_pause`, 页面线程在
  `inspector::paused` 里等 `Pending` 的条件变量 (每 100 ms 醒一次), 有消息就 `dispatch_posted` (V8 域和页面域都答, 页面域的处理器在
  暂停中照样跑, 不过任务末尾的钩子和微任务检查点要等继续以后); V8 的 `quit_message_loop_on_pause` (继续、单步) 让它返回. 三种情况也返回、
  页面接着跑: 最后一个客户端断开 (`extra` 空了)、页面要停 (`inspector::stop_when` 交给它的 `stopping`, web 被 terminate 时为真, 返回后
  V8 的 terminate 生效)、进程收到退出信号 (sys.rs 的信号线程先调 `inspector::release_pauses`, 之后整个进程不再暂停; Broker 的 isolate 没有
  `stopping`, 停在断点上时要靠它才能 Ctrl-C)、没有客户端时根本不进 (热重载那个 session 设了 `Debugger.setSkipAllPauses`, 所以没人连着时
  `debugger` 不停). 暂停循环在 TryCatch 里派发, 处理器抛的错记一条 warn 后清掉, 不留到继续后的页面代码里. 暂停期间不跑微任务检查点,
  所以页面这边的 `DevToolsPage` 对同步结果当场回复 (只有返回 Promise 的方法等它落定), 否则暂停时 Elements 展不开节点.
  暂停期间这个 web 的 `busyMs` 记 0, Broker 不把它当卡死. 页面在跑长任务时 DevTools 的 `Debugger.pause` 另经 `IsolateHandle::request_interrupt`
  在 JS 里插进去 (`interrupted` 只从队列里取出 `Debugger.pause` 交给 V8). 暂停时窗口停在最后一帧. 现在 V8 只答 Runtime、Debugger、Console、Profiler、HeapProfiler 等它自己的
  域. 没人接的页面错误 (`host.reportError`) 也经 `inspector::thrown` 报给 inspector (`Runtime.exceptionThrown`, 带 Error 本身, 栈在它的
  `description` 里).
  **重载不断开** (#202): Broker 让同一个 web 重来前 (`Web.terminate` 时 `reloading` 为真: `location.reload` 的 `window.reload`、单应用
  acdev dev 的 `devReload`、桌面的 `devReload`) 先调 napi `DevTools.reloading(web 名)`, 端点记 10 秒 (`RELOAD_WAIT`); 期间目标留在
  `/json/list` 里, 它的客户端不因列表变化被关. 旧页面 `closed` 时 `Pages` 见到这个记号就不发 detached, 把会话挂起 (记着客户端开过的域和
  `enable` 参数, 挂起期间收到的消息排队); 同名新页面 `opened` 时清掉记号, 在新 inspector 上重新 `connect`: 开过 Runtime 的先补发
  `Runtime.executionContextsCleared`, 再按原顺序重放各域的 `enable` (负数 id, 回复不给客户端; V8 因此自己发 `executionContextCreated`,
  页面域照常进页面), 开过 Page 的用 `Page.getFrameTree` (id -1) 的回答发 `Page.frameNavigated`, 开过 DOM 的发 `DOM.documentUpdated`,
  再按序投递排队的消息. 等满 10 秒没回来按真的关闭处理 (detached 后关). 桌面里的应用经 `devReload` 是关窗再开新窗口, web 名变了, 仍是新目标.
  接回时先投 `Posted::ExpectHandler`: 新页面还没装处理器的话, 此后 2 秒内到的页面域消息先排着, 装上后在下一个任务里投给它, 2 秒还没装才答
  -32601; 别的时候 (没接回过的会话、Broker 的 isolate) 没有处理器就立即答 -32601. 挂起的会话和页面这边的排队各最多 1000 条, 多出的答
  -32000 "page is reloading".
  **页面这边答的域** (#187): `Page`、`DOM`、`CSS`、`Overlay`、`Network` (`inspector/page.rs` `DOMAINS`, 按方法名点前面的域分) 不进 V8 session,
  经同一个 `Remote` 投成 `Posted::Page`, 在页面线程上交给页面装的处理器 (`host.devtoolsHandler(fn)` → 回 frame id 即 web 名; `fn(session, text)`,
  客户端断开时 `fn(session, null)`), 处理器经 `host.devtoolsSend(session, text)` 把回复和事件发回这个客户端. 页面没装处理器时立即答 -32601 (刚从重载接回时除外, 见上).
  JS 一侧在 acde-host `devtools/`: `DevToolsPage` (只在 `ACDE_DEV` 时由 `worker.ts` 装) 按域分发, 每个客户端一份 `enabled`, 没有自己
  `enable` / `disable` 的域由它记开关, 没实现的方法答 -32601, `emit(domain, method, params)` 只发给开了这个域的客户端, `emitTo(session, …)` 只发给
  这一个; 域是 `DevToolsDomain` `{name, methods, closed?}` (`closed(session)`: 客户端断开). 已有: `Page` (`devtools/page.ts`): `getFrameTree` / `getResourceTree`, 主 frame 的 `id` 和 `loaderId` 是 web 名
  (和上下文 auxData 的 `frameId` 一样), `url` 是页面地址, `resources` 还空着. 加一个域: 在 `devtools/` 写一个 `DevToolsDomain`, 在 `worker.ts`
  `DevToolsPage.install` 的列表里加上; 域名不在 `DOMAINS` 里时再加进去 (不加就仍然交给 V8).
  `DOM` (#190, `devtools/dom.ts`, 页面一侧的取数在 `devtools/pagedom.ts`): 只读的元素树. `getDocument` / `requestChildNodes` (发 `setChildNodes`) /
  `describeNode` 的 `nodeId` 和 `backendNodeId` 都是 render 的节点 id (`WebInternals.idOf`, 和 `acdev inspect` 的 ref 同一个), 只有空白的文本节点
  不列. 哪些节点展开过 (子节点发给了前端) 按客户端 session 各记一份: `getDocument` 只重置自己的, `setChildNodes` 只发给要它的 session,
  断开或 `disable` 时删掉那份, 所以几个前端各看各的、后连上的也拿得到完整的树. `enable` 或 `getDocument` 之后用页面的 `MutationObserver`
  (不让页面看到) 逐个 session 推: 父节点在它那里展开过就推 `childNodeInserted` / `childNodeRemoved`, 只是见过 (它的父节点展开过) 推
  `childNodeCountUpdated`; `attributeModified` / `attributeRemoved` / `characterDataModified` 只推给见过这个节点的 session. 最后一个客户端
  `disable` 或断开时停.
  编辑类方法 (`setAttributeValue`、`setOuterHTML`、`removeNode` ...) 回 -32000 "read only". `resolveNode` 要 V8 的 objectId, 由 `inspector/page.rs`
  `rewrite` 改写成这个客户端 V8 session 里的 `Runtime.evaluate` (表达式调页面上 `Symbol.for('acde.devtools.node')` 的查找函数, 只在 dev 时装),
  回来的 `result` 改名成 `object` (补 `subtype: 'node'`), 找不到是 -32000.
  `setInspectedNode` 也在 `inspector/page.rs` (`set_inspected`) 拦下: 用同一个查找函数拿到节点, 存成 `Global` 包进 `InspectableImpl`, 交给这个客户端
  V8 session 的 `add_inspected_object`, 控制台的 `$0` (带 `includeCommandLineAPI` 的求值) 就是它; 找不到是 -32000.
  查询和搜索 (#201): `querySelector` / `querySelectorAll` 用页面节点自己的 `querySelectorAll` (`pagedom.ts` `query`), 无效选择器回 -32000
  `'<选择器>' is not a valid selector.` (异常在 acde-host 里接住, 不进页面); 结果里前端没见过的节点, 先沿祖先自上而下给没展开过的推
  `setChildNodes` (同 `requestChildNodes`, `DomDomain.#reveal`). `performSearch` 照 Chrome: 元素名、属性名和属性值、非空白文本里含查询串
  (不分大小写; `"…"` 是整串相等; `<x` 元素名开头, `x>` 结尾, `<x>` 相等), 再并上按 CSS 选择器查到的 (选择器无效就只算文本), 按文档顺序;
  不支持 XPath. 最多留 1000 个 (多的丢掉, `resultCount` 是留下的数). 结果按客户端 session 存 (`searchId` 是 `<session>.<序号>`),
  `getSearchResults` 只认本 session 的, 区间越界回 -32000; `discardSearchResults` 或断开 (`DevToolsDomain.closed`) 时删掉.
  `Network` (#188, `devtools/network.ts` `NetworkDomain`): fetch 和 XHR. 数据来自 acde-dom 观察点的 `network` (`NetworkObserver`): `Tracked`
  只在 `recording()` 为真 (有客户端 `Network.enable`) 时把请求头、请求体、响应头和收到的每块数据交给它, 没人开时不记录也不复制. 事件
  `requestWillBeSent` / `responseReceived` / `dataReceived` / `loadingFinished` / `loadingFailed` (失败、中止 (`canceled`)、没有 network 能力
  被拒都带 `errorText`), `requestId` 是 `Tracked.id`, `loaderId` / `frameId` 是 web 名, 时间戳是 `performance.now()` 秒; `responseReceived` 的 `response.timing`
  (`ResourceTiming`): `requestTime` 是请求开始, `sendStart` / `sendEnd` 是 0 (交给传输层即发出), `receiveHeadersEnd` 是收到响应头时相对它的毫秒,
  DNS、连接、SSL、代理、worker 这些没有可靠来源的是 -1. 请求完成后整个
  响应体留着给 `Network.getResponseBody` (文字类型且是合法 UTF-8 回文字, 其余 base64; 没有的回 -32000), 每页最多 16 MiB, 超出时丢最早的;
  单个超过上限的一超过就丢掉已收的块, 不留. 有客户端中途 `Network.disable` 时丢掉进行中的记录, 之后的数据不再上报. #189 补上其余流量:
  WebSocket (`webSocketCreated`、握手请求和 101 回答、`webSocketFrameSent` / `Received` (文字 opcode 1, 二进制 2 base64)、`webSocketClosed`;
  连不上是 `webSocketFrameError` 再 closed, 不发 requestWillBeSent), EventSource (类型 `EventSource`, 每条消息 `eventSourceMessageReceived`,
  响应体不留), 页面加载的图片、样式表、字体 (类型 `Image` / `Stylesheet` / `Font`; 应用包里的文件经 `Tracked.network` 只报给 DevTools,
  dev 日志不变; 渲染器自己从应用包读的 (`<img>`、`<link rel=stylesheet>`、CSS 里的字体: render `net.rs` `LocalNet` 直接读文件, 不经 JS 加载器)
  在 dev 模式下也给加载器发一个 `served: true` 的 `Load` (没有 `done`), src/host `dom.rs` 以 `(-1, url, destination, cors, true)` 调页面的
  加载回调, acde-dom `reportServed` 在有客户端录制时按路径读出文件补报; 非 dev 模式不发). 图片取自文档的图片缓存或并入进行中的请求时不再取,
  blitz fork 经 `NetProvider::reused` 报成 served 且 `cached` (`Load.cached`, 加载回调第 6 个参数): 只报状态和按扩展名的类型, 不读文件,
  `responseReceived` 带 `fromMemoryCache: true`. `<img>` 插入文档时, 当前 `src` 已经开始取过 (成功、失败或还在取, 记在 `ElementData.image_tried`) 就不再取, 和规范一样不重发请求、
  不再触发一次 load / error (`process_added_subtree`); `src` 换了才重新取. 不在文档里的 `<img>` (`new Image()`) 设了 `src` 也加载
  并发 load / error (blitz fork `mutator.rs` `set_attribute`; render `Engine::image` 只在请求进行中时算加载中), 和浏览器一样. 因页面没有 network 能力被拒的 (`NO_NETWORK`) 在 `loadingFailed` 带 `blockedReason: "other"`.
  `Tracked` 的 `frame(sent, data)` / `message(event, data, lastId)` 交给 `NetworkObserver` 的可选同名方法; `exchange` 可以接收调用方建好的 `Tracked`.
  `CSS` (#191, `devtools/css.ts`) 和 `DOM.getBoxModel`: 取数和 `getComputedStyle` / `getBoundingClientRect` 是同一个原生接口 (`dom.computedStyleAll`,
  `dom.rect`, 经 `pagedom.ts`). `getComputedStyleForNode` 是 stylo 算出的全部值; `getBoxModel` 的 border 是 `rect` (变换后的包围盒),
  按算出的 `border-*-width` / `padding-*` 往里缩得 padding / content, 按 `margin-*` 往外扩得 margin (`auto` 当 0), `display: none` 或不是元素回 -32000.
  `getMatchedStylesForNode` 只给 inline style (元素的 `style` 声明, 没有 `styleSheetId`, 不可编辑), `matchedCSSRules` / `inherited` 留空:
  render 还没有可靠的“哪条规则命中”来源, 不编. 编辑类方法 (`setStyleTexts`、`addRule` ...) 回 -32000 read only. 每次选中都现取, 页面改了样式再选就是新的.
  `Overlay` (#192, `devtools/overlay.ts`): `highlightNode` (`nodeId` / `backendNodeId`) 调 DomNative `highlight(id)` (`-1` 去掉), 同一节点不重画;
  `hideHighlight`、`disable` 和画它的客户端断开时去掉 (别的客户端的 `hideHighlight` 也去掉页面经 objectId 画的). 只给 `objectId` 时
  `inspector/page.rs` `rewrite` 改成这个客户端 V8 session 里的 `Runtime.callFunctionOn`, 在那个对象上调页面的 `Symbol.for('acde.devtools.highlight')`
  (只在 dev 时装, 不是本页节点回 false → -32000). `setInspectMode`、`setShowViewportSizeOnResize` 等开关回 `{}` 不做事. src/host `dom.rs` 的
  `highlight` 只在有 `ACDE_DEV` 时转给 render, 别时不做事.
