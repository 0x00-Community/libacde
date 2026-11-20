# desktop web / window web

每个 web 有自己的 DOM、CSS、JS 堆、render 文档和 surface.

- **渲染 (会话)**: 每个 web 一个 xdg_toplevel, 自己光栅化 (acdebin-system 的 `crates/paint`, 三块轮转的 GBM 画布, 见它的 `docs/window-paint.md`), 经 linux-dmabuf 交给合成器. 窗口 id:
  桌面 0, web 窗口从 `1<<20` 起 (Broker 发), 和原生窗口同一空间. 应用窗口先是居中的 `free` 窗口, 页面尺寸是清单的 `size` (默认 800x600),
  Broker `grant` 时加上窗框的高 (`Frame.windowSize`), 整个窗口不超过屏幕.
- **输入**: 合成器按 z 找到指针下的窗口; acdebin-system 的 `crates/window` 转成 shell 事件; 滚轮的默认滚动和 :hover 在 render 线程上. pointermove 对齐到帧
  (acde-host `host.ts`: 只留最新的, 下一个 `frame` 或别的事件前才派发).
- **记住位置** (`src/places.ts`): 窗口没了时合成器发 `windowgone`, Broker 按应用 id 记最后的 rect (全屏的记进全屏之前的) 到
  `$XDG_STATE_HOME/acde/windows.json` (`ACDE_STORAGE` 为空时只在内存); 下次打开且没有别的窗口开着时摆回 (露出屏幕不到 48 像素或标题栏
  在屏幕上沿之外时照旧居中). 只有桌面能隐藏 / 显示 / 提前 (`acde.windows.hide` / `show` / `focus`); 应用的 `minimize` 由运行时最小化 (见 `docs/runtime/window-model.md`「窗口状态」;
  会话里由合成器执行, 发 `windowstate`).
- **窗框**: 应用窗口 (清单 `frame` 不是 false; 桌面和面板没有) 有一条标题栏, **不在页面的文档里**: `<html>`、`innerHeight`、`100vh`、`resize`
  都只是内容区. 窗口表的 rect、grab hint、backdrop 是含窗框的整个窗口; 页面给的 `setGrabHint` / `setBackdrop` 是页面坐标, 加上窗框高再交出去,
  窗框的 hint (标题栏移动, 四边 4 px / 四角 10 px 改大小) 优先. 会话里是 acdebin-system 的 `crates/window` `decoration.rs` 在页面 surface 上开的同步 `wl_subsurface`
  (`set_window_geometry(0, -H, W, 整高)`, 自己的画布 plane `usize::MAX`); 宿主窗口里见 `docs/runtime/window-model.md`「宿主窗口」. 三个按钮 (样式见 render 的「窗框」, `ACDE_FRAME_STYLE`: 红绿灯或 Windows 的最小化 / 最大化 / 关闭) 按下和松开在同一个上时发
  `framebutton {key}` (`close` / `minimize` / `fullscreen` / `restore`), acde-host 调 `acde.window` 的 `close()` / `minimize()` / `requestMode`.
  标题是 `document.title` 或 `acde.window.setTitle` (没有时用 `label`), 经 surface 的 `setTitle`. 高度只在 acde-host `frame.ts` 的
  `Frame.HEIGHT` (32), 经 `claimSurface(token, {frame})` 交给 surface; 画法在 render 的 `Decoration`. 全屏时窗框照旧在. `acde:inspect` 看不到窗框
  (页面坐标加 `contentOffset`).
- **钉住的面板** (Dock、菜单栏、弹出层): 桌面页面调 `acde.desktop.pin(element, {margin?})` → `{update(), unpin()}`, 元素成为单独的 `pinned`
  窗口, 状态和 React 树还在同一个文档里. `pinned.ts` 量元素 (并上 render 报的绘制范围 `panelmove`; 绘制范围记在当时的视口坐标里, 视口宽高或 `devicePixelRatio` 变了就丢掉重量, 并完截到视口以内是裁边不是平移), 要令牌 (`Shell.grant({mode: 'pinned', rect})`),
  再开一个 toplevel (acdebin-system 的 `crates/window` `panel.rs`; 宿主窗口里是 owner 为桌面的表项), `Dom::set_panel` 让 render 单独画它 (`Frame.panels`). 输入区域只是
  元素的盒子 (`pointer-events: none` / 不可见时为空), 指针送回同一个文档; 主窗口或任一面板有焦点就算 `activate`. 元素变了 (ResizeObserver +
  MutationObserver, 700 ms 后补量; 元素上的 `animationend` / `transitionend` 也补量, 淡入时量到的不透明度 0 不会让输入区域一直空着) 重新量, 量不到时 `hide`.
  要令牌之前就 `set_panel`, 元素从此不画在主窗口里: 面板生效前它哪儿都不显示, 不会出现“看得见、点击却落到下面窗口”的一段
  (桌面页的主窗口在 background 层, 应用窗口盖在它上面); 令牌要不到时取消 `set_panel`, 元素回到主窗口. backdrop-filter 转成 `set_backdrop`.
  `pin(element, {keyboard})`: `'on-demand'` (默认, 点到面板时才给键盘) 或 `'exclusive'` (面板开着时独占键盘, 菜单这类用键盘打开、要立刻接按键的);
  别的值在页面里抛 TypeError. 它经 surface 的 `openPanel(…, exclusive)` 进 `SurfaceCommand::OpenPanel { exclusive }` (C 接口是 `OPEN_PANEL` 的
  `flag`), 由前端决定怎么做 (acdes 的 layer 面: Exclusive / OnDemand).
  `pin(element, {reserve: {edge, thickness?}})` 让面板在输出的一条边上占一条 (像 layer-shell 的 exclusive zone; 不给 thickness 时是从那条边到面板远端的距离,
  跟着量的结果变), 经 surface 的 `reservePanel(id, edge, thickness)` 交给运行时. 宿主窗口 (`State::work_area`) 用输出减去显示着的面板占的边得到工作区,
  面板隐藏、挪动、改尺寸、取消钉住或输出变了时重算 (`refill`): `filled` 的窗口跟着改 rect, 全屏不管工作区; 工作区变了给 Broker 发
  `workarea {x, y, width, height}`, Broker (`broker/output.ts`) 把它放进发给每个 web 的 `screen` 消息和 `WebInit.screen.workArea`
  (`acde:window.output.workArea`). 不写 reserve 的面板什么都不占. 钉住后用句柄的 `reserve({edge, thickness?} | null)` 就地换占的边或厚度 (不用 unpin 再钉,
  工作区只变一次, 中间没有整个输出的一步; `null` 不再占, 不认识的边或负的厚度抛 TypeError). 一个宿主窗口就是一个输出. 会话里 acdes 的 `crates/window` 把面板做成 wlr-layer-shell 的面, `reservePanel` 设它的独占区, 工作区由 winman 算 (输出减去各面的独占区),
  变了经 `winman_desktop_v1.work_area` 报给桌面进程, 铺满的窗口由 winman 跟着改 rect (acdes 的 `docs/workspace.md`). 元素要是 stacking context, 不带祖先的裁剪和透明度.
  `acde.desktop.setDoubleClick('fill' | 'minimize' | 'none')` (SDK `DoubleClickAction`) 声明双击标题栏做什么, 随时可以再设, 下一次双击就按新的;
  不认识的值抛 TypeError. 桌面页面和声明了 `acde:desktop` 的系统应用窗口都有 (经自己的 surface 的 `setDoubleClick`, 整个宿主一份), 服务没有;
  会话里还没有 (只记 debug 日志).
  `acde.desktop.setFrameTheme(theme | null)` (SDK `FrameTheme`: `active` / `inactive` 各 `{background?, title?, line?, buttons?}`, 颜色是
  `#rgb` / `#rgba` / `#rrggbb` / `#rrggbbaa` / `rgb()` / `rgba()`; `buttonStyle: 'dots' | 'glyphs'`, `buttonSide: 'left' | 'right'`,
  `titleAlign: 'center' | 'start'`, `light: {active?, inactive?}`; 每次给的是整个主题, 不和上次的合并) 声明运行时画的标题栏的样子, 运行时照旧画、命中、拖动和双击.
  检查在 SDK `FrameThemes.parse` (`@acde/api/manifest`; 坏颜色和别的取值抛 TypeError; 不认识的键 (各层) 不再报错, 忽略并记一行 warn, #114:
  以后加的可选键在旧运行时上不出错, 但这次修改 (#114) 之前的 acdert 仍会抛, 桌面要先探测), 转成 19 个数交给 surface 的 `setFrameTheme` (整个宿主一份,
  `State.theme`). 设了就重画所有开着的有窗框的窗口 (`Msg::Theme`), 焦点变化时窗口换 `active` / `inactive` (`Msg::Active`); `null` 回到
  今天的样子. 只管合成里的窗口; 会话里还没有 (只记 debug 日志). 谁有它同 `setDoubleClick`.
  **页面不支持深色时用 `light`** (#114): 主题带了 `light` 的配色时, 页面没声明支持深色的窗口改画 `light` 那套 (缺的项用运行时默认, 不借深色主题的);
  声明了的照旧用 `active` / `inactive`. 规则同浏览器: 根元素计算后的 `color-scheme` 含 `dark`, 或根元素没写 (`normal`) 而 `<meta name="color-scheme">` 的内容含 `dark`;
  页面同时声明了 `light` 时, 还要 `prefers-color-scheme: dark` 成立 (系统是深色, 或 `emulate` 成深色) 才算深色, 也就是页面此刻画成深色时.
  **没有主题时** (单独跑的应用, 没有桌面设 `setFrameTheme`) 深色页面的窗口画运行时的深色默认 (`FrameTheme::DEFAULT_DARK`), 别的画浅色默认.
  禁用的最小化按钮按标题文字的颜色取 16% 不透明度 (深色标题栏上也看得见).
  web (acde-host `frame.ts` `Frame.followScheme`, 只给有窗框的应用窗口) 在页面载入后判断一次, 之后只在影响配色的地方变了时 (50 ms 合并一次)
  再判断: 文档节点的子节点、根元素的属性和子节点、`<head>` 的子节点, 以及 `<head>` 每个子元素的 `name` `content` `rel` `href` `media`
  `disabled` 属性和 `<style>` 的文字; 每次判断时按当前的 `<head>` 重新挂. 一个观察器都不带 `subtree`: acde-dom 只要文档里有带 `subtree`
  的观察器, 每次 DOM 变动都要取目标的全部祖先逐个查 (`mutation.ts` `interested`), 改样式会慢约 25% (#206); 也不观察 `<body>`.
  照 CSS 的规则先读根元素计算后的 `color-scheme` (每次判断读一次), 它是 `normal` 时才看 meta (应用自己切到深色时在根上写 `color-scheme: dark`, meta 还是 `light dark`); 另外监听页面 `matchMedia('(prefers-color-scheme: dark)')` 的 `change`. 结果变了才经 surface 的
  `setPageDark(dark)` 交给 hostwin (`Entry.dark_page`, `Msg::PageDark`, 画在 `Bar.dark_page`). 只经 CSSOM (`insertRule`) 改的
  `color-scheme`, 以及 `<body>` 里的 `<style>` 不触发重新判断. 回归基准 `bash scripts/dom-perf/bench.sh` (无头, 有窗框的应用里 1 万个
  节点的创建和改样式, 每边多轮中位数); 设 `BASE_ACDERT` / `BASE_APP` 时与基准版本交替跑, 任一项比值超过 `DOM_PERF_LIMIT` (默认 1.1, 即 #206 的验收线: 不超过基准版本的 110%) 退出码非零. 还没判断时当作不支持. 会话侧待实现 (会话的标题栏还没有主题).
  `acde.desktop.on('edge', fn)` (SDK `ScreenEdgeEvent` `{type: 'screenedge', edge: 'top' | 'right' | 'bottom' | 'left', entered, output}`, #113):
  指针进入、离开屏幕的一条边时各发一次 (角上是两条边各一次), 给桌面藏起菜单栏和 Dock 时用; 只发事件, 不动焦点和输入. 边的判断在 acde-model
  `ScreenEdge::touched` (离边不到 1 个逻辑像素, 且那一侧外面没有别的输出; 边的变化 `ScreenEdge::changes`). 会话里 winman 不报屏幕边 (旧合成器的 `screen_edge` 随 repo-split 删掉, Dopame 没有用它); 宿主窗口 (hostwin `App::moved`) 按宿主内容区算, 指针离开宿主窗口时离开所有边, `output` 是 `host`, 宿主窗口不论是否全屏都发
  (运行时不抓窗口外的输入). 宿主窗口给 Broker 推 `edgeenter` / `edgeleave` (`key` 是边, `title` 是输出), Broker 转给桌面和声明了 `acde:desktop` 的系统应用.
  调试端点 `pointer {x, y}` 把指针移到屏幕坐标 (经 `Shell.inject` 的 `move`, 和真的移动走同一段), 用来模拟指针到边.
- **帧**: `requestFrame` 之后, 上一帧交出去就给下一帧, 两帧之间至少隔最快输出刷新周期的 0.9 倍; 页面没出新帧时退回 wl_surface 的
  frame 回调. `ACDE_WEB_FPS` 指定帧率.
- **通信**: acde-host `remote.ts` 的 `RemoteBridge` 按 Broker 给的能力建代理 (注入的能力直接用 natives), `define` 登记成内置模块
  `acde:<名字>` (`modules.rs` 用 V8 合成模块, 导出 `{<名字>, default}`). 事件不派到 `window` 上: 页面用模块上的 `on(类型, fn)` 订阅 (返回退订
  函数, 不认识的类型抛 TypeError, 文字是 `acde:<模块> has no <类型> event (<有的类型>)`; `acde:window` 也一样, `WindowSelf` 里的事件名表按 SDK `WindowEventName`
  逐个写, 少一个多一个都编不过, 另加 `shortcut`). 事件和结果都有 SDK 类型 (`types/events.d.ts`、`types/office.d.ts`): 事件按 `type` 区分 (`acde:windows` /
  `acde:launch` 的 `on(短名)` 收 `{type: 'window<短名>', window, …}`, `appschanged`、`download`、`notification`、`screenshot {path, width, height}` 或
  `{path, error}`、`shortcut {key, title: 作用域, window}`、终端的 `output` / `cwd` / `exit`); `acde.window` 的 `grab {kind, granted}`、
  `grablost {kind: 'lost', reason}`, `suspend` / `resume` 没有参数 (运行时还不发). 公共事件、请求和结果里没有 `unknown` 和 `[字段: string]: unknown`
  (SDK `test/types.test.ts` 遍历检查, 也查第 0 级的无类型重载和旧事件都不在了). 产生这些事件的地方用 `satisfies` 对 SDK 类型. 其余调用经 `rpc.ts` 到 Broker (错误原样回来, 回调函数可以跨). 各文档里的
  `acde.<名字>.<方法>` 指 `acde:<名字>` 导出的值.
  - **窗口状态和输出 (页面)**: `acde.window.state`、`requestState(state)` (不行就什么都不变, 没有回答)、`on('state', {state, previous})`.
    状态由运行时决定, 页面没请求的变化 (桌面最小化、窗框按钮、给最小化的窗口焦点) 也发 `state`, 页面必须照事件更新自己, 不能只记自己请求过什么.
    `acde.window.output` 是窗口所在输出的 `{width, height, scale, workArea}` (逻辑像素), 变了发 `on('output')`; 合成的桌面里所有窗口一样
    (Broker 的 `screen` 消息带上 `scale`, `WebInit.workArea` 没给时是整个输出). 窗口位置只是相对当前输出的提示, 应用接口里没有绝对屏幕坐标.
    `acde.window.hostControls` 为真时宿主 (平台) 提供窗口控件, 页面应藏起自己的最小化 / 最大化 / 关闭按钮.
    `requestActivate` 可能被拒或变成提醒用户 (attention), `requestGrab` 可能以 `denied` 拒绝, `animate` 可能立刻做完 (照样发 `windowanimated`).
    桌面 / 系统应用经 `acde.windows.requestState(id, state)` 改任何窗口 (不认识的状态 400, 会话里 501), 收 `on('state')`; `list()` 的每一行都带 `state`
    (SDK `NativeWindow.state` 必有; web 侧 `remote.ts` 的 `WindowList.deliver` 按 `window` 快照 (和 `windowclose`、`windowcrash`) 维护行, 先发 `window`, 再按快照的 `first` / `changed` 推出已过时的 `open` / `title` / `state` / `activate` / `parent` / `desk` / `geometry` 照发,
    没收到过 `windowstate` 的窗口是 `normal`, 换标题不丢状态; `key` 是桌面页重载后也不变的窗口键 (winman window v4 的 identifier, acdes 原样转成 `key`),
    `window` 快照和 `windowopen` 带了就记在行上, 后来的事件不带时保留, 推出的 `open` 也带上; 合成器不给时没有这个字段), `acde:inspect` 的窗口 (driver 用的) 带 `state`.
  - window web: 清单给的服务和能力; `acde.window` (标题、菜单、关闭、最小化、`requestMode` / `setGrabHint` / `setBackdrop` /
    `requestActivate` / `requestGrab` / `releaseGrab`、app 级 `shortcut(combo)`; `beforeclose` 返回 false 留住窗口; `request` 是别的应用
    `acde.open(<id>, {…})` 要它打开的东西, 要对方清单的 `open` 允许, 为一个请求开的窗口把它交给第一个 `request` 处理函数, 只给一次;
    `acde.window.id` 是自己的窗口号). **系统请求** (SDK `SystemRequest`): `{kind: 'open-link', url}` 和 `{kind: 'open-file', url}`
    (`url` 是 file URL 或绝对路径). `acde.open(request)` 不写目标时运行时挑应用 (`open.ts` `Opener.request`): 链接先给清单 `schemes` 里有这个
    scheme 的应用, 再按今天的 `x-scheme-handler` 默认和原生程序, http / https 最后交给系统默认程序; 文件按 `mimeTypes` 和今天的默认. 要调用方声明
    `openFiles` (桌面不用). `acde.open(<id>, request)` 写了目标时要 `open` 允许, 目标不接受 (链接的 scheme 不在 `schemes`, 文件的类型不在
    `mimeTypes`) 时 409 `not_accepted`. 收到的请求带 `kind`: 链接是 `{kind: 'open-link', url}`, 文件保留今天的字段 (没有 `fs` 的应用
    `{kind: 'open-file', url}`, 有 `fs` 的 `{kind: 'open-file', root, path, file}`). 不带 `kind` 的 `open(<id>, {…})` 回 400 `untyped_request`.
    **私有请求** (SDK `AppRequest<K, Fields>`: 接收方声明自己收的 `kind` 和字段, `on<Task | Page>('request', …)` 按 `kind` 收窄; 和系统请求合成
    `WindowRequest`): `acde.open(<id>, {kind, …})`, 运行时只查目标清单的 `requests`
    有这个 `kind` (没有时 409 `not_accepted`, 不交给对方), 内容由接收方自己查; `open` 照旧决定能叫哪些应用. 目标没写 `requests` 就什么私有请求都不收.
    `open(<id>, request, {parent, modal})` 给新窗口一个父窗口 (`broker/family.ts`: 应用只能写自己应用的
    web 窗口, 桌面可以写任何开着的窗口; 不存在 404, 别人的 403, 不是窗口号 400); 新窗口居中在父窗口上方, 不用记住的位置; 已在运行的单例应用只收请求,
    不改父窗口. `{newWindow: true}` 要求给已在运行的应用再开一个窗口, 回答的 `running: true` 表示应用本来就在运行; 页面运行时把这些选项原样交给会话管家,
    什么时候真的另开由管家 (acdes) 定. 父窗口、模态记在合成器的窗口表 (hostwin `Entry`、会话 model `Window`), `acde.inspect` 的窗口带 `parent` / `modal`;
    应用拿不到 `acde:windows`; 起原生程序和嵌进 slot 用 `acde:launch` (`start` / `embed` / `close`, 要清单的 `launch` / `slots`).
  - desktop web: 全部服务; `acde.apps` (`list` / `open`), `acde.packages` (`refresh` / `install` / `natives()` / `runNative(id)`), `acde.launch`; `acde.windows` (所有窗口, 加 `kill`、
    `invokeMenu`、`stats`、`focus` / `hide` / `show`、`animate`、session / desktop 级 `shortcut`); `acde.session`; `acde.inspect`. 另收
    `windowmenus`、`windowready`、`windowcrash {reason}`、`windowhang` / `windowresponsive`、`windowshow`.
  - 不在清单里的调用拒绝 (403, `forbidden`).
  - **按调用者层级注入** (`broker/tier.ts`): 外壳模块 (`SHELL_MODULES`: `windows`、`packages`、`session`、`desktops`、`inspect`、
    `desktop`、`setDefault`) 只给桌面页面、声明了它的桌面服务和声明了它的系统应用 (与某个已安装桌面同包的应用); 应用模块 (`APP_MODULES`: `launch`、
    `apps` 只有 list / open、`open`、`openPath`、`handlers`、`window`) 给所有应用. 声明外壳模块的第三方应用启动时拒绝 (403, `shell_module`).
    没给的模块经 `host.refuseModule` 记下原因, import 时报模块名和缺的声明. 哪个模块要哪条声明只在 SDK `Manifest.importRule(name, app, origin)`
    (`{allowed, missing, reason, unverified}`, app 为 null 是桌面; acde-host 的 `Tiers` / `CapabilityCheck` 和 acdev 的检查都用它).
    **服务的命名空间** (不是系统模块的名字): 运行时按服务名授权, 应用列了 `clock` 就拿到这个服务 `api` 的每个键 (`DesktopServices.start` 把每个键记到服务名上,
    `ServiceCalls.#allowed` 先把命名空间换成服务名再查 `app.services`), 而命名空间是工厂函数的返回值, 静态规则看不到. 所以规则是: 名字在 `app.services` 里 → 许;
    不在但应用列了别的服务 → 许, `unverified` 带一句说明 (`acde:timer resolves only if the service <应用> lists ("clock") provides the "timer" namespace`,
    acdev 把它打成 `note:`); 一个服务都没列 → 拒绝, `missing` 是 null (缺的是提供它的那个服务的名字, 规则不知道), 原因让人写服务的名字
    (`acde:timer is not a system module; if a desktop service provides it, list that service's name in the services of <应用>`; 拼错的模块名也落在这里), 不再让人把命名空间当服务名写进去
    (那样应用要求一个不存在的服务, 启动时 `needs "timer" (service not running)`). 别的名字 `unverified` 都是 null. 新的 `acde:packages` (refresh / install / natives / runNative)
    和 `acde:launch` (`start` 要 `launch`, `embed` 要 `slots`, `close` 只关自己起的窗口: 按窗口的 pid 认 `start` 起的, `embed` 走 claim) 桌面也有.
    系统应用拿到 `acde:windows` 时也收全局窗口事件 (`Tiers.listens`). **窗口列表靠快照补齐**: 启动器不填 `WebInit.windows`, 页面的 `WindowList` 开局是空的. 窗口客户端连上 winman 时把现有窗口逐个报成 `first: true`
    的 `window` 快照. 启动器 (`js/launcher/src/main.ts` 的 `#toDesktop` / `#catchUp`、`app/events.ts` 的 `WindowSnapshots`) 在桌面页线程没起来或正在重载时不丢事件:
    - acdes (`native.Windows`): 进程内重载时旧页面的 Surface 放掉、窗口线程结束, 新页面 claim 时重新绑 `winman_desktop_v1`, winman 重报全部窗口, 窗口号是新的 (整个进程往上数, 不复用),
      `key` 不变, 旧号失效. 所以启动器不重放: 首次起来前的事件按序暂存; 重载期间到的 `window` / `windowclose` / `windowcrash` (旧号) 丢掉, 其余暂存; 线程起来后交暂存的, 再等 acdes 重报.
    - acdev 宿主窗口 (`native.Shell`, 不重报): 启动器记着每个窗口的最新快照, 线程起来后先把开着的窗口按 `first: true` 重放 (`replay()`, 关掉的、崩溃的不算), 再交暂存的其余事件.
    所以页面重载后 `list()` 和事件里的 `key` 都齐. 每个包都按这里注入 (没有 `apiLevel` 的包根本不加载).
    SDK 类型分层: `@acde/api/app` 只声明应用层, `@acde/api/shell` 再加外壳层; 包根声明全部.
- **原生应用** (`launcher.ts`, 只有桌面和声明了 `packages` 的系统应用能调): `acde.packages.natives()` 列可见的 `.desktop` (`{id, name, comment, icon (绝对路径, 按 hicolor /
  breeze / Adwaita 找), keywords, wmClass}`, 缓存到 applications 目录的 mtime 变了), `runNative(id)` 用 `gio launch` 起.
- **应用清单** (`acde.apps`, 解析 acde-host `apps.ts`, 校验 acde-api `validate.ts`): `id`、`label`、`icon`、`entry` (默认导出 `WindowApp` 的
  模块, 或 HTML)、`capabilities` (只放系统能力: `SYSTEM_CAPABILITIES` 加外壳模块; 不认识的名字和服务名都是错误)、
  `optional` (capabilities 里可以缺的几项, 每项也要在 capabilities 里)、`services` (用到的桌面服务名, 不能是 `RESERVED_NAMES`)、
  `mimeTypes`、`schemes` (打开哪些 URL scheme 的链接, 小写不带冒号, 不能是 `file`)、`requests` (接受哪些私有请求, 小写词, 不能是 `open-file` / `open-link`)、`heapMb` (默认 256, 最小 32)、`size` (页面尺寸, 默认 800x600, 窗框另算; 单应用模式的主窗口铺满宿主窗口, 设了 `size` 时它是宿主窗口内容区的初始尺寸)、`multiple`、`frame` (默认 true)、`backend`.
- **必需与可选能力** (`broker/access/capabilities.ts` `CapabilityCheck`, 由 `Launchable` 用): 声明的能力 (capabilities 和 services) 默认都是
  必需的, 逐个查状态: `available`、`not-declared`、`unsupported` (平台表不支持或运行时没开起来, 如 fs 的 Finder、office)、`service-not-running`、
  `shell-only` (第三方应用的外壳模块), 要批准的能力 (见「批准」) 另有 `not-asked` (还没问过用户)、`asking` (提示已经弹出, 在等回答)、`denied` (用户拒绝或撤销);
  要批准的能力只有批准了才是 `available`. `reason` 是这个状态的一句话 (`not asked for yet`、`waiting for the user's answer`、`denied by the user` …).
  必需的既不是 `available` 也不是批准的那几种状态 (`not-asked` / `asking` / `denied`, 使用时才问, 被拒只让调用失败) 时应用不进 `acde:apps` 列表和打开方式, 启动报
  `<label> needs "network" (not available on Windows host windows); list it in "optional" to run without it` (平台缺的系统能力写出平台和运行方式,
  #148; 404 `unsupported_app`; 有服务没在跑时 503 `service_not_running`, 原因照 `reason`).
  `acdev dev` 重载时新清单要的能力不可用: 记一行这个错误, 开着的窗口留在旧版本, 会话不退出, 改好清单再存就照常重载. 可选的照常启动;
  声明了但不可用的 fs / terminal / network / office 在 import 时报原因.
- **平台支持表** (SDK `src/runtime/platforms.js` 的 `PLATFORM_SUPPORT` / `platformSupport(platform, mode)`, 从包根和 `/manifest` 导出, 运行时、acdev 的
  平台检查和本仓文档都以它为准): 按 `process.platform` × 运行方式 (`session` Linux 会话、`desktop` 完整桌面的宿主窗口、`standalone` 单应用宿主窗口) 给
  `unsupported` (不支持的能力) 和窗口特性 `window.backdrop` (`full` / `approximated` / `unsupported`). 现在: Linux 宿主窗口缺 `slots`; Windows 缺
  network、office、camera、microphone、slots; macOS 缺 network、office、slots; 窗口背景滤镜都是 `full`. Linux 会话和 macOS 的行没有实测.
- **`acde:capabilities`** (总是注入所有 web, 桌面和应用窗口都有, 服务没有; 类型 `AcdeCapabilities`): `level` (SDK 的 `API_LEVEL`)、`platform`、`mode`、`window`
  (窗口特性)、`get(name)` / `list()` (所有系统能力、`WEB_CAPABILITIES` 加声明的服务, `{name, kind, state, reason, optional}`)、`request(name)` (状态是 `not-asked` / `asking`
  时经 rpc 问用户, 已经在问的并进同一个提示, 回答之后的状态; 别的直接回当前状态, 不问). 快照在 web 起来时由 Broker 算好放进 `WebInit.tier.capabilities`, 之后 Broker 发 `permit`
  消息更新 (`{capability, approval, status}`): 提示弹出时 (`Permissions.ask` 调 `changed(…, 'asking')`) 变 `asking`, 回答或撤销后变 `available` / `denied`, `get` 和 `list` 都跟着变.
  应用按状态做事: `not-asked` 调 `request`, `asking` 等, `denied` 告诉用户, `available` 直接用.
- **批准** (`broker/access/approval/permissions.ts` `Permissions`, SDK `APPROVAL_CAPABILITIES`): **仅 `camera`、`microphone` 需要用户同意** (用户决定 2026-11-11, 同浏览器:
  第一次 `getUserMedia` 时问), 可撤销; 其余能力 (`launch`、`slots`、`terminal`、`network`、`fs.roots` 点名的公共位置、自己的窗口、能力查询、列出和打开应用、
  按类型打开、桌面服务、`office`) 声明即有, 不问 (#146).
  系统应用 (和任一已装桌面同包) 不问. 启动时不问, 必需的和可选的一样 (同浏览器): 第一次 `getUserMedia` 或 `acde:capabilities` `request(name)` 时问,
  是同一个提示 (`reason: 'request'`); 回答「否」、还没答或撤销都只让这些调用失败 (`NotAllowedError` / 403), 应用照常启动、照常在列表里
  (`CapabilityCheck.missing` 不把 `not-asked` / `asking` / `denied` 算缺); 平台不支持的不问, 照旧按 unsupported 拒绝. 没批准的模块不定义, 批准后 web 收 `permit` 再定义, `load()` 拿得到.
  **没批准时的话按当时的状态说** (`Permissions.approval` 四态 `granted` / `not-asked` / `asking` / `denied`, 文字只在 `Permissions.reason`; Broker 自己的 `permit` 仍是
  `granted` / `denied` / `ask` 三态, 管该不该问): import 报 `acde:terminal: "terminal" is not asked for yet; ask for it with acde:capabilities request("terminal")`、
  `… is waiting for the user's answer` 或 `… is denied by the user` (`Permits.reason`, 一个模块有几道门时 (`acde:launch` 的 `launch` / `slots`) 每道没批准的各说一句;
  状态变了 `RemoteBridge.permit` 重新 `refuseModule`, 所以拒绝之后的 import 说的是拒绝); 调用报 `AcdePermissionError` (403, 带 `capability`),
  `code` 是 `denied`、`not_asked` 或 `asking`, 文字是 `<label>: ` 加同一句: `acde:fs` / `acde:office` 里还没问过的公共位置 (`FileAccess.guard`)、
  `acde.launch` / `acde.windows.launch` (`Approvals.allow`)、撤销后的 terminal / network 都走这里 (camera / microphone 走标准接口: `getUserMedia` 自己弹提示, 见 `docs/web/web-apps.md`「摄像头和麦克风」). 决定按应用持久 (`$XDG_CONFIG_HOME/acde/permissions.json`, `{"<app>:<能力>": "granted" | "denied"}`, 不看 `ACDE_STORAGE`);
  拒绝也记 (不影响列表), 桌面收 `appschanged` (`acde:apps` 的 `on('changed')`): 每次决定变了 (`Approvals` 的 `changed`) 都经
  `broker/listing.ts` `AppListing.changed()` 比一次, 列表和桌面上次拿到的 (桌面页面起来时、`list()` 的结果或上一次事件) 不一样才发; `refreshApps` 也走它 (清单变了一定发). 撤销 (`revoke`) 删掉记录: 这一次运行里当作拒绝 (同一次运行不问第二次), 下次运行再问. 撤销后的调用报同样的
  403 `denied`: Broker 侧 (`acde.launch` / `acde.windows.launch`) 当场查, web 侧 terminal / network 的模块和 camera / microphone 的 capture 经
  `broker/access/approval/permits.ts` `Permits` 包一层. 外壳模块 `acde:permissions` (桌面和声明它的系统应用): `pending()`、`answer(id, allow)`、`list(app?)`、
  `revoke(app, capability)`、`on('prompt' | 'answered')` (`permissionprompt {id, app, label, capability, reason: 'request'}`、
  `permissionanswered {id, app, capability, allowed}`). 用户选给应用的文件夹 (#86) 也在 `list` 里: 每个一行 `{app, label, capability: 'fs.folder-<令牌>',
  decision: 'granted', folder: {label, exists}}` (只给文件夹名, 不给路径; Broker 经 napi `Finder.folders(app)` 读授权文件), `revoke(app, 'fs.folder-<令牌>')`
  经 `Finder.revokeFolder` 删掉授权 (回有没有删到) 并给这个应用的 web 发 `permit` 拒绝: 它离开 `roots()`, 以它为 root 的调用 403 `denied` (`FileAccess.guard`).
  进程里所有 `Workspace` 和 `Finder` 共用一份授权 (`fs/mod.rs` 的 `GRANTS`), 撤销对正在运行的 web 立刻生效. 页面里已有的这个文件夹的句柄活到页面结束;
  同一个页面撤销后再选同一个文件夹, `acde:fs` 仍报 `denied` 到页面重开. 开发、无头和 driver 照常问: dev 请求 `permissions` (待答和已有决定)、`answer {prompt, allow}`、`revoke {app, capability}` (#95, 和 `acde:permissions.revoke` 同一份记录和效果,
  回 `{app, capability, revoked}`, 没有决定过的回 `revoked: false`; 桌面和 `<运行时> app` 都能用)
  给 acdev 用 (driver、MCP, 和命令行的 `acdev permissions` / `allow` / `deny` / `revoke`); 每个提示记一行 info (`waits for the user to allow … answer it with acdev allow or acdev deny, …`: 这一行出现在 `acdev dev` 的输出里, 是没有对话框的会话里用户和 agent 知道该做什么的地方), 没人答就一直等, 不自动批准.
  **单应用模式的原生对话框** (`<运行时> app`, 宿主窗口; `broker/access/approval/dialog.ts` `ApprovalDialogs`, 由 `broker/approvals.ts` 接上): 提示同时交给宿主的
  `approve(title, message)` (hostwin `Cmd::Approve`, Windows 是 `platform/win32/approve.rs` 的 `MessageBoxW`, 以宿主窗口为 owner, 是 / 否, 默认否),
  一次只开一个, 其余排队; 答案经 `Permissions.answer` 写进同一份 `permissions.json`, 和桌面答的一样. driver 或别处先答了, Broker 调
  `dismissApproval` 关掉还开着的对话框 (它不算回答), 排队的直接丢掉. macOS 是宿主窗口上的 `NSAlert` sheet (`system/approve.rs`, 「Don't Allow」在前、是默认,
  「Allow」), `dismissApproval` 经 `NSOperationQueue.mainQueue` 回主线程 `endSheet` (`NSModalResponseAbort`, 回 null); Linux 是 `org.freedesktop.Notifications`
  的常驻通知 (urgency critical, 不过期), 带 `deny` / `allow` 两个动作, 关掉或点了别处回 null, `dismissApproval` 调 `CloseNotification`; 通知服务没有
  `actions` 能力、没有会话总线或 5 s 内不应 (`CALL_TIMEOUT`, 只管方法调用, 不管等用户) 时回 null. headless 没有对话框 (`approve` 回 null); 回 null 的提示照旧等 driver 或 `acdev allow` / `acdev deny`.
  `devReload` 对主窗口用 `Launchable.app` 重新查一遍 (应用没了、级别不对、平台不支持、服务没在跑各报各的话), 查不过时结束会话, 那行错误是
  `Launchable.ended`: `<运行时> app <id>: <原因>`. **`<运行时> app` 里以前的拒绝会再问一次** (`PermissionsHost.asksAgain`, 单应用模式为真): 记录里是
  `denied`、这一次运行还没问过的能力, `permit` 当作 `ask`, 问的时候删掉旧记录并记一行 `was denied … in an earlier run: asking again`; 这一次运行里答了「否」照旧不问第二次.
  单应用模式是开发者点名要跑这个应用, 又没有桌面的权限界面; 不这样的话拒绝一次之后这个应用在 `acdev dev` 里再也问不到, 而 `acdev revoke` 要有会话才能用, 只剩手改 `permissions.json`. 桌面和会话里记下的拒绝照旧有效.
  `icon` 是 `ICON_NAMES` (`@acde/api/manifest`, 和 Dopame art 引擎 `APPS` 的名字一致, 加名字两边一起改) 之一, 或 assets 下以
  `.svg` / `.png` 结尾的同源 URL; 运行时原样交给桌面 (`acde.apps.list()`、`windowopen` 的 `icon`).
- **沙箱**: 每个 web 是独立的 isolate, 页面和 acde-host 的 web.js 在同一个 realm. ⚠ 这不是安全边界: 页面碰得到宿主给 web 的原生接口
  (`sys`: 环境变量、读写文件、spawn), 能 import `file:` 模块; 系统调用级的沙箱 (Landlock / seccomp) 还没加.
- **内存和生命周期**: 堆超了只停这个 web (窗口的发 `windowcrash {reason: 'memory'}`, 桌面的重起). 关闭先问 `beforeclose` (3 秒不答就关),
  再停线程、放掉 surface, 它起的原生窗口一起关. 当前任务超过 5 秒报 `windowhang`, 回到循环报 `windowresponsive`, 桌面可以 `kill`.
  desktop web 停了 Broker 重起它 (一分钟最多 3 次). 隐藏的窗口不出帧、画布放掉; 隐藏久了丢弃还没有.
- **调试**: 每个 web 有名字 (`files#1048576`, 也是事件循环和日志里的名字); 运行中的会话用 `ACDE_DEBUG_DIR` 和 acdebin-system 的 `packaging/qemu/drive.sh` (见它的 `docs/qemu.md`).
