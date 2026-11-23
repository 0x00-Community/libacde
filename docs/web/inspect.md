# 看和操作桌面 (`acde:inspect`)

给 agent 用: 不截图, 把屏幕变成文字的语义树, 按 ref 点击、输入、按键. 只有桌面 (`acde.inspect`) 和桌面包的服务 (`ServiceContext.inspect`) 拿得到.

- **网页的树** (render `Dom::semantics(root, limit)`, `dom/semantics.rs`): 节点 `{id, role, name?, value?, states?, level?, rect, children?}`, id 是
  render 的节点 id, 所以 ref 跟着 DOM 节点走: 节点活着 (连在文档上, 或断开了但页面还拿着) 就不变, 移动、改文字、改属性、列表重排都不变,
  不按遍历顺序编号; 重绘时换成了新节点 (`innerHTML`、不带 key 的列表重建) 才是新 ref (节点 id 不复用), 旧 ref 回 `element N is gone`.
  测试 `src/render/tests/semantics.rs`. 角色按 `role` 属性 / 标签 (HTML-AAM: 有名字的 `<section>` 是 `region`, `<output>` 是 `status`), 都没有但 `cursor: pointer` 的是 `clickable`; 名字按 aria / label (可被 label 的元素: input、select、textarea、button、meter、output、progress, `<label for>` 或外面套的 `<label>`, 先于子元素文字) / alt / 文字, 最多
  120 字 (同一元素里相邻的文字节点按原文接起来, 只在原文边界有空白时隔一个空格: JSX 的 `{n}%` 是 `100%`); 剪掉不可见的子树 (`display: none`、`aria-hidden="true"`、`inert`、`opacity: 0`、`visibility` 不是 `visible`、滚到可见范围外的),
  没有角色名字状态的展开; 每个父节点最多 60 个子节点, 全树最多 limit (默认 600). `semantics(root, limit, offscreen, invisible)`: `invisible`
  时留下透明和 `visibility: hidden` 的元素, 自己或祖先 `opacity: 0` 的标 `transparent`, `visibility` 不可见的标 `invisible` (透明的照样能点,
  不可见的点不到); `display: none`、`aria-hidden`、`inert` 照样剪. `<select>`: 下拉框是
  `combobox` (值是选中项的文字, `collapsed` / `expanded`, 打开时子节点是列表里看得见的 `option`, rect 是列表里那一行), 列表框是 `listbox`
  (子节点是看得见的行).
- **web 侧** (`pageinspect.ts`): `tree(root, limit, offscreen, invisible)` (`offscreen` 时不剪掉滚到可见范围外的子树, `invisible` 时留下透明和
  `visibility: hidden` 的, 见上)、`query(selector, within,
  limit)` (`querySelectorAll`, 每个元素再用 `semantics(id, 1, true)` 取它自己的角色名字状态, 没有语义的给 `generic` 和 aria-label / title / 文字,
  加 `tag` 和 `visible`: 有大小、不是 hidden / display:none / 透明)、`point(id)` (元素可点的一点: 先 `scrollIntoView`, 在中心和四个四分点上 `hitTest`, 全被挡时报挡住的
  元素; 元素已经不在文档里 (被移除, 或节点已释放) 时各个调用都回 `element N is gone`, 没有盒子或 `visibility` 不是 visible 的回 `the element is not displayed`, 不拿旧位置去 `hitTest` (那样报出来的是「被别的元素挡住」); 打开的下拉列表里的 option 用 `pickerOptionRect` 那一行的中心)、`type(id, text, replace)` (走输入法提交那条路 `Host.insertText`)、`select(id, values)` (元素自己、祖先或里面的 `<select>`; 按 value
  再按文字找选项, 禁用的回错, 单选给多个回错; 设好后发 `input` 和 `change`)、`focus(id)`, 坐标加 `contentOffset`.
- **Broker** (`inspect.ts`): 窗口 ref `w0` (桌面)、`w1` …, 元素 `w3.12`, 窗框上的菜单 `w3.m1` (第 1 个菜单) 和 `w3.m1.2` (它的第 2 项, 分隔线也占号):
  有窗框菜单的窗口, 树的最前面是 `menubar [w3.m]`, 下面每个菜单一个 `menuitem` (`expanded` / `collapsed`), 打开的那个下面列出它的项 (快捷键是 value,
  `disabled` / `checked` / `focused`). act 菜单 `click` / `hover` 是在菜单名中心发真的指针事件; act 一项时菜单没开就先点开 (别的菜单开着时移过去),
  等列表画出来再点那一行, 禁用的项不点. acdev 的 driver 不用改: `getByRole('menuitem', { name })` 找到这些节点, 点它们走同一个 act. `snapshot({window?, ref?, all?, diff?, limit?, find?, tree?, offscreen?})` →
  `{text, windows, trees}`: 列出焦点窗口 (或指定的) 的树 (`all` 列全部没最小化的窗口, 并留下透明和 `visibility: hidden` 的元素).
  `text` 是固定的英文格式, 不随系统语言变 (页面给的名字和值原样): 窗口从上到下一行一个, `- window "Notes" [app=notes] [focused] [w1]`
  (桌面是 `- desktop`, 标记依次是 `app=`、`native`、`focused`、`minimized`、不是 `free` 的模式、窗口状态 `filled` / `fullscreen` (和模式相同时只写一次)), 列了树的窗口下面缩进两格一行一个节点
  `- button "Save" [pressed] [w1.52]` (角色是 ARIA 名, 状态见上), 后面跟括号里的说明 (`(empty)`、`(N more outside the visible area; …)`、
  `(too many elements, …)`); `find` 只给名字或值含这段文字的元素和祖先, 一个都没有时最后一行是 `(no element contains "…"…)`; `diff`
  只给变了的行 (`+` / `~` / `-` 后跟节点行, `(no changes)`), 变化超过六成整个重列 (`(many changes, listing everything)`), 窗口表没变时
  第一行是 `(windows unchanged)` 且只列有树的窗口. act / run / open / query 的文字和错误也是英文. `act(ref, action, arg)` → `{ok, text, windows?, expanded?}` (做完了就带 `windows` (同快照的 `windows`) 与 `expanded` (文字末尾附的那份快照里列了树的窗口引用, 树因 `limit` 截断的也算列过), 让调用方知道这次看到了哪些窗口, 如 acdes 管家据此给 `origin`; 一步都没做成时没有; `run`、`open` 同): 做完等页面安静
  (200 ms 没有 DOM 变化, 最多 3 秒; 原生窗口 0.4 秒), 窗口表或焦点变了先等新焦点窗口安静. `run(steps)` 依次做, 失败就停. 动作: click /
  double / right / hover / scroll 按 `point` 的屏幕坐标经 `Shell.inject` 发 (被盖住时先提前目标窗口; 桌面钉住的面板盖着的地方只算被更上层的钉住窗口挡住, #111); type / settext 走 web 的 `type`;
  key (`"ctrl+a backspace"`, acde-host `keys.ts` 的 `chordCodes`) 先 focus 再注入: 空格分开的每段先全部解析, 有认不出的就整串不发并报出这些段;
  可打印字符直接写 (`7 . 5 * 2`), 美式键盘上要 Shift 的符号 (`*`、`+`、`(` …) 和大写字母自动加 Shift, 也认 `plus` / `multiply` / `minus` 等名字, `ctrl++` 是 Ctrl 加 `+`; select (arg 是选项的值或文字, 多选给列表) 走 web 的 `select`; drag (arg 是目标 ref, 元素或窗口,
  可以在别的窗口) 在源上按下、量目标 (`point`, 可能滚动; 目标被别的窗口盖住时松开并回错)、按下后停 150 ms (像真人, 不然合成器在客户端还没发移动请求时就看到按键松开了, 客户机里拖标题栏会被拒, 2026-11-23), 分 10 步每步 16 ms 移过去, 再停一帧松开, 页面的指针拖动和
  HTML5 拖放 (也跨窗口) 都靠它. 窗口 ref 能 focus / restore / minimize / close / key. `open(name)` 按 id、名字找应用或原生程序起它. `query({selector, window?, ref?, limit?, attributes?})` →
  窗口 ref 还能 fill / fullscreen (`setState` 设成 `filled` / `fullscreen`), restore 把这两种设回 `normal`; 桌面和铺满宿主的窗口不行.
  窗口 ref 还能 move / resize / raise (2026-11-23, `dev/inspect/place.ts` 的 `Placing`, 填充、最小化也在那里): `move` 的 arg 是屏幕逻辑坐标 `"100,80"`, 大小不变;
  `resize` 的 arg 是 `"800x600"`, 位置不变 (先读窗口表里当前的 x、y); 经 `InspectShell.place(window, {x, y, width?, height?})` 做, 宿主窗口里是启动器
  `Shell.place` (`Launcher.place` 读回新矩形), 回 `w1 is at X,Y WxH`; 应用迟交新尺寸时最多轮询窗口表 2 秒, 还没到就照实说"还没接受这个尺寸". 拒绝 (回 `ok: false`
  并说原因): 桌面、铺满宿主窗口的单应用窗口、钉住的面板、最小化的窗口、状态不是 `normal` 的窗口 (提示先 `act <窗口> restore`), `resize` 另拒固定大小的窗口.
  `raise` 就是给焦点提到最前, 不改状态; 桌面和最小化的窗口不行. 客户机会话里 (窗口表来自管家) 启动器不摆放, acdev 的驱动直接经管家开发连接发
  `{op: "window", window: <acompositor 窗口号>, change: {place | focus}}` (acdes docs/manager.md 开发通道).
  客户机里锁屏页和系统确认页是管家插进窗口表的伪窗口 (行带 `lock: true` 或 `confirm: true`), 快照里它们的 `mode` 是 `lock`、`confirm` (窗口行显示 `[lock]`、`[confirm]`),
  叠在所有窗口之上; 系统确认页的开发名是 `confirm` (锁屏页是 `lock`), 启动器按欢迎消息里有没有 `confirm` 区分. 这两种窗口只由管家显示和收起, 对它们的
  `fill`、`fullscreen`、`minimize`、`move`、`resize`、`raise`、`close` 一律回 `ok: false` 并说明; 确认页的允许、拒绝、输密码用 act 点它自己的按钮.
  `{window, count, nodes, windows}` (`windows` 只有被查的那个窗口, 形状同快照的 `windows`): 在窗口 (默认焦点窗口) 或某个元素下按 CSS 选择器找, 节点是 `InspectMatch` (`InspectNode` 加 `tag`、`visible`, 给了
  `attributes` (最多 20 个属性名) 再加 `attributes: {名字: 值或 null}`), ref
  能直接给 act (不在语义树里的元素也行: `point` 按 render 节点 id 找元素).
- **不在屏幕上的窗口** (`inspect.ts` 的 `Inspect.#offScreen`, 只有这一处): `windows` 里每个窗口的 `state` 是运行时的窗口状态 (宿主窗口的行自带;
  会话的行没有, 取 Broker 按 `windowstate` 记在窗口记录上的), `hidden` 为真表示它不在屏幕上: 被 `windows.hide()` 藏起, 或状态是 `minimized`
  (`act <窗口> minimize`、`requestState`、窗框按钮、页面的 `minimize()`). 宿主窗口里最小化不动隐藏标记, 以前 inspect 只看隐藏标记, 这样最小化的窗口
  没有 `[minimized]`, `all` / `find` 照列它的内容, 它还被算成盖着下面的窗口 (点桌面回 `covered there by …; minimize or close it first`).
  `[minimized]` 标记、`all` / `find` 跳过谁、默认列哪个窗口、点名它时的 `(minimized; act restore on it to see its elements)`、谁盖着谁 (`#cover`)
  都只看 `hidden`; `query` 照样在它的页面里找 (CSS 选择器不管显示与否), 但每个节点的 `visible` 为假. 指针动作 (click / double / right / hover / scroll, drag 的两端) 落在这样的窗口上回 `ok: false`
  `w1 is minimized; act restore on it first` (drag 的目标是 `cannot drop: w1 is minimized; …`, 在源上按下之前就查), 什么都不注入: 以前按它最小化前的位置点下去, 点到的是
  下面的窗口, 还回 `clicked …`. focus / type / settext / key 先给窗口焦点, 运行时给最小化的窗口焦点就是让它回到之前的状态. `restore` (和窗口的 `focus`)
  先取消隐藏再给焦点, 两条路离开屏幕的窗口都回来.
- **像素** (`shot`、`input`, 2026-11-23): 没有元素树的窗口 (游戏、OpenGL / Vulkan 程序、不报 AT-SPI 的原生程序) 按像素看和操作. 只在 acdes 会话里有,
  由管家自己答 (不经页面, 见 acdes `docs/manager.md` 的 `inspect.shot`): `shot(window, {max?})` 回那扇窗自己画面的 PNG (base64, 长边缩到 `max`, 默认 1280)
  和窗口的逻辑矩形; `input(window, events)` 按窗口逻辑坐标发 move / down / up / wheel、相对移动 `motion {dx, dy}` (游戏锁住指针时转视角)、
  按住与松开键 (`keydown` / `keyup`)、`text`、`wait`. 类型 `InspectPixelInput`、`InspectShot`.
- **Agent 的 stage** (acdes, 2026-11-23): `open(name)` 开的原生程序 (不是 acde 应用的) 落在 Agent 自己的 stage 上, 回答带 `stage: true`: 它的窗口不画到屏上、
  不抢人的键盘, 对程序自己照常可见. 快照的窗口 (`InspectWindow`) 带 `stage: true`, 文本行多一个 `[stage]` 标记; `shot`、`input` 的回答带 `stage`.
  stage 上的窗口 `input` 不等人让开、不管它是否最小化; 人在桌面上激活它 (`acde:windows.focus`) 就是人接手, 之后 `stage` 为 false, 照人桌面的规则.
  细节见 acdes `docs/manager.md`「Agent 的 stage」.
- **原生窗口** (runtime `atspi.rs`, napi `Accessibility`, Linux 会话): 读 AT-SPI2. 起服务时把 `org.a11y.Status.IsEnabled` 设成 true; GTK3 总上报,
  Qt 要开关为 true 且在真的显示平台上. `tree(pid, title, limit)` 按 pid 找应用、按标题挑顶层窗口, 角色规范成 ARIA 风格; act 先试 AT-SPI 的
  Action / EditableText / GrabFocus, 不行按坐标. 检查 `src/runtime/tests/atspi-check.sh`.
