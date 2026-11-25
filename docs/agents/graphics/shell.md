# shell (napi 类 `Shell`)

启动器侧的窗口系统对象与页面侧的 surface 怎么接上, 以及给 surface 的事件.

会话里的合成器是 acdes 的 acompositor (协议、DRM/KMS、HiDPI 见 acdes 的 `docs/agents/acompositor/`), 远程桌面是 acdes 的远程进程;
宿主窗口里是 acdev runtime 的 `crates/hostwin`, 见它的 `docs/agents/hostwin.md`. 下面是 Broker 和 web 看到的接口.

- 启动器侧的窗口系统对象: acdes 里是 `native.Windows.connect()`, acdev 宿主窗口里是 `native.Shell.broker()`. 都有 `onEvents(cb)` / `pump()`、
  `grant({desktop?, window?, mode?, rect?, size?, parent?, modal?})` → `{window, token}` (随机 64 位令牌, acdes 的窗口客户端在本进程里按令牌取回)、`place`、`windows()`、
  `shortcut` / `unshortcut` (冲突抛错)、`focus`、`hide`、`close`、`socketName`、`screenshot(path)`; 宿主窗口另有 `release`、`animate`、`stats`、`inject` 等.
- web 侧: `claimSurface(token, {frame?})` (src/host `webnative.rs` → 前端的 `Platform::claim`, 会话里是 acdes 的 `crates/window` `Surface::claim`: 连上 acompositor、开 toplevel 或 layer 面;
  一条 `acde-window` 线程管 Wayland 和光栅化) 给出 surface: `pump`、`wait()` (队列非空时落定)、`waitThen(cb)` (队列非空时在一个新任务里调 cb, 不在微任务里: acde-host `Host.run` 每轮取一批输入就是一个任务, 输入事件经 acde-dom `Input` 用 `fireFromTask` 派发, 每个监听器后跑微任务检查点, React 的离散事件在下一个按键前就提交; 原先循环 `await wait()` 之后都在微任务里, 检查点不生效, 连按三次只算一次)、`requestFrame`、`setClipboard`、
  `readClipboard()`、`startDrag(data, icon?)`、`setTitle`、`setImeArea`、`window`、`hosted`、`contentOffset`.
- 事件 (`Event`, 坐标逻辑像素, button / code 是 evdev 码). 给 surface 的: `pointermove` / `pointerdown` / `pointerup` / `wheel` (带 `scale` 的是触控板捏合这一步的倍数: 页面胶水 `keys.ts` 照 Chrome 换成按着 Ctrl 的 `wheel`, `deltaY = -100 · ln(scale)`, 页面用 `Math.exp(-deltaY / 100)` 换回倍数; acdes 的窗口客户端经 `zwp_pointer_gestures_v1` 的 pinch 发它) /
  `pointerleave`、`keydown` / `keyup {code, key}`、`imepreedit` / `imecommit` (宿主窗口)、`activate` / `deactivate`、`framebutton {key}`、
  `frame {time}`、`resize {width, height, scale}` (页面的物理像素, 已减去窗框)、`dndenter` / `dndmove` / `dndleave` / `dnddrop {x, y, data}`
  (data 是类型 → 文本, 松手时已读好)、`dragend {button}` (1: 放到了别的窗口上)、`grab` / `grablost`. 给 Broker 的: `output`、`windowopen` /
  `windowclose` / `windowtitle` (原生窗口)、`windowstate {window, state, previous}`、`windowactivate`、`windowanimated`、
  `shortcut` (转给 owner)、`windowgone {window, x, y, width, height}` (web 窗口最后的 rect, 全屏的是之前的)、`windowminsize`、`windowparent`、
  `windowcloserequest` (宿主窗口)、`devreload` / `devrequest`、`milestone`、`inputtarget` (`ACDE_DEBUG_INPUT`)、`exit`.
- 注入 `inject({type, x, y, button, dx, dy, source, code})`: `move` / `motion` (相对) / `down` / `up` / `wheel` (source `wheel` / `finger` /
  `continuous`) / `keydown` / `keyup`, 和真实输入同一条路由.
