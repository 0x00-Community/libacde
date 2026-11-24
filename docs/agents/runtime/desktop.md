# 桌面契约: 怎样写一个能换进来的桌面

> 路径相对仓库根. 「运行时」指跑桌面的一方: Linux 会话是 acdes (合成器 acompositor 是它主进程里的一个线程), 调试时是 acdev 的宿主窗口
> (`acdev dev <桌面包>`, 整个桌面合成在一个原生窗口里). 「合成器」指会话里管窗口的那一方, 桌面看不到它.

桌面是一个普通的 acde 包, 用任何 web 前端写都行 (dopame 只是一个例子). 运行时不认识任何具体的桌面; 桌面能依赖的**只有三样**:

1. 包清单 `package.json` 的 `acde` 字段 (类型 SDK `PackageManifest`, 规则 `manifest/`);
2. `acde:` 模块, 类型在 SDK `@acde/api` (桌面用 `@acde/api/shell` 这个入口, 见 `docs/agents/acde-api.md`);
3. 标准 web 接口 (DOM、事件、拖放、`localStorage` …), 和浏览器一样.

合成器的私有 Wayland 协议、acdes 的进程和通道、别的桌面的做法都不是契约, 会变, 不许依赖. SDK 里带 `?` 的方法和事件是**可选的**: 某个运行时
(或某个版本) 没有就是 `undefined` 或调用回 `unsupported`, 桌面要先探测再用, 没有时降级 (下文「各运行时的现状」列出今天缺什么).
本文写行为规则; 机制和实现细节在文中指向的文件里.

## 桌面包声明什么

`acde.desktops[]` 每项一个桌面 (SDK `DesktopManifest`, 校验和找包顺序见 `docs/agents/runtime/packages.md`):

| 字段 | 必填 | 意思 |
| --- | --- | --- |
| `id`、`name`、`description` | `id`、`name` | 登录页、`acde:desktops.list()` 里显示的身份 |
| `entry` | 是 | 桌面页: 包里的 HTML (相对路径, 不以 `/` 起头, 不含 `..`) |
| `lock` | 否 | 锁屏页, 写法同 `entry`; 不写时运行时用自带的锁屏页 (见「锁屏页」) |
| `services` | 否 | 随桌面起停的后台服务名; 写成 `{name, modules, backend}` 时另给这个服务列出它能 `ctx.import` 的外壳模块, `backend` 是它连桌面后台时的角色名 (有了才有 `ctx.backend`) |
| `backend` | 否 | 桌面后台: `{unit}`, 后台进程的 systemd 系统单元模板名, 以 `@` 结尾、不含 `/` (如 `"acdes-session-agent@"`); 管家按 `<unit><UID>.service` 的主进程认它, 只认系统包 |
| `heapMb` | 否 | 桌面页的 JS 堆, 默认 1024, 最小 32 |

同一个包里还要有 `apiLevel` (和运行时的契约级别相同, 否则整个包不加载), 可以有 `apps` (和桌面同包的应用是**系统应用**, 可以声明外壳模块; 不带桌面的包里应用全是 `SYSTEM_APPS` (`manifest/index.ts`, 现在只有社区商店 `community-store`, 同 acdes 的 `registry.rs`) 时也是, acdes 另要求这个包归 root)、
`assets` (同源 URL 前缀 → 包里的目录)、`services` (服务名 → 模块文件). 桌面自己不需要声明 capabilities: 桌面页拿到全部模块.
系统应用的条目可以写 `"sensitive": true` (SDK `AppManifest.sensitive`, F7): 标成敏感应用, 只对带桌面的系统包有效, 第三方包不能自标
(写了的话 acdes 拒收这个应用); 敏感应用怎么对待由 acdes 认定 (见 acdes `docs/agents/manager.md`「敏感应用」), libacde 不看这个字段.

## 谁能拿到哪些模块

按调用者分层注入 (机制见 `docs/agents/runtime/webs.md`「按调用者层级注入」), 没给的模块 `import` 时报 `AcdeError` `forbidden`:

| 调用者 | 拿到 |
| --- | --- |
| 桌面页 | 应用模块 + 全部外壳模块 + 全部系统能力模块 (`fs`、`terminal`、`network`、`power`、`display` …) + 全部服务; **没有 `acde:window`** (桌面页不是应用窗口) |
| 系统应用 (与桌面同包) | 应用模块 + 清单 capabilities 里声明的外壳模块和能力 |
| 第三方应用 | 应用模块 + 声明的能力和服务; 声明外壳模块的启动时拒绝 (403 `shell_module`) |
| 锁屏页 | 只有 `acde:lock` (和 `acde:services`) |
| 桌面服务 | `ctx.import` 只能拿 `services[].modules` 列出的外壳模块, 形状是 SDK `ServiceModuleTypes` (见 `docs/agents/runtime/packages.md`) |

- **外壳模块** (`ShellModule`, 只给桌面一侧): `acde:windows` (所有窗口)、`acde:desktop` (钉面板、窗框主题、桌面事件)、`acde:packages` (重扫包、原生程序)、
  `acde:session` (锁屏、注销、重启、关机、语言)、`acde:desktops` (列出、换桌面)、`acde:permissions` (回答权限询问)、`acde:setDefault`、`acde:inspect`、`acde:software` (系统的 pacman 包: 列出、刷新、装卸社区商店的包、全量更新, 经 acdes 管家交系统总管执行, 见 acdes `docs/agents/manager.md`「软件包」).
- **应用模块** (`AppModules`, 谁都有): `acde:apps` (list / open)、`acde:open`、`acde:window`、`acde:services`、`acde:capabilities`; 要声明的: `acde:launch`、
  `acde:openPath` / `acde:handlers` (`openFiles`) 和各系统能力.

## 桌面页的生命周期

- 会话起来后运行时起桌面页: 一个进程一个页面, 铺满主屏 (左上角那块), 在所有窗口下面; 别的屏只铺壁纸色 (`acde:windows.setWallpaper({color})`),
  窗口可以放上去. 菜单栏、Dock 这类要在窗口上面的东西必须钉成面板 (见「面板与占边」), 画在桌面页里的会被窗口盖住.
- 桌面页**不要自己退出**: 正常退出 (有退出码) 时整个会话以这个码结束; 要注销调 `acde:session.logout()`,
  要换桌面调 `acde:desktops.choose(id)` (运行时停掉应用、服务和旧桌面, 起新桌面, 合成器不重起).
- 崩溃 (被信号结束、连接断了) 时运行时只重起桌面页, 应用窗口不受影响; acdes 60 秒内第 4 次就放弃 (会话结束). 合成器重起时桌面一起重起.
- **重起和重载后从零开始**: 页面开局的窗口表是空的, 运行时把现有窗口逐个补成 `first: true` 的 `window` 快照. acdes 里窗口号是新的 (旧号失效),
  `key` 不变; 所以桌面要跨重载记住的窗口状态 (归属哪个虚拟桌面等) 按 `key` 记, 不按窗口号. 重起时桌面页先要判断是不是「第一次开机」:
  有窗口补报上来就是重起 (补报要过一小会儿, dopame 等 0.5 秒).
- 服务可能比页面晚就绪: 用 `acde:services` 的 `names()` 和 `on('changed')` (`added` / `removed`) 补上, 不要只在加载时取一次.

## 窗口模型

窗口归合成器管, 不在桌面页的 DOM 里. 桌面看到的是一张表和一串事件, 能请求的只有下面这些; 摆放、叠放、焦点的最终裁决都在合成器.
规则细节见 `docs/agents/runtime/window-model.md`.

- **身份**: `id` 是数字窗口号, 本次运行内唯一, 不复用 (acdes 里桌面页重载后换号); `key` (可选) 是合成器给的稳定身份, 重载后不变, 合成器重起后失效.
  应用窗口带 `appId` (acde 应用是它的 id, 原生程序是它自报的 app id, 可能为空)、`instance` (同一应用的第几个实例)、`pid`; `web: true` 是 acde 应用.
- **表和事件** (`acde:windows`): `list()` 是当前的表 (`NativeWindow`, 每行必有 `state`); `on('window')` 是每个窗口每批变化一条完整快照
  (`WindowEvent`: `appId`、`title`、`state`、`previous`、`hidden`、`rect`、`active`、`parent`、`desk`、`first`、`changed` …), `first` 是新窗口
  (或重载后补报的), `changed` 列出这批变了的字段. 另有 `close` (窗口没了)、`crash` (进程崩了; 和 `close` 一样把窗口从表里删掉, 两者都要当「窗口没了」处理,
  且可能先后都到)、`menus`、`currentdesk`. `open`、`title`、`state`、`activate`、`parent`、`desk`、`geometry` 已过时 (由快照推出, 照旧发), 新桌面只听 `window`.
- **Agent 的 stage** (acdes): 快照与 `list()` 的行可带 `stage: true` (变化时 `changed` 里是 `stage`): Agent 起的原生程序的窗口, 不画到任何屏上、
  不拿人的键盘, 也不占虚拟桌面的编号. 桌面可以在任务栏里单独显示或不显示它; `focus(window)` 就是人接手: 窗口移到当前桌面、照常显示并拿键盘,
  之后 `stage` 为 false. 见 acdes `docs/agents/acompositor/desktop.md`「Agent 的 stage」.
  `ready`、`show`、`attention`、`hang`、`responsive` 不保证发 (今天 acdes 都不发).
- **打开应用**: `acde:apps.open(id, request?, {parent, modal, newWindow, root, size})` → `{window, instance?, running?}`. 回答里的 `window` 可能是 0
  (窗口还没出来, acdes 总是这样): 桌面按随后 `first` 快照的 `appId` (有 `instance` 时再对 `instance`) 认领, 认领要能在回答之前或之后发生.
  带请求打开已在运行的单例应用时请求交给它 (`running: true`), 不开新窗口; 要新窗口给 `newWindow: true` (清单 `multiple` 的应用才真开).
  `size: [宽, 高]`  是这次新窗口的页面尺寸, 单位同清单 `size` (内容区的逻辑像素, 窗框另算), 只在真起新进程时用, 已在运行的单例不改;
  管家要两个有限正数 (取整后 1–16384), 否则 400 `bad_request`, 不给或 null 等于没给; 带请求打开 (`open-file`、`open-link`) 同理.
  `acde:openPath(path)` 回 `no_app` (404) 时运行时不弹任何东西, 「打开方式」由桌面自己画 (`acde:handlers` + `acde:setDefault`).
  起原生程序用 `acde:launch.start(argv)` 或 `acde:packages.runNative(id)`, 窗口号同样从快照认领.
- **摆放**: 新窗口由合成器摆 (工作区居中, 按窗口数错开; 有父窗口的在父窗口上方). 桌面可以 `place(id, [x, y, w, h])`
  挪 `normal` 状态的窗口 (宽高大于 0 时也请客户端改尺寸); 坐标是合成器的逻辑坐标, 多屏时按 `acde:display` 的 `bounds` 换算.
- **激活和焦点**: `focus(id)` 就是激活: 先切到窗口所在的虚拟桌面, 取消隐藏, 从最小化恢复到原来的状态, 连同它的模态对话框提到最前, 给键盘.
  激活之外没有「恢复」或「切桌面再聚焦」的组合要做, 每次只调一个 `focus`. 新窗口**不保证**拿到焦点 (用户刚点过桌面或面板时键盘还在桌面):
  桌面开窗后要它在前面, 认领到窗口号后自己 `focus`. 应用不能自己抢焦点: `acde:window.requestActivate()` 可能被拒或只变成提醒, 程序之间转交焦点
  走标准 xdg-activation. 谁有焦点以快照的 `active` 为准.
- **状态**: `normal`、`minimized`、`filled` (铺满工作区)、`fullscreen` (铺满输出), 由运行时裁决 (`fixed` 窗口只能最小化, 面板和桌面页没有状态).
  桌面用 `requestState(id, state)` 改; 窗框按钮、应用自己的 `minimize()` / `requestState()` 也会改. 桌面只照快照的 `state` / `previous` 更新自己,
  不能只记自己请求过什么. 隐藏 (`hide` / `show`, 快照的 `hidden`) 是另一回事: 不画、不命中、不拿焦点, 位置和状态保留, 只有桌面能做,
  用于桌面的全屏视图 (总览、启动台) 临时让开窗口.
- **放在一边** (`setAside(id, on)`, 只有桌面页有): 被放在一边的窗口不进 Super+Tab, 键盘要换窗口时合成器也不挑它, 点它照样拿键盘;
  用于桌面当窗口开的面板. 桌面页进程经原生 `Windows.setAside` 发 `acompositor_window_v1.set_aside` (版本 9 起); 合成器不够新、或没有
  acompositor (acdev 宿主) 时回 `unsupported` (501).
- **关闭**: `close(id)` 是礼貌地请窗口关 (acde 应用先问 `beforeclose`, 3 秒不答就关; 原生程序收 `xdg_toplevel.close`); `kill(id)` 直接结束窗口的进程
  (桌面和会话自己的进程不杀), 给「未响应」时用. 未响应看 `list()` 每行的 `unresponsive` (合成器 ping 5 秒没回), 变化时发 `hang` / `responsive` 事件.
- **父窗口和模态**: 快照的 `parent`; 模态对话框总在父窗口之上, 激活父窗口时焦点落在对话框上. 桌面给父子窗口编组 (任务栏只列顶层) 时看它.
- **应用菜单**: 有桌面时运行时不在窗框上画应用菜单, 应用 `setMenus` 的菜单经 `on('menus')` 交给桌面, 桌面要显示就自己画 (菜单栏、窗口菜单都行),
  选中时调 `invokeMenu(id, menu, item)`. 不画也合法, 只是应用菜单没有入口. 菜单项的快捷键也由桌面决定要不要绑.

## 虚拟桌面

`acde:windows` 的 `addDesk`、`showDesk`、`removeDesk`、`moveToDesk` (可选; 宿主窗口没有, 见「各运行时的现状」):

- **编号归桌面**: 虚拟桌面是非负整数, 一开始只有 0; 别的编号由桌面 `addDesk(n)` 建 (已有就什么都不做), 编号怎么分配、要不要持久由桌面定.
  合成器不知道桌面给它们起的名字和顺序.
- **窗口的桌面**: 快照的 `desk`. 新窗口落在当前桌面; 有父窗口的落在父窗口所在的桌面; `moveToDesk(window, desk)` 连同它的对话框一起挪.
  面板不属于任何虚拟桌面, 每张都在.
- **切换**: `showDesk(n)` 一步换掉整屏, 只画、只命中、只聚焦当前桌面上的窗口. 合成器自己也会换桌面: `focus` 一个别的桌面上的窗口时先切过去,
  所以当前桌面以 `on('currentdesk')` 为准, 桌面页跟着它改自己的界面, **不要**反过来再 `showDesk` 回去 (会和合成器来回打架).
  `currentdesk` 报了桌面不认识的编号 (如重载前留下的) 时可以 `showDesk` 一次回到自己的桌面, 不要反复推.
- **删除**: `removeDesk(desk, into)` 把窗口并进 `into`, 删的是当前桌面时同时切过去.
- **错误** (带 `code`): `unknown_desktop` (编号不存在)、`last_desktop` (删唯一的桌面)、`same_desktop` (`into` 就是要删的)、`unknown_window`、
  `unsupported` (这个运行时没有虚拟桌面).
- 没有这些方法时桌面自己模拟: 按归属 `hide` / `show` 窗口.

## 面板与占边

`acde:desktop.pin(element, {margin?, reserve?, keyboard?, anchor?, userOnly?})` → `PinnedPanel` (机制见 `docs/agents/runtime/webs.md`「钉住的面板」):

- 元素变成单独的面板, 盖在所有应用窗口上面, 但状态和 DOM 还在桌面页这一个文档里 (同一棵组件树, 同一份事件). 元素要是 stacking context,
  不带祖先的裁剪和透明度; 输入区域就是元素的盒子 (`pointer-events: none` 或不可见时为空), 元素变了运行时自动重量, `update()` 立即重量,
  量不到时面板隐藏. 用完 `unpin()` (组件卸载时).
- `reserve: {edge, thickness?}` 让面板在一条边上占一条 (像 layer-shell 的独占区): 工作区 = 输出减去各边占的 (同一条边取最大), `filled` 的窗口跟着改尺寸,
  新窗口避开占边. 不给 `thickness` 时是从边到面板远端的距离, 跟着量的结果变; 要动画、悬停放大不挤动窗口就明确给. 已钉住的用 `reserve(r | null)`
  就地改, 不要 unpin 再钉 (工作区会多变一次). 不写 `reserve` 的面板什么都不占.
- 工作区变了运行时告诉应用 (`acde:window.output.workArea`); 桌面页自己从 `acde:display` 的 `workArea` 取.
- `keyboard`: `'on-demand'` (默认, 点到面板才拿键盘, 菜单栏、Dock) 或 `'exclusive'` (面板显示期间独占键盘, 收起后键盘回原窗口; 弹出菜单、确认卡这类
  用键盘打开、要马上接按键的). 面板拿到键盘之后, 按键交给桌面页的焦点元素: 面板打开时要把页面焦点放进面板.
- 桌面页本身和 `on-demand` 面板一样: 点到才拿键盘.
- `userOnly: true` : 这个面板是只有用户本人能答的卡片 (确认、权限询问; 一般同给 `keyboard: 'exclusive'`), 缺省 false, 不是布尔值抛
  `TypeError`. 运行时把它一路交给窗口系统; acdes 的会话管家只在这类面板显示期间拒绝 Agent 注入的按键和落在面板上的指针 (acdes `docs/agents/manager.md`).
  弹出菜单、启动台、聊天浮层这类普通的 `exclusive` 面板不要给: Agent 要能在它们开着时照常操作 (包括点掉它们). 面板里个别元素只许用户点的, 照旧由
  管家的 `guard` 逐步核对 (`docs/agents/dev/dev-ipc.md`), 不靠这个标记.
- **锚定弹出** (`anchor: {window, x, y, width?, height?, edge?, align?, adjust?}`): 菜单、提示气泡、
  下拉框这类跟着某扇窗里某个位置的面板, 不给屏幕坐标, 只说锚在窗口 `window` (`windows.list()` 的窗口号) 内容区的矩形 `(x, y, width, height)` 上
  (那扇窗页面的 CSS 像素, 宽高缺省 0 即一个点); 合成器按真实窗框和缩放把面板放在它的 `edge` 边 (缺省 `bottom`), 沿边 `align` 对齐 (`start` /
  `center` / `end`, 缺省 `start`), 放不下时 `adjust` 里的 `flip` 换到对边、`slide` 平移进屏幕 (缺省两个都要, 空数组都不要); 跟着父窗口移动和隐藏,
  父窗口关了一起收起. 贴边按元素本身算 (运行时把元素在面板里的位置交给合成器), `margin` 留的阴影位置不让它偏. 页面里元素的位置不再决定屏幕位置,
  元素仍要在桌面页可见范围里布局. 已钉住的用 `anchor(next)` 换锚点 (下拉框跟着输入框滚动). 不能和 `reserve` 同给, 也不能和
  `keyboard: 'exclusive'` 同给 (`TypeError`; 菜单要键盘走 `requestGrab`). 窗口系统不会放锚定面板 (老 acdes、acdev 宿主窗口) 时 `pin` 当场抛
  `NotSupportedError`, 桌面可以退回自己换算坐标.
- **弹出窗口** (`popup: true`, `parent?: PinnedPanel`): 菜单、下拉框、启动台这类“点外面就收起”的弹层, 交给窗口系统做成标准弹出窗口
  (xdg_popup 那一类), 由合成器负责点到外面时收起, 页面不再自己判断点击落在哪. `parent` 是挂在哪个已钉住的面板上 (运行时换成它的面板号交给窗口系统),
  不给时挂在桌面主层面上; 不是这里钉住、或已经结束的面板抛 `TypeError`. 可以和 `anchor` 同给; 不能和 `keyboard: 'exclusive'` 或 `reserve` 同给
  (`TypeError`: 弹出窗口打开时自己拿键盘, 不占边). 被收起时 `closed` 以 `'dismissed'` 兑现, 收尾和 `unpin()` 一样; 一个面板结束时, 挂在它下面的
  面板 (`parent` 是它) 都以 `'parent-closed'` 结束. 窗口系统不会做弹出窗口时 (acdev 宿主窗口、老 acdes) 照常按 `on-demand` 面板打开:
  `PinnedPanel.popup` (只读, getter) 说这次实际是不是弹出窗口, 请求了并真的建成时为 true, 退回时为 false, 这时 `closed` 不会以 `'dismissed'` 兑现,
  点外面收起要页面自己做. 元素要可见、有了位置才真正打开, 在那之前 `popup` 为 false, 打开后按实际结果更新.
- **`closed`**: 每个面板都带的 Promise, 面板结束时兑现: `'unpinned'` (页面 `unpin()`)、`'parent-closed'` (锚定的父窗口关了, 运行时已收起面板,
  页面只清自己的状态)、`'refused'` (窗口系统这次不让锚: 窗口不存在、会话锁着等, 或面板根本没开出来), `'dismissed'` (弹出窗口被合成器收起: 点到了外面)
- 面板里的 `backdrop-filter` 交给合成器模糊下面的窗口 (acdes: 每个面板只取最大的一块).
- 全屏窗口盖住面板所在的边时, 面板仍在最上面: 要给全屏让路, 桌面自己把面板变透明、不接指针 (按前台窗口的 `state`).

## 窗框

- acde 应用窗口 (清单 `frame` 不是 false) 的标题栏由运行时画、命中、拖动, **不在应用页面的文档里**, 也不在桌面的手里. 原生程序自己画 (CSD).
  `frame: false` 的应用自己画标题栏, 由 `acde:window.hostControls` 决定要不要画窗口按钮.
- 桌面只能**声明**样子和行为: `acde:desktop.setFrameTheme(theme | null)` (SDK `FrameTheme`: 活动 / 非活动的颜色、`buttonStyle`、`buttonSide`、
  `titleAlign`、给不支持深色的页面用的 `light`; 每次给整个主题, `null` 回到默认) 和 `setDoubleClick('fill' | 'minimize' | 'none')`.
  未知的键忽略并记 warn, 坏颜色抛 TypeError. 规则见 `docs/agents/runtime/webs.md`「窗框」.
- 窗口的外圆角与阴影也只能声明: `acde:desktop.setWindowLook(look | null)` (SDK `WindowLook`: 圆角半径与 CSS 角形 (`round`、`squircle`、`superellipse(k)`), 聚焦 / 不聚焦两档 CSS 外阴影层),
  由合成器画, 窗口页面自己不再画外阴影和外圆角. 规则见 `docs/agents/runtime/webs.md`「窗框」.
  各键都可省: 只改给了的键, 没给的保持上一次的 (起初是方角、无阴影、`standard`; `setWindowLook(null)` 回到这个状态), 所以可以单独 `setWindowLook({motion})`.
  `look.motion` 选窗口动画的档 (`'standard'` 缺省、`'reduced'`), 只能选档不能给数值; 档的定义只在 `acde_scene::motion` (见 `docs/agents/graphics/render-gpu.md`「动画曲线」),
  经 `WindowLook.motion` (Rust `acde_scene::motion::Preset`, 页面交出的数列里紧跟阴影层的那个数是它的线上编号) 交给合成器.
  `look.titlebarHeight`  是运行时画的窗框标题栏高度, CSS px, 24 到 64 的整数 (小数四舍五入), 由桌面声明 (Dopame 取 uikit 令牌 `--dp-titlebar-h`, 40);
  没给过时用运行时缺省 `Frame.HEIGHT` (32). 它是数列的最后一个数 (可省; Rust `WindowLook.titlebar`, `None` 是没给), 经合成器交给各应用进程的窗口一侧,
  各应用进程的启动器在建窗时问原生 `titlebarHeight()` (没有这个方法的宿主, 如 acdev, 用缺省), 窗框画多高、内容区往下让多少都按它.
  `look.controls` (`{inset, size, gap, caption}`: 红绿灯离边距离、直径、间距, Windows 样式每个按钮的宽度) 和 `look.title` (`{size, weight}`: 标题字号、字重)
  同样由桌面声明 , 各项都可省, 省掉的用运行时缺省 (`FrameMetrics::default()`, `src/render/decoration.rs`: 12、12、8、46, 13、600, 只此一份).
  范围: inset 0–48, size 6–32, gap 0–32, caption 24–80, title.size 9–24, title.weight 100–900. 线上是标题栏高度之后再跟 6 个数 (没给的写 -1, 标题栏没给写 0),
  Rust 进 `WindowLook.frame`; 宿主 (各应用进程的窗口一侧) 拿到后调 `FrameMetrics::set(Some(..))`, 它只改进程里给之后新建窗框用的值: 每个 `Decoration` 建立时存一份 `FrameMetrics::current()` (2026-11-24), 之后画窗框、按钮位置 (`Decoration::dots`、`button_at`、`grab_hints`) 都只用这一份, 所以和标题栏高度一样只对新开的窗口生效, 已开的窗口不会变成旧栏高配新按钮尺寸. `FrameTheme::grab_hints` (没有 `Decoration` 的宿主用) 取当前值.
  mac 样式三个按钮的圆心、半径和颜色由 `Decoration::dots(bar)` 给 (`Dot {button, centre, radius, color}`, 2026-11-24; 颜色同 `Decoration` 画的: 主题的按钮色或红黄绿, 失焦按主题, 最小化不可用时是标题色的 16%), 只此一份;
  宿主能让合成器画玻璃时 (acdes 会话) 调 `Decoration::glass_dots(true)`, `Decoration` 不再画这三个圆点和描边, 由宿主按 `Decoration::dots` 在每个按钮处加一块 clear 玻璃, 着色取按钮色.
  `acde:desktop.motion(preset?)` 同步返回 `{preset, duration, easing}`: `duration` 毫秒, `easing` 是 CSS `linear(…)` 缓动 (24 段采样), 页面要和合成器的动画对齐时用它,
  不另存数值; 不给 `preset` 时是上一次 `setWindowLook` 选的档 (没设过或设成 null 时 `standard`). 不认识的档名抛 TypeError. 取值来自原生 `surface.motion(name)` (`webnative.rs`).
- 想让自己画标题栏的应用跟着桌面的主题, 由桌面自己经服务发布 (dopame 的做法), 运行时不管.

## 快捷键

- `acde:windows.shortcut(combo, scope)` / `unshortcut`: `scope` 是 `'session'` (谁也盖不过) 或 `'desktop'` (应用自己登记的同一组合优先).
  组合写成 `ctrl+alt+t`、`super+k`, 单独的 `super` 是「只按一下 Super」(中间按了别的键就不算). 被占用时登记失败, 但不保证 reject
  (acdes 里被合成器拒绝的只记一行 warn). 命中的键 (连同松开) 不交给窗口,
  桌面收 `acde:desktop` 的 `on('shortcut')` `{window, key, title}`: `key` 是登记时的写法, `title` 装的是作用域 (字段名是历史, 照旧).
- 桌面页重载时它登记的快捷键自动撤销, 重新登记即可. 锁屏期间所有快捷键都不生效. Super+Tab 是合成器自己的 (切到下一个窗口).
- 「菜单开着时 Esc 收起」这类临时快捷键: 打开时登记、收起时撤销 (键盘可能还在原来的窗口里, 页面收不到 Esc).

## 锁屏页

- 锁定的入口只有运行时: 桌面调 `acde:session.lock()`, 或运行时自己按电源策略锁 (空闲、合盖、睡前、logind 的 Lock). 桌面不画「假锁屏」.
- 锁定时运行时**另起一个进程**打开清单的 `lock` 页 (没写时用运行时自带的), 页面挂成锁屏表面: 主屏上是页面, 别的屏是纯色. 锁屏页和桌面页不共享任何状态,
  只用自己的 HTML / CSS / JS; 语言按 `navigator.languages`, 深浅色按 `prefers-color-scheme`.
- 锁屏页只有 `acde:lock`: `user()` → `{name, fullName, avatar}` (`avatar` 是 `data:` URL 或 null); `unlock(password)` 成功后**不用做任何事**, 运行时解锁并结束这个进程;
  失败的错误带 `code` (SDK `LockError`): `auth` (403, 密码不对)、`busy` (409, 上一次还在验证)、`throttled` (429, 带 `retryAfter` 秒). 没有 `acde:lock` 时
  (不是在锁屏时打开的) 页面应禁用输入并说明. 锁屏页不能关机、重启、注销.
- 锁定期间键盘全给锁屏页, 快捷键不生效; 睡前运行时等锁屏页出第一帧才放行睡眠, 所以锁屏页要尽快出第一帧, 不要等网络或大资源.
  锁屏页崩了屏幕保持锁定 (黑底), 运行时限次重起它.
- 调试: `acdev dev --lock` 用假的 `acde:lock` 预览 (见 `docs/agents/runtime/layout.md`「锁屏页预览」).

## 拖放

- 全是标准 HTML 拖放. 从别的窗口 (acde 应用、原生文件管理器) 拖进桌面页或面板的文件, `drop` 时在 `dataTransfer` 里是 `File` (不超过 64 MiB 的普通文件)
  和只读的 `FileSystemHandle` (`items[i].getAsFileSystemHandle()`), **拿不到路径**: 本地文件的 `text/uri-list`、`text/plain` 被去掉. 要在 `drop` 里同步取
  `items`. 桌面要把拖来的东西放到哪里, 读内容后自己写 (见 `docs/agents/web/fs.md`「拖放和打开请求的句柄」「拖放不再授权」).
- 从桌面页拖出去: `draggable="true"` 的元素照标准开始拖动, `dataTransfer` 里的字符串类型交给系统拖放, 别的窗口照常收.
  上一次拖动的 `dragend` (窗口系统的拖放结束) 还没到时又按下鼠标, 页面先把上一次拖动当作没放成结束掉 (源上照发 `dragend`, `dropEffect` 为 `none`),
  再照常开始新的; 之后才到的那条结束在新拖动开始前到达时不起作用, 开始后才到会提前结束页面里的新拖动, 系统拖放不受影响 (js/page `host.ts`).

## 截图

- 桌面页拿着键盘时按 PrintScreen, 运行时截主屏 (不含光标) 存成 `$XDG_PICTURES_DIR/截图 <时间>.png`, 再给桌面发 `acde:desktop` 的 `on('screenshot')`:
  `{path, width, height}` 或 `{path, error}`. 截图之后的提示、预览、打开由桌面做.
- 主动截图: `acde:windows.screenshot?({screen?, window?, cursor?, path?})` (SDK `ScreenshotOptions`), 回 `{path, width, height}` (`Screenshot`), 存成 PNG.
  不给 `screen`、`window` 时截主屏 (`screen` 是屏的序号, 0 是主屏), `window` 是 `list()` 里的窗口号 (只截这个窗口自己的画面), 两个不能同给;
  `cursor: true` 把指针也画进去 (默认不画); `path` 是绝对路径, 不给时存成 `$XDG_PICTURES_DIR/截图 <时间>.png`. 屏或窗口不存在 404, 锁屏时截单个窗口 403 `locked`
  (整屏照截, 截到的是锁屏), 写不进 `path` 400. 桌面页、声明了 `windows` 的系统应用和 `modules` 列了 `windows` 的服务能调; 单个窗口只有桌面页能截
  (只有它的 `list()` 有窗口号). 不发 `on('screenshot')` 事件 (那个只属于 PrintScreen), 提示由调用方自己做.
- acdes 里截图在会话主进程里做 (经管家交给合成器线程, 不经桌面的 Wayland 连接); acdev 宿主窗口只截整个宿主窗口: 给 `window` 或非 0 的 `screen` 回 501.
- 会话外 (如 ssh 进来) 用 acdes 的命令 `acdes screenshot`, 不归桌面管 (见 acdes `docs/agents/architecture.md`「截图」).

## 通知、下载和权限询问

- 应用的标准 `Notification` 交给桌面: `acde:desktop` 的 `on('notification')` (`NotificationEvent`: `app`、`label`、`title`、`body`、`icon`、`tag`).
  有桌面时运行时自己不显示通知, 显示、勿扰、记录都归桌面. 下载完成是 `on('download')` (acdes 还没有: 应用的下载回 501).
- 要用户同意的能力 (今天只有摄像头和麦克风) 经 `acde:permissions` 交给桌面: `on('prompt')` / `on('answered')`, 页面起来时用 `pending()` 补上已经在等的.
  **桌面必须回答** (`answer(id, allow)`): 没人答时应用一直等. 桌面也可以在设置里列出和撤销 (`list`、`revoke`).

## 后台服务

- 桌面清单 `services` 列出的服务随桌面起、随桌面停 (acdes 里每个一个进程, 没有窗口和 Wayland 连接); 崩溃限次重起 (60 秒内第 4 次不再起). 停的时候先断开,
  给 3 秒收尾, 之后结束进程.
- 服务只用 `ServiceContext` 和标准接口, 外壳模块按清单的 `modules` 给. 服务的 `api` 是一组命名空间, 页面以 `acde:<命名空间>` 导入; 应用要在自己清单的
  `services` 里写服务名才能用.
- 页面和服务的起停互不等待: 页面用 `acde:services` 跟上服务的出现和消失.
- 页面传给服务的回调 (订阅等) 在页面释放或关掉时作废; 服务用 `ctx.releaseSignal(回调)` 拿一个 `AbortSignal`, 在它 `abort` 时清掉为这个回调留下的东西 (细节见 `docs/agents/runtime/layout.md`「服务模式」).
- 有角色的服务经 `ctx.backend.connect()` 连桌面后台, 连接由管家代办, 服务不碰后台的套接字和令牌; 后台在不在看 `ctx.backend.up` 和 `onChange` (细节见 `docs/agents/runtime/layout.md`「连桌面后台」).

## 必须和可以

桌面**必须**:

- 有 `entry`, 而且不自己退出; 注销、换桌面走 `acde:session` / `acde:desktops`.
- 把要在窗口上面的东西钉成面板; 需要应用避开的边用 `reserve` 占.
- 照 `window` 快照 (和 `close` / `crash`) 维护窗口表, 状态以运行时报的为准; 按 `key` 跨重载记东西.
- 有 `acde:permissions` 时回答它的询问.
- 可选的方法和事件先探测再用.

桌面**可以** (不做也合法, 只是相应的东西没有入口):

- 任务栏 / Dock / 窗口菜单 (用 `focus`、`requestState`、`close`)、虚拟桌面、应用菜单 (`menus` + `invokeMenu`)、通知中心、截图提示、打开方式对话框、
  窗框主题和双击行为、全局快捷键、自己的锁屏页、副屏壁纸色.

桌面**不能**: 画应用窗口的内容或标题栏, 把窗口放进自己的 DOM, 绕开合成器摆放焦点, 用合成器的私有协议, 读拖放来的路径.

## 各运行时的现状

acdes 一列按 acdes 的代码核对过; 宿主窗口一列取自 `docs/agents/runtime/webs.md`、`docs/agents/runtime/window-model.md` 的记载, 没有逐项实测, 只有标了
「(实测)」的几行在 2026-11-24 用 `acdev dev` 的本地会话 (`docs/agents/runtime/layout.md`「本地会话」) 在 macOS 上试过.

| 能力 | acdes 会话 | acdev 宿主窗口 |
| --- | --- | --- |
| 虚拟桌面 (`addDesk` …)、`setWallpaper` | 有 | 没有 (模拟: `hide` / `show`) |
| `place` | 有 (只挪 `normal` 的窗口) | 有 |
| `kill` | 结束窗口的进程 | 同 `close` |
| `animate` | 立即完成, 不动 | 合成器逐帧插值 |
| `setFrameTheme` | 有: 经管家到每个应用进程 (含之后才起的); 只认桌面页, 系统应用设的只到它自己的窗口 | 有 |
| `setDoubleClick` | 有: 同上; 双击判定用 `ClickSettings` 的默认值 | 有 |
| `setWindowLook` | 有: 只认桌面进程, 经 `acompositor_desktop_v1` 版本 6 交给合成器 (合成器低于版本 6 时不生效) | 没有 (忽略) |
| `on('edge')` (指针到屏幕边) | 不发 | 发 |
| 快捷键的键名 | 字母、数字、F1–F12、方向键、Home / End / PageUp / PageDown / Insert / Delete、Esc、Tab、Enter、Space、Backspace、Print 和几个符号; 媒体键 (`xf86*`) 还不认 | xkb keysym 名 |
| 应用自己的 `acde:window.shortcut` | 回 501 | 有 |
| `windows.stats()` | `{}` | 有 |
| `windows.screenshot()` | 整屏、某块屏、单个窗口, 可带指针 | 只有整个宿主窗口 |
| `acde:permissions`、`capabilities.request` | 回 501 (还没有询问可答) | `permissions.list` / `pending` 回空; `capabilities.request` 声明了的是 `available` |
| 快照的 `rect`、`list()` 的位置 | 有 | 有 (宿主窗口每帧比较后发 `windowgeometry`, 实测) |
| `apps.open`、`windowopen` / `windowclose`、`windowmenus` | 有 | 有 (本地会话, 实测) |
| `apps.openPath`、`open-file`、`handlers`、`setDefault`、借文件夹 | 有 | 有 (本地会话, 实测打开 Markdown、文件夹); 没有 acde 应用时 macOS / Windows 交给系统默认 |

acdes 里系统应用声明的外壳模块现在只有 `session`、`desktops`、`setDefault`、`packages.refresh`、`software` 能用, `windows`、`permissions`、`inspect` 回 501;
要这些能力的界面先放在桌面页里.

## 还没有的

- 应用的 `requestAttention` 转给桌面 (acdes 应答了但不转, `attention` 事件不发).
- 副屏壁纸的图片 (`setWallpaper` 的 `image`, acdes 只记日志).
- 跨进程把别的程序嵌进桌面 (`slots` / `launch.embed`, 回 501).
