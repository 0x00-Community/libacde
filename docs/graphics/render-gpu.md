# 组件契约: render、shell、GPU 与性能

改接口先改这里.

## render (`src/render`, Rust 接口)

- `Document::new(width, height, scale, &Options {base_url, resources: [Resource {prefix, dirs}], default_lang, verify_paint, svg_text_path,
  dark, name})`: 尺寸是物理像素. 同源资源按最长前缀在 dirs 里找 (`render/net.rs` `LocalNet::path`): 路径先百分号解码, 只许普通的段 (挡 `..`), 也不给以 `.` 开头的段 (`.env`、`.git` 这类); 页面 fetch / XHR / 模块走的 `Assets::file` 用同一个 `resource_file`, 再把文件和借出的目录都解开符号链接 (`canonicalize`), 解开后的文件不在那个目录里就当没有 (2026-11-22: 借出的文件夹里指向外面的链接, 如 `~/Pictures/x` → `~/.ssh/id`, 原来照读); 系统图标走内置前缀 `/acde-icons/`: 地址是 `/acde-icons` 加图标的绝对路径 (acde:icons 的 `icons.lookup` 返回的就是它), 只认解开符号链接后落在图标目录里的文件 (`$XDG_DATA_HOME` 与 `$XDG_DATA_DIRS` 下的 `icons`、`pixmaps`、`swcatalog/icons` (AppStream 的应用图标, 商店用), 加 `~/.icons`; 路径先按百分号解码再核对; 清单只在 `net.rs` 的 `icon_dirs`, acdes 应用沙箱把这几处只读绑进去); 前缀不叫 `/icons/`, 免得和应用自己的资源前缀撞名. `file:` 只在开发模式 (`dev_mode`) 读借出的目录, 平时一律当没有, 和浏览器一样 (用户 2026-11-22: 原来任何页面写 `<img src="file:///…">`、CSS `url(file:…)` 都能读到本机任意文件; 中间一版放行过图标目录); 不联网, 读不到回空. `Document` 发句柄:
  `dom()` (JS 线程)、`handle()` (出帧方).
- 线程: 文档由 JS 线程和 `acde-render` 线程共用 (一把锁).
- DOM: `Dom` 实现 acde-dom 的 `DomNative` (契约 `web/packages/acde-dom/src/native.ts`, 同名 snake_case). 节点 id 是 u32 不复用, `<html>` 是 0;
  查询是 CSS 像素; 错误说明以 DOMException 名字开头. 改了文档还没 `Dom::commit` 时 render 不出帧, 期间的指针和滚动在 commit 时按顺序放回.
  连在文档上的节点一直活着, 断开的子树有 JS 包装 (pin) 才活; 包装对象连着时强引用, 断开变 WeakRef, 回收时 unpin (acde-dom `realm.ts`).
  `hit_test` 返回节点 id (坐标是视口的, 整页滚动过也对: blitz 的 `hit` 要页面坐标, `element_from_point` 加上视口的滚动); `metrics` 对根元素照 CSSOM View 给: `clientWidth` / `clientHeight` 是视口的大小 (不是内容的), `scrollWidth` / `scrollHeight` 不小于视口 (acde-dom 的 `scrollIntoView` 靠这个判断整页能不能滚); `caret_rect` / `caret_from_point` / `text_rects` (偏移是 UTF-16). `<select>`: `options` / `option_selected` /
  `set_option_selected` / `selected_index` / `set_selected_index` / `reset_select` / `list_box_option_at`; 选项列表 `open_picker` /
  `close_picker` / `picker_state` (`[select 或 -1, 高亮的选项序号或 -1, 一页几行]`) / `picker_option_at` (-2 在列表外, -1 在列表里但不是
  选项) / `picker_option_rect` / `set_picker_active`; 列表画在 `Frame.plane` 的最后. `Dom::highlight(id)` (`NONE` 去掉, DevTools 的高亮, #192,
  `engine/highlight.rs`): 每帧按当时的布局在整页 (含选项列表) 之上画这个元素的 margin / border / padding / content 四块 (CSS 像素的
  `getBoundingClientRect` 当 border 盒, 按 taffy 的 margin / border / padding 宽度外扩内缩, 颜色是 Chrome 的默认半透明配色), 所以滚动、
  缩放和布局变化都跟着走; 元素不在文档里、`display: none` 或不是元素时不画. 改了就标脏并叫醒出帧线程, 不需要 `Dom::commit`. `scroll_at` 落在打开的列表上时滚列表, 落在外面先关掉它.
- 出帧 (物理像素): `RenderHandle {tick(now_ms), frame(), scroll_at, pointer, commit, invalidate, set_waker}`. CSS 动画的时钟是单调墙上时间,
  tick 只对齐到 vblank. 脚本的一个任务里 (从任务里第一次经 `Dom` 读写引擎到任务末的 `Dom::commit`) 动画时钟不走: 任务里第一次重算记下当时的时间,
  之后这个任务里的重算都用它 (照 HTML, 文档时间线只在「更新渲染」时前进; 不然一个任务里先起的动画到后面读 `getComputedStyle` 时已经多走了几十毫秒,
  WPT 插值测试的「100s 时长、-50s 延迟」会差 0.01px). 出帧线程的重算不在任务里, 照旧用墙上时间; 时间只进不退. `Frame {plane, panels, size, scale, animating, commit, trace}` (size 是物理像素, scale 是画这一帧时的缩放: 前端靠这两个认出改尺寸或缩放之前画、晚到的帧): plane 是整页 (含画布背景, 不含面板); `panels` 是
  `Dom::set_panel` 钉住的元素 `Panel {id, size, plane, extent, reach}`: extent 是面板里实际画到的范围 (物理像素, 相对这一帧的面板矩形), reach 是同一块换成页面坐标 (CSS 像素, 用这一帧画面板时的矩形和缩放算, 前端报 `panelmove` 用它, 不拿自己记的矩形去拼: 缩放切换时两边不同步, 拼出来的位置是错的; 尺寸或缩放和窗口现在的不一样的帧是改之前画的, 前端不报它的范围, 不然页面会拿旧布局的范围去并新的盒子). `Plane {drawing, layers, damage, damage_rects (最多 8 块), bounds}`;
  layers 是有 backdrop-filter / filter 时切成的段, 每段 `{drawing, bounds, backdrop, filter}` (`Drawing` 出 `scene` / `scene_for`): backdrop 先处理画布上已画的那块再画这段, filter
  先画进临时纹理再合成. 光栅化方 (src/vello) 做 filter 的 drop-shadow、模糊、颜色矩阵, backdrop 的模糊和所有颜色函数 (`Backdrop::effect`:
  按顺序合成一个颜色矩阵, 几个 blur 按平方和开方合并); backdrop-filter 里的 drop-shadow 不做; 只切最外层: 已有 filter 的元素里面再有 filter / backdrop-filter 时不做 (#46);
  这两种同一组滤镜只记一次 warn (最多记 64 组).
  窗口之间的毛玻璃 (`setBackdrop`、面板的 `backdrops`) 仍只带 blur / saturate / brightness.
  磨砂的角形 (2026-11-22): blitz-paint 推带 `backdrop-filter` 的图层时用 `push_shaped_layer` 带上元素的 `Corners {radius, exponent}`
  (`acde-scene` 的 `corner`; `exponent` 是超椭圆指数 `n = 2^k`, 2 是圆弧, CSS `squircle` 是 4), `render::Backdrop` 记下它: `radius()` 有声明时
  用声明的半径 (乘图层变换的缩放), 不再从背景路径猜; `exponent()` 没有声明时是 2. GPU 合成的 `gpu::Backdrop` 多一个 `exponent`, 着色器
  用 acde-scene 导出的 `corner::WGSL` (`acde_corner_coverage`, 建着色器时拼在 `compose.wgsl` 前面; acdes 的 compose 同样拼它), 和 `corner::coverage`
  逐行对应, 公式只在 acde-scene 一处; 前端把面板的磨砂交给
  合成器时带上 `exponent()` (acdes 用 `acompositor_backdrop_v1` 版本 2 的 `add_shaped_area`). 窗口级的 `setBackdrop` 也带角形 (2026-11-22, issue 16):
  `effect.shape` 收 `round` / `squircle` / `superellipse(k)` (k 取 1 到 3, 和 `setWindowLook` 的 `corners.shape` 同一个解析 `cornerShape`, `js/page/src/frametheme.ts`),
  胶水只传 k, 原生层 (`webnative.rs` `set_backdrop`) 用 `acde_scene::corner::exponent` 换成 n, `SurfaceCommand::Backdrop` 的效果是
  `[半径, 模糊, 饱和, 亮度, 指数]`; C 接口不改结构, 指数放在 `AcdeCommand.thickness` (Backdrop 本来不用它). 没写 shape 时是圆弧 (n = 2).
- 模糊 (src/vello `Composer`、shell `effects.rs`) 是降采样 / 升采样链, 几层、采样偏移多大由 acde-model `Blur::of(sigma)` 一处决定
  系统玻璃 (2026-11-22, 总仓 `.scratch/libacde/glass-plan.md`): 元素有 `backdrop-filter` 又声明了作者自定义属性 `--acde-glass` (`regular`、`clear`, 其余分项
  `--acde-glass-refraction`、`-blur`、`-bleed`、`-highlight`、`-white-point`、`-smoothing`、`-tint` 盖在预设上; `-highlight: <角度> <强度>` 的角度按 CSS
  的约定, 0deg 朝上、顺时针, 表示光从哪边来, 打亮朝向光源的边, 预设 315deg 是左上来光 (2026-11-22 改, 原来按数学角且 y 朝下, 135deg 打亮了左下;
  `-tint` 的颜色认 `#rgb`、`#rgba`、`#rrggbb`、`#rrggbbaa` 与 `rgb(r g b / a)`) 时, blitz-paint (`render/glass.rs`) 从计算样式取这几个
  自定义属性的文本, 交 `acde_scene::glass::Glass::from_properties` 解析 (解析只此一处; 写错的分项留空并记 warn), 和角形一起作为 `LayerShape` 随
  `push_shaped_layer` 带出. `render::Backdrop::glass()` 给出它 (长度已乘图层变换的缩放), `gpu::Backdrop` 多一个 `glass` 字段. 玻璃的算法、线上格式
  (`to_wire` / `from_wire`, 25 个小端 f32)、`scaled` / `reach`、CPU 简化版 (`glass::cpu::paint`: 最小和最大两层模糊按两层的不透明度归一、在预乘空间混合, 区域外的渗出是预乘底色乘渗出系数, 和 GPU 一致; 高光、白点、着色按直通色算; 不做折射) 和
  逐像素的 WGSL (`glass::WGSL`, 要先拼 `corner::WGSL`) 都在 acde-scene; CPU 的盒式模糊也只此一份 (`acde_scene::blur`, 原 acompositor `frost.rs` 原样搬来).
  合成器经 `acompositor_backdrop_v1` 版本 3 的 `add_glass_area` 收玻璃 (acdes). 进程内的 GPU 合成 (`src/vello/compose.rs`, 宿主窗口用) 也画玻璃
  (2026-11-22): 取样区域是元素外扩 `reach()`; 五层里不透明度大于 0 的每层用现有的模糊管线 (`blur_tagged`) 各模糊一次, 按 `Pipe::Mix` 依次插值成按不透明度归一的
  混合 (第 i 层的权重是它的不透明度除以已混合各层之和), 再过一遍 `backdrop-filter` 的颜色矩阵 (`Pipe::Filter`), 最后 `fs_glass` 逐像素做折射取样、高光、白点、
  着色、平滑边缘和渗出; 着色器拼的是 acde-scene 的 `corner::WGSL` 和 `glass::WGSL`, 和合成器同一份函数. 参数经 `Params`: 矩阵前三列放折射、渗出与高光、
  白点与平滑, `bias` 是取样区域, `color` 是着色.
  (也给出窗口之间毛玻璃的取样外扩 `margin`): 偏移在约 0.5–1.3 之间连续调 (一层时从 0 起, 五层时最多到 2), 强度随 sigma 连续变,
  近似同 sigma 的高斯 (`SPREAD` 是 DX12 上量出的每档方差, `tests/vello_compose.rs` 按相位平均量边缘的展宽对照); sigma 小于约
  0.96 px 时一层不偏移, 再按 `mix` 和原图混合. sigma 为 0 不模糊 (drop-shadow、backdrop 只拷一份). shell 的 GLES 链不做 `mix`.
- 动画曲线 (`acde_scene::motion`, 2026-11-22, 总仓 `.scratch/system/window-motion.md`): 合成器的窗口动画只用这一份. 两档 `Preset`: `Standard` 是临界阻尼的弹簧
  (响应 0.35 秒, 不过冲, 约 0.51 秒落到 0.1% 内, `Curve::length`), `Reduced` 是 0.15 秒线性淡入淡出且不缩放、不位移 (`moves()` 为假); 桌面只能选档, 不能给数值.
  `Ease` 是 CSS 的五个缓动关键字 (`linear`、`ease`、`ease-in`、`ease-out`、`ease-in-out`, 三次贝塞尔按 x 二分求 y), `acde:windows.animate` 的 `easing` 用它;
  两者的线上编号 (`to_wire` / `from_wire`, 按名字查 `Ease::wire_of`) 供 acdes 的合成器协议用; 弹簧落到 0.1% 内的时刻按常量算, 不在每帧求解.
  `Curve::css(n)` 把曲线按 n 段采样成 CSS 的 `linear(…)` 缓动, 时长取 `length()`:
  页面要和合成器的动画同步时 (桌面切换时滑动桌面层) 从 `acde:desktop` 拿这两个值, 不在页面里另存一份 (用户 2026-11-22 选 B).
- `Dom::capture(id, scale)`: 单独录一个元素 (拖动图标), 单边超过 2048 缩小; JS 拿 `picture` 编号 (`dom.rs`, 每个 web 留最后一个).
- canvas 2D (acde-dom `dom/canvas.ts` 编成命令流, `Dom::canvas`): 要像素的都在 GPU 上 (render `gpu.rs`, 用 `acde::gpu::Gpu::publish` 登记的设备):
  超过 20000 条命令 (`ACDE_CANVAS_MAX_COMMANDS`) 画成纹理再接着记, canvas 当图片源是 `GpuImage`, `shadowBlur` 用 `blur.wgsl`,
  `getImageData` / `toDataURL` (`toBlob` 经它) 同步读回. 没有设备 (`Gpu::publish(None)`, 如 acdes 用 CPU 画页面时) 画布没有纹理, 这两个改由
  `acde::render::gpu::set_cpu_raster` 登记的 CPU 光栅器把画布的场景画成整张 RGBA8 (非预乘、逐行、宽 × 高 × 4 字节, 长度不对当作没画出) 再裁出要的区域;
  谁也没登记时照旧给全透明 / `data:,` (`toBlob` 得到 0 字节, libacde 单子 13). 提交都拿 `Gpu::submit_lock`. `drawImage` 一张 SVG 的 `<img>` 时按它的自然尺寸画成场景
  (blitz-paint `svg_picture`, 和 `<img>` 同一画法). `ctx.filter`: acde-dom 解析 `<filter-value-list>` (无效的值不改, 长度只认 px),
  命令 14 送效果 (blur、brightness、contrast、grayscale、hue-rotate、invert、opacity、saturate、sepia、drop-shadow; url() 不支持);
  有滤镜时每次绘制先把它的范围 (加上效果外扩, 裁到画布) 画进一张图, src/vello `Offscreen::filtered` 依次做 (`blur.wgsl` 的
  `blur_rgba` 全通道模糊、`matrix.wgsl` 颜色矩阵、drop-shadow 是 alpha 模糊再垫在下面), 再画回去; 阴影还按没滤过的形状算.
- `createImageBitmap` / `ImageBitmap` (acde-dom `dom/bitmap.ts`): 把 `<img>`、`Blob`、`ImageData`、canvas、`ImageBitmap` 画进一个不挂在文档上的
  2D 画布, 支持裁剪矩形、`resizeWidth` / `resizeHeight`、`imageOrientation: 'flipY'`、`premultiplyAlpha` (WebGL 上传时按它, 不看
  `UNPACK_FLIP_Y_WEBGL` / `UNPACK_PREMULTIPLY_ALPHA_WEBGL`); `<video>` 来源报 `NotSupportedError`. 2D `drawImage` 和 WebGPU 外部拷贝收它.
- 选项 `default_lang` (默认 zh-Hans)、`dark` (事后用 `set_dark` 改, 都只改
  viewport, 媒体查询和 `matchMedia` 跟着变). `semantics(root, limit, offscreen, invisible)`: offscreen 时不按可见范围剪, invisible 见 `docs/web/inspect.md`「看和操作桌面」. `<iframe>` 不加载
  (`skip_iframe_content`), 只是空盒子. 增量录制见 vendor/blitz; `ACDE_PAINT_VERIFY=1` 每帧再整页录一次比对 (测试用). 图片缓存最多 256 张 /
  64 MB. 所有入口 `catch_unwind`.
- 窗框 (`decoration.rs`): `Decoration::scene(&Bar {title, width, height, scale, style, fullscreen})` → 物理像素的场景 (背景 #f4f2f7; `FrameStyle::Mac`:
  左边三个 12 px 圆点 (关闭 / 最小化 / 全屏), 标题居中截断; `FrameStyle::Windows`: 右边三个 46 px 宽的按钮 (最小化 / 最大化 / 关闭, 1 px 线画的图标,
  `fullscreen` 时最大化换成还原), 标题靠左截断; 没有悬停效果). `FrameStyle::current()`: `ACDE_FRAME_STYLE` (`mac` / `windows`), 没设或别的值时
  Windows 上的单应用是 `Windows`, 别处是 `Mac` (宿主窗口给出完整桌面时 `FrameStyle::follow_session`, 跟 Linux 会话一样). `Bar::button_at(x, y)` 是按钮命中, `FrameStyle::grab_hints` 是标题栏 (按钮区外) 和四边四角的 hint. 测试 `tests/decoration.rs`.
  `Bar.theme` (`FrameTheme`: `active` / `inactive` 两组 `FrameColors {background, title, line, buttons}`, RGBA, 没给的用默认; 失焦的组没给的项用
  有焦点的; `style` / `side` (`ButtonSide`) / `align` (`TitleAlign`) 没给时按 `Bar.style`: `Mac` 是左边、居中, `Windows` 是右边、靠左) 和
  `Bar.active` 只改样子; 按钮放在哪一边时命中和 `FrameTheme::grab_hints` 跟着换边. `buttons` 给圆点上色 (不给是红黄绿), 给图标上色 (不给是标题色).
- 帧率面板 (`hud.rs`, 宿主窗口的调试用, 谁画、数从哪来见 acdev `docs/hostwin.md`「帧率面板」): `Hud::scene(Option<&HudStats>, scale)` → `Hud::SIZE`
  (236 × 112 逻辑像素, `Hud::pixels(scale)` 是物理像素) 的场景: 圆角深色底, 大字 FPS (按平均帧间隔着色: 不超过预算的 1.25 倍绿、2 倍以内黄、
  更长红), 平均 / 最长帧间隔和每帧光栅化时间, 下面是最近 `Hud::BARS` (120) 帧的帧间隔柱 (同样着色, 高度到 3 倍预算封顶) 和 1 倍、2 倍预算两条线.
  `None` 是「idle」(一秒内没有新帧). 字和窗框共用 `draw_line`, 等宽字体 `ui-monospace, SF Mono, Menlo, monospace`.

## shell (napi 类 `Shell`)

会话里的合成器是 winman (协议、DRM/KMS、HiDPI 见 winman 的 docs), 远程桌面是 acdes 的远程进程;
宿主窗口里是 acdev runtime 的 `crates/hostwin`, 见它的 `docs/hostwin.md`. 下面是 Broker 和 web 看到的接口.

- 启动器侧的窗口系统对象: acdes 里是 `native.Windows.connect()`, acdev 宿主窗口里是 `native.Shell.broker()`. 都有 `onEvents(cb)` / `pump()`、
  `grant({desktop?, window?, mode?, rect?, size?, parent?, modal?})` → `{window, token}` (随机 64 位令牌, acdes 的窗口客户端在本进程里按令牌取回)、`place`、`windows()`、
  `shortcut` / `unshortcut` (冲突抛错)、`focus`、`hide`、`close`、`socketName`、`screenshot(path)`; 宿主窗口另有 `release`、`animate`、`stats`、`inject` 等.
- web 侧: `claimSurface(token, {frame?})` (src/host `webnative.rs` → 前端的 `Platform::claim`, 会话里是 acdes 的 `crates/window` `Surface::claim`: 连上 winman、开 toplevel 或 layer 面;
  一条 `acde-window` 线程管 Wayland 和光栅化) 给出 surface: `pump`、`wait()` (队列非空时落定)、`waitThen(cb)` (队列非空时在一个新任务里调 cb, 不在微任务里: acde-host `Host.run` 每轮取一批输入就是一个任务, 输入事件经 acde-dom `Input` 用 `fireFromTask` 派发, 每个监听器后跑微任务检查点, React 的离散事件在下一个按键前就提交; 原先循环 `await wait()` 之后都在微任务里, 检查点不生效, 连按三次只算一次)、`requestFrame`、`setClipboard`、
  `readClipboard()`、`startDrag(data, icon?)`、`setTitle`、`setImeArea`、`window`、`hosted`、`contentOffset`.
- 事件 (`Event`, 坐标逻辑像素, button / code 是 evdev 码). 给 surface 的: `pointermove` / `pointerdown` / `pointerup` / `wheel` /
  `pointerleave`、`keydown` / `keyup {code, key}`、`imepreedit` / `imecommit` (宿主窗口)、`activate` / `deactivate`、`framebutton {key}`、
  `frame {time}`、`resize {width, height, scale}` (页面的物理像素, 已减去窗框)、`dndenter` / `dndmove` / `dndleave` / `dnddrop {x, y, data}`
  (data 是类型 → 文本, 松手时已读好)、`dragend {button}` (1: 放到了别的窗口上)、`grab` / `grablost`. 给 Broker 的: `output`、`windowopen` /
  `windowclose` / `windowtitle` (原生窗口)、`windowstate {window, state, previous}`、`windowactivate`、`windowanimated`、
  `shortcut` (转给 owner)、`windowgone {window, x, y, width, height}` (web 窗口最后的 rect, 全屏的是之前的)、`windowminsize`、`windowparent`、
  `windowcloserequest` (宿主窗口)、`devreload` / `devrequest`、`milestone`、`inputtarget` (`ACDE_DEBUG_INPUT`)、`exit`.
- 注入 `inject({type, x, y, button, dx, dy, source, code})`: `move` / `motion` (相对) / `down` / `up` / `wheel` (source `wheel` / `finger` /
  `continuous`) / `keydown` / `keyup`, 和真实输入同一条路由.

## GPU 与性能 (硬约束)

- **整条链路不许 CPU 拷像素、不许 GPU 回读**. 例外: RDP 后端 (回读损坏区域)、截屏 / capture、标准接口要同步给像素的 (`getImageData` /
  `toDataURL`).
- **网页优先在 GPU 上画, 没有 GPU 时回退 CPU** (用户定 2026-11-13): 没有 vello_cpu; `acde::gpu::adapters` 真 GPU 排前, CPU 类型的设备 (lavapipe)
  排最后, 只有它时用它 (虚拟机 / VNC, 慢). 这时页面的 WebGPU 适配器 `isFallbackAdapter` (`GPUAdapter` 和 `GPUAdapterInfo` 上都有) 为 true, 页面可以据此降级 (src/host/webgpu `adapter_info`: 设备类型是 CPU). 什么设备都没有时网页图层不可用 (日志 `web layer unavailable`), 页面不出帧.
  合成器自己 (GLES) 不在此列. acdebin-system 的会话不走这条: 没有 GPU 时它在 CPU 上画页面, 不用 lavapipe (见它的 `docs/window-paint.md`).
- **两种后端, 按 feature 选** (用户定 2026-11-19): wgpu (src/vello) 和 detour (还没有, 以后在 `src/detour`). acdev 只开 wgpu: 三个平台一致, 给开发者用;
  acdes 开 detour 加 wgpu: Xe3 上走 detour, 别的 GPU 走 wgpu, 没有 GPU 时在 CPU 上画, WebGPU 总是 wgpu. wgpu 每个 OS 只编一个后端: Windows DX12、Linux Vulkan、
  macOS Metal (`acde::gpu::BACKEND`). Linux 另编进 wgpu 的 GL 后端 (`gles` 特性), 不进 `BACKEND`, 只经 `acde::vello::Device` 用
  (用户定 2026-11-20, GL 取证见总仓 `.scratch/render/wgpu-gl-probe.md`, 接口决定见 `.scratch/render/decisions.md`): 会话由 acdes 开始时探测一次
  (`ACDE_RENDER`: `detour:<节点>`、`wgpu:vulkan:<节点>`、`wgpu:gles:<节点>`、`cpu`, acdes 解析), 页面进程照它调
  `Device::open(Backend::Vulkan | Backend::Gles, 节点)`; 探测用 `Device::probe(backend, 节点)` (只开设备、不建 vello 管线, 成功时回 `describe()` 那一行).
  OpenGL ES 默认关: 开关 `ACDE_GL_ES=1` 由 acdes 的探测读, libacde 不读环境变量, 调了 `open(Backend::Gles, …)` 就开 (取证: 液态玻璃静止时还在出帧,
  开了之后静止时桌面进程 CPU 是 CPU 画页面时的十几倍; dopame 让它静止不出帧后再改默认).
  选适配器: 读 `/sys/class/drm/<节点名>/device/{vendor,device}`, 和 wgpu `AdapterInfo` 的 `vendor` / `device` 对上的用它; 对不上 (GL 后端常报 0) 时,
  非软件适配器恰好一个就用它; 有几个时 (WSL 的 D3D12 设备没有 PCI 编号, 渲染节点挂在 faux_driver 下) 先挑名字含 `MESA_D3D12_DEFAULT_ADAPTER_NAME`
  的 (Mesa 在 WSL 里按名字选卡的同一个变量), 再挑独显, 记一行 info. EGL 无表面平台由 Mesa 自己挑设备, 不能按节点指定, 所以 GLES 下这个比对只是核对.
- 不合规的 Vulkan 驱动 (2026-11-22, 用户要 WSL 用真显卡): wgpu-hal 把一致性版本为 0 的驱动 (WSL 的 Dozen, 即 dzn) 藏起来, 除非实例带
  `InstanceFlags::ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER`; libacde 建实例的地方 (`gpu::adapters`、`vello::Device` 的枚举) 都带上它. 宿主窗口和
  会话里自开设备的 `Gpu::open` / `Gpu::open_with(label, fits)` 按名字 (`MESA_D3D12_DEFAULT_ADAPTER_NAME`) → 合规独显 → 合规集显 → 不合规独显 →
  不合规集显 → 其他 → 软件 (llvmpipe) 逐个试: 开不了设备就换下一个; 不合规的还要过 `fits` (宿主窗口传 `Vello::probe`, 在当前线程建一遍 vello
  的管线), 不过也换下一个, 软件适配器始终是最后的退路, 换过时记一行 warn. 驱动在开设备时直接段错误 (有人报过 NVIDIA 上的 Dozen) 接不住,
  这时用 `MESA_D3D12_DEFAULT_ADAPTER_NAME` 指定别的卡.
  `open(Backend::Gles, …)` 开好设备后在当前线程上、在 Validation 和 Internal 错误作用域里、包着 `catch_unwind` 建一次 vello 的全部管线
  (`Vello::on_this_thread`, vello 的 `num_init_threads = 1`: 错误作用域只管本线程, 多线程建管线时错误漏到未捕获的处理里会被 wgpu 当致命错误 panic),
  结果在 `fits_vello()`; 为假时调用方不给它装 vello 合成器, 画布留在 CPU 上. Vulkan 设备 `fits_vello()` 恒为真. GLES 设备没有 dma-buf 导入和
  sync_file (`import` 报错, `arm` / `sync_file` 回 None), 只给画布、WebGL、WebGPU 用. `describe()`: 适配器、后端、驱动串、档位, 以及 vello 依赖的限额
  (每阶段存储缓冲数、计算工作组尺寸和调用数、工作组共享内存、存储缓冲大小) 和 downlevel 标志. `open_hardware()` (Vulkan, 不看节点) 留给还没改成按
  `ACDE_RENDER` 选设备的调用方. 只建 OpenGL ES 上下文靠 `vendor/wgpu-hal` 的补丁 (见 `vendor.md`). 2D 场景 (`PaintScene` / `Scene` / `RenderCommand` / `Filter`, blitz-paint 和 render 直接画进它) 是 `acde-scene` 的;
  渲染器 (`Vello` / `Raster` / `Composer` / `Device` / `Offscreen` / `GpuImage`, Linux 上 dma-buf 画布导入和 sync_file) 在 src/vello; src/gpu 的 `Gpu`
  (设备登记, 唯一一份) 包着 src/vello 的 `Shared`. acgpu 和 detour-gpu 0.1 都不再用. acdes 的 crates/paint 只剩线程、GBM 画布、损伤和区域、把帧拆成 `Part`
  (`plane.rs`), 以及 CPU 画页面.
- Windows DX12 的着色器编译器是静态链接的 DXC (wgpu `static-dxc`, mach-dxcompiler 的预编译库): 不用 FXC (FXC 编的 vello 着色器约 10 秒后
  `DXGI_ERROR_DEVICE_REMOVED`), 不用 dxcompiler.dll.
- 原生应用经 linux-dmabuf v4 零拷贝, 全屏且格式合适时直扫; pre-commit 钩子等 syncobj acquire point 或 dmabuf 的读 fence. 缺口: presentation
  不是硬件时间戳, 没有 tearing-control 和 VRR.
- 网页图层 (会话): vello 画进 web 自己 GBM 分配的 linear dmabuf. 交上去时不等 GPU 画完: 光栅线程的 sync_file 作为写 fence 挂到画布 dma-buf 上 (隐式同步, `DMA_BUF_IOCTL_IMPORT_SYNC_FILE`) 就 commit, 合成器在 GPU 上等它 (`Device::wait_for`: sync_file 临时导入 64 个二值信号量里的一个, 挂进下一次提交的等待; 导入失败把 fd 退回, 调用方在 CPU 上等), CPU 读画布时内核在 `DMA_BUF_IOCTL_SYNC` 里等; 挂不上 fence 的内核上照旧等 sync_file 落定再 commit. 以前每帧都等 GPU 画完才 commit, 页面这一帧的 GPU 时间整段压在帧循环里 (.243 Xe3 1080p 压力页约 4 ms). 光栅化线程 `acde-web` 一次一帧, CPU 编码和 GPU 执行
  重叠, 挪窗口不重新光栅化, 闲 2 秒放掉中间纹理. 一帧里所有窗口、所有图层按损伤区切出的块 (`Part::pieces`) 排进一张图集
  (`Raster` 的 `Atlas`, 架子式摆放, 先排高的; 宽高取这一帧最大的画布、至少 4096, 不超过设备的 2D 纹理上限; 放不下时退回按顺序分批), 整张图集
  一次 `Vello::render_clipped` (每块裁在自己的格子里), 再按原顺序在同一个编码器里清区、背景模糊、阴影、贴回画布 (`Composer` 的 `at` 是块在图集里
  的位置; 同一画布上连续的无阴影无滤镜块并成一个渲染通道); 只在编码器里有读图集的操作时才在 vello 之前提交一次. 原来每块一次 vello 渲染加两次提交,
  损伤区多 (最多 48 块) 时一帧几十次提交.
  不带滤镜和背景模糊的图层 (`render::Layer::areas`, 只在没有非 SrcOver 合成的图层上给) 不再取整块外接框: 它的绘制矩形向外取整到像素 (加 2 像素)
  后两两相减成互不重叠的矩形 (超过 64 块退回外接框), 只按这些矩形切块. 第一个特效 (滤镜、阴影、背景模糊) 之前的普通块当底块; 后面的普通块完全
  落在某个底块里、且它和底块之间单独贴的块 (特效的范围含阴影和模糊外扩、背景模糊的采样区, 以及没并进去的普通块) 和更后的底块都不压到它时, 直接
  画进那个底块的格子 (按原顺序叠在底块之上, 裁在自己的范围内), 不再单独占一格、单独贴一次. .243 (Xe3, 1080p@144) 上满屏 60 张移动卡片、15 个
  滤镜层的压力页: 原来一帧 9.5 M 像素分 4 批 vello (约 9 M 像素是整屏的 4.4 倍), 现在 6.5 M 像素一批; 冻结帧和逐块贴的结果逐像素比过, 只差在
  字形抗锯齿像素的量化上 (合并后少一次 8 位往返, 字色更准). 两套 vello 着色器都在会话开始时编好 (#109): 光栅化线程自己的 (`vello warm-up`) 和
  页面画布、阴影、截图用的进程级 `Offscreen` (src/render `gpu.rs`, 进程里第一个光栅化线程起来时 (`Once`, 进程里可能有多个 `Raster`) 在 `acde-warm` 线程里 `gpu::warm()`, 日志 `offscreen vello warm-up`),
  不再等第一个用到画布或阴影的窗口打开时才编, 编的时候还拿着全进程一把 `Offscreen` 锁. 宿主窗口 (Windows、macOS、Linux 宿主) 同样在
  `hostwin/src/app.rs` 的 `run` 里 `Gpu::publish` 之后起 `acde-warm` 线程预编这套 `Offscreen` 着色器.
- 帧预算: 120 fps 下每帧 8 ms. 盯 acde-dom `test/cost.test.ts` / `test/caches.test.ts`、shell / DomNative 的 `stats`、`drive.sh perf`; 提交里写测到的数.
- WebGPU (`navigator.gpu`, Linux 和 macOS; Windows 上还关着 (没编过也没跑过, 见 acdev #56)): src/host `webgpu/`, 跑在光栅化线程的设备上; 回调由 `acde-gpu-poll` 线程非阻塞轮询 (每帧路径上不用
  `poll(Wait)`, 它拿着 snatch 读锁会卡住别的线程的提交). WebGL 也只经 wgpu, 见 `docs/graphics/webgl.md`.

## src/render 的已知问题和测试数据

- canvas 的 `createPattern` 在第一次用时才取来源的位图 (规范是创建那一刻).
- vello 0.11 没有图片的场景把图集当 1x1 重建、已上传的图片画空; 每个场景垫一块 1x1 渐变绕开 (src/vello `Vello::render`), 升级 vello 时看
  上游修了没有.
- `ACDE_PAINT_VERIFY=1` 下机器忙时 `tests/damage.rs` 偶发等帧超时, 单独重跑都过.
- `tests/fixtures/replay/*.ops.gz` 是旧 acde-host 的 DOM 操作流 (`tests/replay.rs` 回放, 格式见它), `svg/*.svg` 是简单 SVG 的对比用例, 元素的特殊渲染
  在 `tests/elements.rs`. `release_keeps_memory_flat` 只在 Linux 上跑.
