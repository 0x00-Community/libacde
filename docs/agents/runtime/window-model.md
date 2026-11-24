# 窗口模型与协议

框架只有两个抽象: **canvas** (工作区) 和 **window**. 都是 Rust 的表项 (src/model), 不映射到 DOM / JS 对象; 窗口的内容只是客户端交的
一块缓冲, 框架不跑 JS、不持有文档. 桌面、面板、菜单都是窗口.

- **canvas**: 屏幕只有一块, canvas 可以有多个, 同时只显示一个. `rect` 是屏幕的逻辑尺寸 (也是裁剪范围), `scale` 是 HiDPI 缩放, 创建后都
  不变 (分辨率或缩放变了就重建). 每个 canvas 一个 z 列表和一个焦点.
- **window**: `{id, canvas, mode, rect (canvas 坐标, 可以超出 canvas, 超出的不画), z, content, input, grab hints, parent / modal, focus}`.
  四种模式: `fullscreen` (铺满 canvas), `free` (位置尺寸都能变), `fixed` (只能挪), `pinned` (都不变). 桌面是 z 最低的 `fullscreen`
  窗口; 面板和菜单是 `pinned`, 永远在 `free` / `fixed` 上面. 标题栏、边框、阴影是客户端的内容 (客户端画), 不是框架的概念.
  第 1 级契约里模式只管能不能挪、改尺寸 (`free` / `fixed` / `pinned`, SDK `WindowMode` 不再有 `fullscreen`); 全屏是**窗口状态**.
- **窗口状态** (src/model `state.rs`: `WindowState` + `Restore`, 宿主窗口的 `Entry.state` / `restore`, `State::change_state`): `normal`、`minimized`、
  `filled` (铺满工作区)、`fullscreen` (铺满输出); 隐藏仍是框架另一个状态. 每次变化都由运行时裁决:
  `fixed` 只能最小化, `pinned`、桌面和面板一律拒绝, 同一状态不算变化. 离开 `normal` 时记下原来的 rect, 进全屏时记下之前的状态 (`normal` 或 `filled`),
  最小化时记下之前的状态; 「回去」(`Restore::back`) 用它们, 所以 normal → filled → 全屏 → 回到 filled → normal 还是原来的 rect. 最小化的窗口不画、不命中,
  有焦点时焦点给下一个; 给它焦点 (`focus` / `show`) 就回到之前的状态. 每次变化给窗口自己发 `state {state, previous}`, 给 Broker 发
  `windowstate {window, state, previous}` (旧的 `windowfullscreen` / `windowunfullscreen` / `windowminimize` 已随第 0 级去掉). 宿主窗口的窗框按钮
  (最小化、全屏 / 还原) 由运行时直接改状态, 只有关闭交给页面. `requestMode` 只收 `free` / `fixed` / `pinned` (`fullscreen` 抛 TypeError,
  全屏用 `requestState`); 改模式时当前状态在新模式下不允许就回到 `normal`. 独立应用的窗口 (`<运行时> app`) 内部仍是 `fullscreen` 模式, 页面改不了.
  会话 (acdebin-system 的 `crates/window`) 把请求映射成标准 xdg_toplevel 请求 (`set_fullscreen` / `set_maximized` / `set_minimized` / 取消), 按 configure 里的
  fullscreen / maximized 给页面发 `state`. winman 执行四种状态; 桌面的 `acde:windows.requestState` 经 acdes 的
  `native.Windows.setState` 发 `winman_window_v1.set_state`, 变化由 winman 报回, acdes 窗口客户端给桌面页发 `window` 快照 (见 webs.md). `filled` 是 xdg 的 maximized, 铺满工作区 (见 `docs/agents/runtime/webs.md`「钉住的面板」); `fullscreen` 铺满输出. 只有 `normal` 的窗口能拖动、拖边改尺寸;
  输出或工作区变了, 铺满的窗口跟着改 rect; `windowgone` 报的是 `normal` 时的 rect. 还没显示就要铺满 / 全屏的客户端 (Firefox 记着上次是最大化)
  在显示时照做, 这样的窗口没有 `normal` 的 rect: 回到 `normal` 时让客户端自己定尺寸, 等它确认离开铺满 / 全屏后的第一次提交再放到中间
  (之前那一帧还是铺满的尺寸). 最小化的窗口藏起, 桌面 `focus` / `show` 它时回到之前的状态. 页面的 `minimize()` 在会话里走自己的
  `set_minimized` (Broker 经 `window.minimize` 回调页面).
- **分层**: 客户端出缓冲 (dmabuf / shm + damage + opaque 区域); 合成只出计划; 呈现 (GPU / DRM / RDP) 执行. 只传句柄、只报脏区,
  框架不读像素; 客户端慢就用上一次的缓冲.
- **`CompositePlan`** (src/model `plan.rs`): `{damage (最多 8 块), route: Composite | Direct, layers: [{window, dst, clip, look, effect, redraw}]}`,
  不透明的上层剔除下面. 原来由 acdes 的旧合成器执行 (含 backdrop 的模糊), 旧合成器删掉后只剩宿主窗口用; winman 不画窗下的 backdrop.
- **窗口管理** (框架裁决, 客户端只能请求): 新窗口居中、按窗口数错开 32 像素, 有父窗口的居中在父窗口上方 (不错开). 只有 `free` 改尺寸. 会话里 configure 不带 tiled 状态 (带了的话
  GTK / Firefox 认为窗口四边贴着别的窗口, 不给拖边改尺寸), 命中按客户端的 input 区域: 窗口几何外的那圈缩放边 (CSD 画在阴影里的) 也算;
  交互改尺寸期间 configure 带 `resizing`. 点进一个窗口: 拿焦点
  并提到它那一层最前, 它的子窗口 (含子窗口的子窗口, 保持原来的先后) 跟着提到它上方 (model `front_family`). 隐藏是框架状态 (不画、不命中、不拿焦点). `animate {opacity, scale, dx, dy, duration, delay, easing}` 由合成器每帧插值,
  做完发 `windowanimated`; 有 look 的窗口按画出来的位置命中.
- **焦点和模态**: 键盘只发给焦点窗口. 要把焦点给有模态子窗口 (xdg-dialog `set_modal` + `set_parent`, 或 Broker 授权时的 `parent` / `modal`) 的窗口时
  改给最上层的模态子窗口, 子窗口关掉焦点回父窗口. 指针也不发给有模态子窗口的窗口: 在它上面没有指针焦点, 按下只把焦点给模态子窗口 (不开始拖动).
- **grab** (宿主窗口; winman 下要等指针锁定、键盘锁定两组标准协议): `request_grab(kind)` 在被别人拿着或框架在移窗时拒绝; 键盘 grab 同时给焦点. 收回 (发一次 `grab_lost`): 在别的窗口
  按下 (`other_window`)、Esc (`esc`, 这次 Esc 不发)、焦点被给别人 (`focus_lost`)、客户端死掉.
- **全局快捷键** (src/model `shortcut.rs`): 组合写成 `ctrl+alt+t` (键是 xkb keysym 名, 按未修饰的那层匹配, Shift 写进组合; `mod` 在 macOS 上是 `super` (⌘), 别处是 `ctrl`, 同 Dopame 桌面的写法). 作用域 session /
  desktop / app / menu, 先到先得, 冲突拒绝, session 级不可覆盖; 匹配顺序 session → 焦点窗口的 menu 级 → 焦点窗口的 app 级 → desktop. 命中的键 (连同松开) 不发给窗口, 发
  `shortcut {window: owner, key, title: 作用域}`, `key` 是这一条绑定注册时写的文字 (同一组合的 app 级和 menu 级各记各的). keyboard-shortcuts-inhibit 生效时只有 session 级还拦.
  menu 级是窗口菜单项的快捷键 (acde-host `broker/menukeys.ts`), 只在没有桌面、菜单画在窗框上时 (`framesMenus`): 应用 `setMenus` 时 Broker 给有 `onSelect`、没禁用、带 `shortcut` 的项注册
  (同一文字只给第一项; 被占的记一行 warn 跳过), 再 `setMenus` 时按差异增删, 窗口关掉时随 app 级一起忘掉. 按下时 Broker 不转发事件, 直接
  调应用窗口的 `menu` (同桌面的 `invokeMenu`), 所以应用另用 `self.shortcut()` 注册了同一组合也只跑一次 (菜单项赢). 有桌面时不绑:
  菜单归桌面管 (Dopame 的编辑器给「编辑」菜单配了 `mod+C` / `mod+V` 等, 页面自己也处理这些键, 运行时抢走它们会改变编辑器的行为).

## 窗口协议

acdes 会话里窗口、桌面、会话与注入的协议全归 winman (`winman_desktop_v1`、`winman_window_v1`、`winman_session_v1`、`winman_inject_v1` 和标准协议),
页面窗口就是标准 `xdg_toplevel`, 面板与桌面是 wlr-layer-shell 的面. 原来的 `acde_window` (客户端 ↔ 框架) 与 `acde_shell` (Broker ↔ 旧合成器)
两份协议连同 acdes 的旧合成器已删 (repo-split 05); 逐项对照与去向在总仓 `.scratch/repo-split/protocol-map.md`.

## 宿主窗口

宿主窗口 (crates/hostwin, `<运行时> app` / `<运行时> desktop` 在别人桌面里的那条路) 归 acdev runtime ([acdev](http://gitea.dopamine/AC/acdev) 根上的 `src/`、`crates/hostwin`), 说明在它的 AGENTS.md.
