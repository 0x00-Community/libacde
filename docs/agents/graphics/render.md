# render: 文档、DOM 与出帧 (`src/render`)

`src/render` 是 Blitz 包成的渲染引擎: 文档、DOM 原生接口、出帧、canvas、窗框. 改接口先改这里; 模糊与系统玻璃见 [glass.md](glass.md).

- `Document::new(width, height, scale, &Options {base_url, resources: [Resource {prefix, dirs}], default_lang, verify_paint, svg_text_path,
  dark, name})`: 尺寸是物理像素. 同源资源按最长前缀在 dirs 里找 (`render/net.rs` `LocalNet::path`): 路径先百分号解码, 只许普通的段 (挡 `..`), 也不给以 `.` 开头的段 (`.env`、`.git` 这类); 页面 fetch / XHR / 模块走的 `Assets::file` 用同一个 `resource_file`, 再把文件和借出的目录都解开符号链接 (`canonicalize`), 解开后的文件不在那个目录里就当没有 (2026-11-22: 借出的文件夹里指向外面的链接, 如 `~/Pictures/x` → `~/.ssh/id`, 原来照读); 系统图标走内置前缀 `/acde-icons/`: 地址是 `/acde-icons` 加图标的绝对路径 (acde:icons 的 `icons.lookup` 返回的就是它), 只认解开符号链接后落在图标目录里的文件 (`$XDG_DATA_HOME` 与 `$XDG_DATA_DIRS` 下的 `icons`、`pixmaps`、`swcatalog/icons` (AppStream 的应用图标, 商店用), 加 `~/.icons`; 路径先按百分号解码再核对; 清单只在 `net.rs` 的 `icon_dirs`, acdes 应用沙箱把这几处只读绑进去); 前缀不叫 `/icons/`, 免得和应用自己的资源前缀撞名. 系统声音同法走 `/acde-sounds/` (给 acde:sounds 的 `sounds.lookup` 返回的路径用): 只认 `$XDG_DATA_HOME` 与 `$XDG_DATA_DIRS` 下的 `sounds`; 两个前缀共用 `net.rs` 的 `system_file` 一处核对 (同一套数据目录、同样先解码再解开符号链接), 页面的 `<img>`、CSS 经渲染的取资源走它, fetch、XHR 和媒体 (`<audio>` 等, 经 `Assets::file`) 也走它. `file:` 只在开发模式 (`dev_mode`) 读借出的目录, 平时一律当没有, 和浏览器一样 (用户定: 原来任何页面写 `<img src="file:///…">`、CSS `url(file:…)` 都能读到本机任意文件; 中间一版放行过图标目录); 不联网, 读不到回空. `Document` 发句柄:
  `dom()` (JS 线程)、`handle()` (出帧方).
- 线程: 文档由 JS 线程和 `acde-render` 线程共用 (一把锁). 出帧时损伤、切段和面板 (`Finish::run`) 在这把锁外做, 用的绘制缓存和损伤状态在另一把锁 (`Framer`) 里; JS 线程上要清它们的调用 (钉面板、换玻璃偏好、`recover`) 等这一段做完.
- DOM: `Dom` 实现 acde-dom 的 `DomNative` (契约 `js/dom/src/native.ts`, 同名 snake_case). 节点 id 是 u32 不复用, `<html>` 是 0;
  查询是 CSS 像素; 错误说明以 DOMException 名字开头. 改了文档还没 `Dom::commit` 时 render 不出帧, 期间的指针和滚动在 commit 时按顺序放回.
  连在文档上的节点一直活着, 断开的子树有 JS 包装 (pin) 才活; 包装对象连着时强引用, 断开变 WeakRef, 回收时 unpin (acde-dom `realm.ts`).
  `hit_test` 返回节点 id (坐标是视口的, 整页滚动过也对: blitz 的 `hit` 要页面坐标, `element_from_point` 加上视口的滚动); `metrics` 对根元素照 CSSOM View 给: `clientWidth` / `clientHeight` 是视口的大小 (不是内容的), `scrollWidth` / `scrollHeight` 不小于视口 (acde-dom 的 `scrollIntoView` 靠这个判断整页能不能滚); `caret_rect` / `caret_from_point` / `text_rects` (偏移是 UTF-16). `<select>`: `options` / `option_selected` /
  `set_option_selected` / `selected_index` / `set_selected_index` / `reset_select` / `list_box_option_at`; 选项列表 `open_picker` /
  `close_picker` / `picker_state` (`[select 或 -1, 高亮的选项序号或 -1, 一页几行]`) / `picker_option_at` (-2 在列表外, -1 在列表里但不是
  选项) / `picker_option_rect` / `set_picker_active`; 列表画在 `Frame.plane` 的最后. `Dom::highlight(id)` (`NONE` 去掉, DevTools 的高亮, #192,
  `engine/highlight.rs`): 每帧按当时的布局在整页 (含选项列表) 之上画这个元素的 margin / border / padding / content 四块 (CSS 像素的
  `getBoundingClientRect` 当 border 盒, 按 taffy 的 margin / border / padding 宽度外扩内缩, 颜色是 Chrome 的默认半透明配色), 所以滚动、
  缩放和布局变化都跟着走; 元素不在文档里、`display: none` 或不是元素时不画. 改了就标脏并叫醒出帧线程, 不需要 `Dom::commit`. `scroll_at` 落在打开的列表上时滚列表, 落在外面先关掉它.
- 光标: `RenderHandle::on_cursor(listener)` (`CursorListener`, `Fn(&'static str)`): 指针下的光标变了时在 render 线程上调, 值是 CSS 的关键字
  (`auto` 已经定成 `default` / `text` / `pointer`, 见 `docs/agents/web/web-apps.md`「光标」); 装上时已经有值就先调一次. `Engine` 在每次 `pointer`
  和每次重算 (`resolved` 里真的 resolve 了) 之后取一次, 出帧线程每轮末尾比较, 变了才调.
- 出帧 (物理像素): `RenderHandle {tick(now_ms), frame(), scroll_at, pointer, commit, invalidate, set_waker, on_cursor}`. CSS 动画的时钟是单调墙上时间,
  tick 只对齐到 vblank. 脚本的一个任务里 (从任务里第一次经 `Dom` 读写引擎到任务末的 `Dom::commit`) 动画时钟不走: 任务里第一次重算记下当时的时间,
  之后这个任务里的重算都用它 (照 HTML, 文档时间线只在「更新渲染」时前进; 不然一个任务里先起的动画到后面读 `getComputedStyle` 时已经多走了几十毫秒,
  WPT 插值测试的「100s 时长、-50s 延迟」会差 0.01px). 出帧线程的重算不在任务里, 照旧用墙上时间; 时间只进不退. `Frame {plane, panels, size, scale, animating, commit, trace}` (size 是物理像素, scale 是画这一帧时的缩放: 前端靠这两个认出改尺寸或缩放之前画、晚到的帧): plane 是整页 (含画布背景, 不含面板); `panels` 是
  `Dom::set_panel` 钉住的元素 `Panel {id, size, plane, extent, reach}`: extent 是面板里实际画到的范围 (物理像素, 相对这一帧的面板矩形), reach 是同一块换成页面坐标 (CSS 像素, 用这一帧画面板时的矩形和缩放算, 前端报 `panelmove` 用它, 不拿自己记的矩形去拼: 缩放切换时两边不同步, 拼出来的位置是错的; 尺寸或缩放和窗口现在的不一样的帧是改之前画的, 前端不报它的范围, 不然页面会拿旧布局的范围去并新的盒子). `Plane {drawing, layers, damage, damage_rects (最多 8 块), bounds}`;
  layers 是有 backdrop-filter / filter 时切成的段, 每段 `{drawing, bounds, backdrop, filter}` (`Drawing` 出 `scene` / `scene_for`): backdrop 先处理画布上已画的那块再画这段, filter
  先画进临时纹理再合成. 光栅化方 (src/vello) 做 filter 的 drop-shadow、模糊、颜色矩阵, backdrop 的模糊和所有颜色函数 (`Backdrop::effect`:
  按顺序合成一个颜色矩阵, 几个 blur 按平方和开方合并); backdrop-filter 里的 drop-shadow 不做; 只切最外层: 已有 filter 的元素里面再有 filter / backdrop-filter 时不做 (#46);
  这两种同一组滤镜只记一次 warn (最多记 64 组).
  同一元素既有 `mask-image` 又有 filter (AC/libacde#5): 规范的顺序是先滤镜、再遮罩、最后 clip-path, 但段模型里滤镜由前端在合成时套用, 没有"滤镜之后
  再套遮罩"这一步; 原来滤镜层弹出就切段, 遮罩的 DestIn 落进下一段新开的空隔离层, 遮罩丢掉, 整块显示 (dopame 菜单栏充电闪电成白方块). 现在 `Split` 在滤镜层之后紧跟着
  DestIn 层时 (`masked_after`) 把这层遮罩并进滤镜那一段 (段到遮罩层弹出为止), 并去掉滤镜里的 drop-shadow (`unshadowed`; 只剩 drop-shadow 时不切段).
  内容在遮罩内不透明时 (按规范投影全被遮罩裁掉) 和规范逐像素相同. 和规范不一致的两处: 模糊在遮罩之后做, 遮罩边缘会被柔化; 内容在遮罩内半透明时看不到本该透出的投影.
  按规范的完整修法见 AC/libacde#5 (跨 libacde、acdes、acdev: Part 加滤镜之后的遮罩, 各合成器先把阴影和内容画到临时图上再套遮罩), 留作后续.
  回归测试在 `tests/render_layers.rs` (遮罩外的像素和没有这个元素时相同; 模糊保留、遮罩在段内).
  `text-shadow` (vendor/blitz `blitz-paint/src/text/shadow.rs`): 每个阴影是同一段字形按偏移再画一遍, 颜色用阴影的, 写在后面的画在下面, 都在文字之前;
  有模糊时这段字形放进一个 GaussianBlur 滤镜图层 (标准差为模糊半径的一半, 同 box-shadow 和 drop-shadow), 所以带模糊的文字阴影也走上面的 filter 段, 受同样的限制
  (已有 filter 的元素里面不做); 装饰线和 "…" 不带阴影, `background-clip: text` 的蒙版不画阴影.
  窗口之间的毛玻璃 (`setBackdrop`、面板的 `backdrops`) 仍只带 blur / saturate / brightness.
  磨砂的角形: blitz-paint 推带 `backdrop-filter` 的图层时用 `push_shaped_layer` 带上元素的 `Corners {radius, exponent}`
  (`acde-scene` 的 `corner`; `exponent` 是超椭圆指数 `n = 2^k`, 2 是圆弧, CSS `squircle` 是 4), `render::Backdrop` 记下它: `radius()` 有声明时
  用声明的半径 (乘图层变换的缩放), 不再从背景路径猜; `exponent()` 没有声明时是 2. GPU 合成的 `gpu::Backdrop` 多一个 `exponent`, 着色器
  用 acde-scene 导出的 `corner::WGSL` (`acde_corner_coverage`, 建着色器时拼在 `compose.wgsl` 前面; acdes 的 compose 同样拼它), 和 `corner::coverage`
  逐行对应, 公式只在 acde-scene 一处; 前端把面板的磨砂交给
  合成器时带上 `exponent()` (acdes 用 `acompositor_backdrop_v1` 版本 2 的 `add_shaped_area`). 窗口级的 `setBackdrop` 也带角形 (issue 16):
  `effect.shape` 收 `round` / `squircle` / `superellipse(k)` (k 取 1 到 3, 和 `setWindowLook` 的 `corners.shape` 同一个解析 `cornerShape`, `js/page/src/frametheme.ts`),
  胶水只传 k, 原生层 (`webnative.rs` `set_backdrop`) 用 `acde_scene::corner::exponent` 换成 n, `SurfaceCommand::Backdrop` 的效果是
  `[半径, 模糊, 饱和, 亮度, 指数]`; C 接口不改结构, 指数放在 `AcdeCommand.thickness` (Backdrop 本来不用它). 没写 shape 时是圆弧 (n = 2).
- 输入法浮层 (2026-11-24, 总仓 `.scratch/system/zorder.md` 5.1): 页面自画的候选框 (`ImePanel`) 和预编辑文字原来挂在 `document.body` 上, 只能画进页面所在的表面,
  会被钉住的面板和更靠前的窗口盖住. 宿主的 `platform::Window::ime_float()` 返回 true 时 (surface 的 `imeFloat`, 默认 false; acdes 的窗口 crate 返回 true), 两者改挂到一个
  `position:fixed; inset:0` 的容器 (`data-acde-ime-float`, `js/page/src/panel.ts` 的 `ImeLayer`) 里, 自身改 `position:absolute`. 候选框放下后量一次大小 (`ImePanel::fit`, 2026-11-24): 右边越出页面就往左挪到贴右沿, 下边放不下而上方放得下就翻到光标上方, 所以内容不出页面; 越出屏幕时合成器裁掉而不平移 (acdes).
  每次变动后量出各子元素外框的并集 (CSS 像素, 页面坐标; 候选框外扩它阴影能画到的范围 `ImePanel.SHADOW_REACH`, 否则阴影被浮层边界切成一块硬边矩形, 2026-11-24; 并集再裁到页面内), 变了才发: dom `setImeFloat(容器, x, y, w, h)` 把容器的子树当作一块面板画, 面板号固定为
  `render::IME_FLOAT_PANEL` (`u32::MAX`, 在 `Frame.panels` 里, 只在浮层显示的帧里有); surface `setImeFloat(x, y, w, h)` 发 `SurfaceCommand::Ime(Some(ImeFloat {x, y, width, height}))`
  (整数, 逻辑像素, 原点是页面内容区左上角). 每次发出都记一行 info 日志 (target `acde::ime`, `ime float x=… y=… width=… height=…` 或 `hidden`), 取证用. 宽或高为 0 即 `Ime(None)`: 输入结束 (预编辑清空且候选框收起)、输入框失焦或窗口失去激活、候选框收起后都立刻发 None, 合成器看不到
  输入框是否激活 (页面经 D-Bus 直连 fcitx5), 靠这条防止浮层残留. 宿主把这块面板画进 `acde_ime_surface_v1` 的表面 (acdes, 放在输入法段, 不收输入).
  返回 false 的宿主 (acdev 的宿主窗口) 照旧画在文档里. C 接口不转发 `Ime`.
  接上输入法的时机 (2026-11-24): 焦点进出元素 (`focusin` / `focusout`, 不管是点击、键盘还是脚本 `focus()`) 和窗口重新激活后, 当轮微任务里就查一次 (`TextEntry.report`), 不等下一帧或按键; 原来只在帧里和打字时查, 页面自己 `focus()` 的输入框要等打字才接上, 这之前的 Ctrl+Space 会落到页面.
  取证 (2026-11-24): 页面 `InputMethod` 在状态变化时记 info 行 (target `acde_host`, 前缀 `input method:`): 文本框接上输入法、没有文本框持有、窗口变为激活或不激活; 按键时只在情形变化时各记一次: 没有文本框持有时按键直接给了页面、输入法把按键交还页面.
  指针 (5.1 第 4 条): 合成器把落在浮层上的指针事件只交回本连接, 宿主把浮层表面内坐标加上 `ImeFloat` 的 x、y, 当作普通页面指针事件 (同钉住面板) 交给页面; 候选框就在文档里那个位置,
  按下选中点到的词 (`mousedown` 取消默认, 不动键盘焦点), 滚轮翻页 (deltaY 大于 0 下一页). 宿主不为点浮层发激活或焦点变化.
- 动画曲线 (`acde_scene::motion`): 合成器的窗口动画只用这一份. 两档 `Preset`: `Standard` 是临界阻尼的弹簧
  (响应 0.35 秒, 不过冲, 约 0.51 秒落到 0.1% 内, `Curve::length`), `Reduced` 是 0.15 秒线性淡入淡出且不缩放、不位移 (`moves()` 为假); 桌面只能选档, 不能给数值.
  `Ease` 是 CSS 的五个缓动关键字 (`linear`、`ease`、`ease-in`、`ease-out`、`ease-in-out`, 三次贝塞尔按 x 二分求 y), `acde:windows.animate` 的 `easing` 用它;
  两者的线上编号 (`to_wire` / `from_wire`, 按名字查 `Ease::wire_of`) 供 acdes 的合成器协议用; 弹簧落到 0.1% 内的时刻按常量算, 不在每帧求解.
  `Curve::css(n)` 把曲线按 n 段采样成 CSS 的 `linear(…)` 缓动, 时长取 `length()`:
  页面要和合成器的动画同步时 (桌面切换时滑动桌面层) 从 `acde:desktop` 拿这两个值, 不在页面里另存一份 (用户选 B).
- `Dom::capture(id, scale)`: 单独录一个元素 (拖动图标), 单边超过 2048 缩小; JS 拿 `picture` 编号 (`dom.rs`, 每个 web 留最后一个).
- canvas 2D (acde-dom `dom/canvas.ts` 编成命令流, `Dom::canvas`): 要像素的都在 GPU 上 (render `gpu.rs`, 用 `acde::gpu::Gpu::publish` 登记的设备):
  超过 20000 条命令 (`ACDE_CANVAS_MAX_COMMANDS`) 画成纹理再接着记, canvas 当图片源是 `GpuImage`, `shadowBlur` 用 `blur.wgsl`,
  `getImageData` / `toDataURL` (`toBlob` 经它) 同步读回. 没有设备 (`Gpu::publish(None)`, 如 acdes 用 CPU 画页面时) 画布没有纹理, 这两个改由
  `acde::render::gpu::set_cpu_raster` 登记的 CPU 光栅器把画布的场景画成整张 RGBA8 (非预乘、逐行、宽 × 高 × 4 字节, 长度不对当作没画出) 再裁出要的区域;
  谁也没登记时照旧给全透明 / `data:,` (`toBlob` 得到 0 字节). 提交都拿 `Gpu::submit_lock`. `drawImage` 一张 SVG 的 `<img>` 时按它的自然尺寸画成场景
  (blitz-paint `svg_picture`, 和 `<img>` 同一画法). `ctx.filter`: acde-dom 解析 `<filter-value-list>` (无效的值不改, 长度只认 px),
  命令 14 送效果 (blur、brightness、contrast、grayscale、hue-rotate、invert、opacity、saturate、sepia、drop-shadow; url() 不支持);
  有滤镜时每次绘制先把它的范围 (加上效果外扩, 裁到画布) 画进一张图, src/vello `Offscreen::filtered` 依次做 (`blur.wgsl` 的
  `blur_rgba` 全通道模糊、`matrix.wgsl` 颜色矩阵、drop-shadow 是 alpha 模糊再垫在下面), 再画回去; 阴影还按没滤过的形状算.
- `createImageBitmap` / `ImageBitmap` (acde-dom `dom/bitmap.ts`): 把 `<img>`、`Blob`、`ImageData`、canvas、`ImageBitmap` 画进一个不挂在文档上的
  2D 画布, 支持裁剪矩形、`resizeWidth` / `resizeHeight`、`imageOrientation: 'flipY'`、`premultiplyAlpha` (WebGL 上传时按它, 不看
  `UNPACK_FLIP_Y_WEBGL` / `UNPACK_PREMULTIPLY_ALPHA_WEBGL`); `<video>` 来源报 `NotSupportedError`. 2D `drawImage` 和 WebGPU 外部拷贝收它.
- 选项 `default_lang` (默认 zh-Hans)、`dark` (事后用 `set_dark` 改, 都只改
  viewport, 媒体查询和 `matchMedia` 跟着变). 页面能用的偏好类媒体查询: `prefers-color-scheme` (`dark` / `set_dark`)、`prefers-reduced-transparency`
  (`set_glass` 的减少透明度, 见 `docs/agents/dev/build-run.md` 的玻璃通透度)、`prefers-reduced-motion` (acss 现在恒为 `no-preference`, 还没接系统的值). 系统颜色 `AccentColor` / `AccentColorText` 跟 `set_accent` (`Option<[u8; 3]>`, 见 `docs/agents/dev/build-run.md` 的强调色). `semantics(root, limit, offscreen, invisible)`: offscreen 时不按可见范围剪, invisible 见 `docs/agents/web/inspect.md`「看和操作桌面」. `<iframe>` 不加载
  (`skip_iframe_content`), 只是空盒子. 增量录制见 vendor/blitz; `ACDE_PAINT_VERIFY=1` 每帧再整页录一次比对 (测试用). 图片缓存最多 256 张 /
  64 MB. 所有入口 `catch_unwind`.
- 窗框 (`decoration.rs`): `Decoration::scene(&Bar {title, width, height, scale, style, fullscreen})` → 物理像素的场景 (背景 #f4f2f7; `FrameStyle::Mac`:
  左边三个 12 px 圆点 (关闭 / 最小化 / 全屏), 标题居中截断; `FrameStyle::Windows`: 右边三个 46 px 宽的按钮 (最小化 / 最大化 / 关闭, 1 px 线画的图标,
  `fullscreen` 时最大化换成还原), 标题靠左截断; 没有悬停效果). `FrameStyle::current()`: `ACDE_FRAME_STYLE` (`mac` / `windows`), 没设或别的值时
  Windows 上的单应用是 `Windows`, 别处是 `Mac` (宿主窗口给出完整桌面时 `FrameStyle::follow_session`, 跟 Linux 会话一样). `Bar::button_at(x, y)` 是按钮命中, `FrameStyle::grab_hints` 是标题栏 (按钮区外) 和四边四角的 hint. 测试 `tests/render_decoration.rs`.
  `Bar.theme` (`FrameTheme`: `active` / `inactive` 两组 `FrameColors {background, title, line, buttons}`, RGBA, 没给的用默认; 失焦的组没给的项用
  有焦点的; `style` / `side` (`ButtonSide`) / `align` (`TitleAlign`) 没给时按 `Bar.style`: `Mac` 是左边、居中, `Windows` 是右边、靠左) 和
  `Bar.active` 只改样子; 按钮放在哪一边时命中和 `FrameTheme::grab_hints` 跟着换边. `buttons` 给圆点上色 (不给是红黄绿), 给图标上色 (不给是标题色).
- 帧率面板 (`hud.rs`, 宿主窗口的调试用, 谁画、数从哪来见 acdev `docs/agents/hostwin.md`「帧率面板」): `Hud::scene(Option<&HudStats>, scale)` → `Hud::SIZE`
  (236 × 112 逻辑像素, `Hud::pixels(scale)` 是物理像素) 的场景: 圆角深色底, 大字 FPS (按平均帧间隔着色: 不超过预算的 1.25 倍绿、2 倍以内黄、
  更长红), 平均 / 最长帧间隔和每帧光栅化时间, 下面是最近 `Hud::BARS` (120) 帧的帧间隔柱 (同样着色, 高度到 3 倍预算封顶) 和 1 倍、2 倍预算两条线.
  `None` 是「idle」(一秒内没有新帧). 字和窗框共用 `draw_line`, 等宽字体 `ui-monospace, SF Mono, Menlo, monospace`.
