# vendor

依赖来自 crates.io, 加上两处 git: `vendor/blitz` 是子模块 (Gitea `AC/blitz`, `.gitmodules` 写相对地址 `../blitz.git`: 跟着本仓库的 origin 走, ssh 克隆就是 ssh, CI 用 http 加令牌克隆就是 http (这台 Gitea 不许匿名读, 带令牌的 `-c http.extraHeader` 也要给 `submodule update`); fork 自 DioxusLabs/blitz, 原来在 GitHub `Jabberwocky238/blitz`, 2026-11-14 整个搬进内网), blitz 的 taffy
照上游 Blitz 钉 DioxusLabs/taffy 的 git 提交 `4e59968`. 根 `Cargo.toml` 用 `[patch.crates-io]` 把 blitz-* 换成子模块里的. 克隆后 `git submodule update --init`; 打包 (acdebin-system 的 PKGBUILD) 和 CI 也取子模块.
小补丁不留: stylo、parley、fontique、vello、taffy、libseat-sys、fslock、async-dnssd 都用 crates.io 原版 (servo 版 stylo 没有的 `@container`、
`accent-color`、`field-sizing`、`prefers-reduced-motion` 和 SVG 的 `pointer-events` 取值随之没有).

| 目录 | 上游 |
| --- | --- |
| `blitz` (子模块) | DioxusLabs/blitz main `fb297a38` 上接我们的提交: 只留 blitz-dom / blitz-paint / blitz-traits / stylo_taffy / debug_timer (`MIT OR Apache-2.0`) |

blitz fork 和上游结构的不同: HTML 解析用 src/render 自己的 html5ever TreeSink; `build_inline_layout_into` 在 `layout/inline/build.rs`; `paint_scene`
先录进 detour 的 `Scene` 再回放 (没有 anyrender, 它的 SVG 画法成了 blitz-paint 的 `svg.rs`); blitz-dom 重新导出 `taffy`. 验证: 在
`vendor/blitz` 里 `cargo clippy --workspace --all-targets`、`cargo check -p blitz-dom --no-default-features`、
`cargo test -p blitz-dom -p blitz-paint`、`cargo clippy -p blitz-paint --all-targets --features scrollbars`; 再 `cargo test --release -p acdert --test 'render_*'`
(加 `ACDE_PAINT_VERIFY=1` 再一遍).

**blitz-paint**

| 文件 | 改了什么 |
| --- | --- |
| `cache.rs` (新), `render.rs` `record` / `ensure` / `flatten`, `lib.rs` `paint_scene_cached` / `paint_scene_reference` | 增量录制: 按绘制边界 (根、stacking context、裁剪 / 滚动容器、图片、SVG、canvas、大列表的块级子元素) 缓存片段, `Dep::holds` 核对后复用, 帧按引用展开成 `Piece`; 表单控件和 iframe 每帧重录; LRU; 整页录制给 verify |
| `render.rs` `render_element_part` `border_box` | 边框盒在元素自己的坐标里 (否则挪过的片段不能复用) |
| `text.rs` `embolden` | 合成粗体: 字体没有要求的那么粗的字面 (fontique 的 synthesis `embolden`) 时照 Skia (Chrome) 加粗轮廓: 9 px 以下是字号的 1/24, 36 px 以上 1/32, 中间线性, 每边一半; 上游只有 `font-embolden` 那种给所有字都加的苹果式描粗 |
| `layers.rs`, `render.rs`, `render/background.rs`, `render/box_shadow.rs` | `push_layer` / `maybe_with_layer` 代替 `LayerManager` (上游超过 1024 层丢裁剪), 不建多余的裁剪路径 |
| `debug_overlay.rs` (删) | 去掉 devtools 叠加层和 euclid |
| `lib.rs` `paint_element` | 只画一个元素和子树, 不带祖先的变换 / 透明 / 裁剪 (拖动图标) |
| `lib.rs`, `render.rs` `flatten` / `render_element`, `cache.rs` `reuse` | `pinned` 元素 (面板) 不进帧, 单独展开返回 |
| `render.rs` `draw_canvas_background`, `render/background.rs` | 根 (或 body) 的整个背景铺满画布 |
| `render.rs` `draw_canvas_picture`, `cache.rs` `is_boundary` | `<canvas>` 画 2D 图, canvas 是绘制边界 |
| `render.rs`, `text.rs` | 行内元素按行画 padding / border / 圆角背景 |
| `render.rs` `draw_text_input_text` | 空输入框画 placeholder |
| `render/form_controls.rs` `draw_input` | 复选框和单选框: `appearance: none` 时不画控件 (元素自己的边框、背景、`::before` 照常); 选中时填文字颜色 (原版 stylo 没有 `accent-color`), 勾按和白色的对比度 (低于 3 用深色); 没选中的复选框边框是文字颜色 |
| `render/form_controls.rs` `draw_select` / `draw_picker`, `lib.rs` `paint_select_picker` | `<select>`: 下拉框的选中项文字和箭头, 列表框的行 (只画看得见的); 打开的选项列表 (render 画在帧的最上面) |
| `render/box_shadow.rs` | spread 加到圆角; 偏移和模糊乘缩放; 透明内阴影只跳过自己; 内阴影第一个在最上; 不模糊的内阴影一次填充 (padding box 加反向的洞, 重合的边抵消; 先填再 DestOut 挖洞时两条边落在同一像素里会剩一条淡线, xterm.js 的竖线光标) |
| `render/box_shadow.rs`, `render.rs` (滚动偏移、`draw_svg`), `render/background.rs` | 偏移在元素自己的坐标里加 |
| `render/border.rs` `draw_outline` | outline 沿中线、`outline-offset`、dashed / dotted、圆角、在内容之后、乘 opacity |
| `render.rs` `draw_svg` / `draw_simple_svg` / `draw_svg_tree` / `draw_svg_image` | 画 simple SVG 的命令; usvg 树相对元素画再放置; 没有 `viewBox` 的内联 `<svg>` 不拉伸到盒子: 用户单位就是 CSS px, 从内容框左上角画 (上游把 usvg 按内容包围盒定的尺寸拉满盒子, 偏大且偏移; 图片的 SVG 照旧拉伸, 浏览器给图片合成 viewBox); 背景和遮罩层 (`render/background.rs`) 也画 simple SVG 的命令 (图片加载时简单的 SVG 只留根元素的壳, 原先背景、`mask-image` 里的图标、下拉箭头一律不画) |
| `svg.rs` `mask_clips` / `filter` / `layered`, blitz-dom `serialize.rs` 写 `mask-type` (呈现属性也收) | 遮罩: 只有一条纯色无描边路径时按它裁剪, 不透明度是 alpha 遮罩的填充不透明度 / 亮度遮罩的颜色亮度乘它; 别的遮罩只裁到遮罩区域 (vello 没有遮罩层). 滤镜: 从 SourceGraphic 接起来的高斯模糊、drop shadow、颜色矩阵 (Figma 导出的透明 feFlood + feBlend 前缀放过) 变成合成器的滤镜层, 在组的用户坐标里; 别的原语整组不滤. 已知: 合成器里滤镜层外面的非矩形裁剪 (遮罩路径) 只按包围矩形裁 (同 #46 一类), Vite logo 的模糊光晕会溢出闪电形状, 在遮罩矩形内; canvas / 离屏里滤镜层不做 (vello 忽略) |
| `text.rs` `stroke_text` / `Ellipsis` / `draw_ellipsis_marker`, `render.rs` `ellipsis` | `text-overflow: ellipsis` 和 line-clamp 的 "…" (`Ellipsis.end` 是 frame 的内容盒宽, 已经是设备像素, 不再乘 scale) |
| `render/gradient.rs` | 色标位置按规范; 超出渐变线的色标: 线性拉长, 径向 / 锥形夹回 |
| `render.rs` `fieldset_frame` / `element_cx`, `render/border.rs` `draw_border`, `cache.rs` `visit` | fieldset 的框从 legend 中线开始, 边框在 legend 后留空; legend 算进片段依赖 |
| `render.rs` 开头 | `scrollbars` 下补 `Color` / `Stroke` 的 import |

**blitz-dom**

| 文件 | 改了什么 |
| --- | --- |
| `node/element.rs` `SpecialElementData::RangeInput`, blitz-paint `render/form_controls.rs` `draw_range` / `accent_color` | `<input type=range>` 画轨道和滑块 (`appearance: none` 时不画); 复选框、单选框和滑块都用 `accent_color` (文字颜色) |
| `mutator.rs` `set_attribute` | 不在文档里的 `<img>` 设 `src` 也取图 (HTML 的 update the image data; `new Image()` 预加载): 上游在文档外不做任何特殊处理, 图一直不来; render 的 `image_state` 也不再把文档外的图一律当作加载中, 取不到就报失败 |
| `layout/mod.rs` textarea / 文本类 `<input>` 的叶子布局, `document.rs` `with_text_input` | textarea 排完版把文字的高度 (加上下内边距) 算进 `scrollable_overflow_rect`, `scrollHeight` 是整段文字 (聊天输入框按它长高; 上游只有 rows 那么高) |
| `document.rs` `svg_with_document_refs`, `node/svg.rs` `missing_local_refs`, `node/serialize.rs` `write_svg_raw` | usvg 只在它解析的那份 SVG 里找 `<use href="#id">` / `url(#id)`; 页面常把 `<symbol>`、渐变放在另一个 `<svg>` (藏起来的雪碧图): 这份里没定义的 id 按文档找到 SVG 元素, 抄进 `<defs>`, 再找它引用的; 没算过样式的 (在 `display: none` 的 `<svg>` 里) 按原属性写 |
| `document.rs` `svg_box`, `node/serialize.rs` `write_svg_marked` / `SVG_BOX_ID`, `stylo.rs` SVG 呈现属性 | 内联 SVG 里的元素没有布局盒: `svg_box` 把所在的最外层 `<svg>` 再序列化一次, 目标元素包进 `<g id="__acrender_box">`, 交 usvg 解析 (文字排成路径), 取这个组在画布上的包围盒, 按画的时候的变换 (`SvgImageData::inline_viewport_transform`) 放进内容框, 换成内容框的比例; render 的 `rect` / `clientRects` 按 `<svg>` 的内容框换成页面坐标 (getBoundingClientRect, 和 acde-dom 里 getBBox 的文字、图片、use 部分). `font-size="20"` 这样不带单位的呈现属性按用户单位补 `px` (CSS 的 font-size 不收纯数字, 原先这一条被丢掉, 计算样式回到页面字号) |
| `stylo.rs` `match_non_ts_pseudo_class`, `assets/default.css` | `:popover-open` / `:modal` 看元素状态 (由 render `Dom::set_open_state` 设, 上游一律 false), `:open` 是 `details` / `dialog` 有 `open`; popover 的 UA 样式 (定位、边框) 搬出 Gecko 的 pref, 没有 top layer, `:popover-open` 和 `dialog:modal` 给最大 z-index 代替 |
| `has.rs` (新), `document.rs` 打开 `layout.css.has-selector.enabled` / `snapshot_node_and`, `mutator.rs` `set_attribute` / `clear_attribute` / `remove_node` / `remove_and_drop_all_children` / `add_children_to_parent`, `stylo.rs` `each_custom_state` | `:has()` (上游在 servo 里关着, 选择器不解析). 快照失效只看元素自己和右边的选择器, `:has()` 的锚在左边 (祖先、前面的兄弟), 所以状态变化、属性变化 (class / id / 其他属性名, 新旧值都算)、插入和移除前各调一次 Stylo 的 `RelativeSelectorInvalidator` (照 Gecko glue 的 `MaybeInvalidateRelativeSelector*`): 改之前调, 不带快照, 找到的锚记 restyle 并把祖先标脏; 样式里没有 `:has()` 时属性那条直接返回. `each_custom_state` 原是 `todo!()` (失效时会走到), 没有自定义状态, 什么也不做 |
| `animation_events.rs` (新), `stylo.rs` `resolve_stylist_pass`, `document.rs` `take_animation_events` / `has_css_animations` | 推进 CSS 动画和过渡时记事件 (animationstart / iteration / end / cancel, transitionrun / start / end / cancel; elapsedTime 照 Servo 的 `script/animations.rs`, 取到微秒): 上游 Servo 在 script 里发, Blitz 没有. 第一次 `take_animation_events` 之后才记, 最多攒 4096 个; 离开文档的元素上没结束的动画和没结束的过渡记 cancel, 已结束的动画被样式去掉不记 |
| `node/node.rs` `NodeFlags::IN_XML_DOCUMENT`, `stylo.rs` `is_html_element_in_html_document` | 上游写死 `true` (类型选择器和属性名全按小写比); 改成 HTML 命名空间且所属文档不是 XML 文档 (这个标记由 src/render 在 adopt 时设). SVG 的 `linearGradient` 这类名字在 HTML 文档里也按原样比 |
| `layout/damage.rs` `propagate_damage_flags` | 内联 `<svg>` 后代的损伤只重建这个 `<svg>`, 往上只传 REPAINT |
| `mutator.rs` `set_attribute` / `clear_attribute` / `restyle_for_attribute`, `document.rs` `snapshot_attribute` | 只影响选择器的属性只重算元素自己 (快照让后代失效; 有 `:has()` / nth-of 时重算父子树); 值没变直接返回 (`value`、`style` 和 `<img>` 的 `src` / `srcset` 除外: 同值也照常处理, `<img>` 会重新加载) |
| `mutator.rs` `set_attribute` / `load_image` / `process_added_subtree`, `node/element.rs` `ElementData.image_tried` / `image_loads`, `document.rs` 取图失败和 `apply_loaded_image`, blitz-traits `net.rs` `NetProvider::reused` | `<img>` 不在文档里时设 `src` 也加载 (上游只在文档里加载); 插入文档时当前 `src` 已经开始取过 (成功、失败或还在取) 就不再取 (克隆体没取过, 插入时取); 取自图片缓存或并入进行中的请求时调 `reused` 让提供者照样上报; 每次取图结束 (取到、失败、取自缓存) `image_loads` 加一, render `take_loaded` 按它每次结束报一次 (页面各触发一次 load / error, 和浏览器一样), 不再只在状态变了时报 |
| `node/simple_svg.rs`, `svg.rs`, `element.rs`, `layout/construct.rs` | 基本图形 + 纯色的 SVG 直接转 kurbo 路径, 不经 usvg (`<text>` 和嵌套 `<svg>` 走 usvg; `svg_text_only`; `ACRENDER_SVG_PATH=1` 打印) |
| `node/serialize.rs` `svg_source`, `assets/default.css` | 序列化内联 SVG 时写进算好的样式和字体, CSS 几何算成属性; 后代的 CSS `transform` / `translate` / `rotate` / `scale` (类、动画) 按视口算成矩阵写进 `transform` 属性, 盖过原属性 (根 `<svg>` 的随它的盒子画). SVG 元素 (除最外层 `<svg>`) 的 `transform-origin` 默认 `0 0`, 同 Firefox; 内联 `<svg>` 默认 `overflow: hidden` (同 Chrome / Firefox 的 UA 样式), 画出盒子的内容被裁掉; `color` / `fill` / `stroke` (和替换 currentColor 的值) 写成 sRGB 的 `rgb()` / `rgba()`, 超出色域的截到边上: usvg 不认 `oklch()`、`lab()`、`color-mix()` 等 (Tailwind 4 的调色板全是 oklch, 图标原先看不见或成了黑色) |
| `node/svg.rs` `external_uses` / `inline_external_uses`, `document.rs` `svg_with_sprites`, `net.rs` `SvgSpriteHandler` | 内联 SVG 里指向别的文件的 `<use href="icons.svg#id">` (#152; usvg 只认同一文档里的): 文件经 net provider 取一次 (按去掉片段的 URL 记, 取不到不重试), 到了把等它的 `<svg>` 重建; 序列化后把目标元素原样抄进根下的 `<defs>`, 改成 `acde-sprite-<n>-<id>`, `<use>` 指向它. 只抄目标元素本身, 它再引用文件里别的 id 不跟 |
| `stylo.rs` `synthesize_presentational_hints_for_legacy_attributes`, `SVG_PRESENTATION` | SVG 的呈现属性 (fill / stroke / opacity / font-* 这 18 个, 和序列化写回的一样) 按 CSS 解析成 PresHints: `stroke="var(--x)"` 能解析, `getComputedStyle` 也读得到; 含 `url(` 的值不映射 (照旧原样交给 usvg) |
| `util.rs` `parse_svg_image` / `parse_svg_with` | 按源文本缓存 usvg 解析 (512 条); 没有 `<text>` 用空字体库 |
| `net.rs` `ImageHandler::parse` / `best_icon_entry`, `Cargo.toml` image 的 `ico` 特性 | ICO / CUR 图片 (#203; 上游没开 `ico`, favicon 报错): 照浏览器取面积最大的一项, 同样大时取色深最大的 (image crate 自己先比色深), 抄成只有这一项的 ICO 再交给 image 解 (PNG 和 BMP 项都认); CUR 的项里第 4–7 字节是热点, 只按面积比, 抄出来写成 1 个平面、色深 0 (image 遇到超过 256 的值会报错); `ico` 带上 `bmp`, 单独的 BMP 图片也能解了 |
| `util.rs` `FONT_DB`, `document.rs` `svg_fonts` | fontdb 的通用字族用 fontique 选的; SVG `<text>` 能用 @font-face |
| `webkit_compat.rs` (新), `document.rs`, `net.rs`, `node/element.rs`, `cssom.rs` | 解析前改写 `-webkit-line-clamp` / `-webkit-box` / `-webkit-inline-box` |
| `layout/inline/*.rs`, `node/text.rs` | ellipsis 另排一个 "…"; line-clamp 截到第 N 行 |
| `node/inline_edge.rs` (新), `layout/inline/build.rs` / `layout.rs` | 有边距的行内元素两端各放一个零高行内盒 |
| `layout/construct.rs` `flush_inline_pseudos_recursive` | 按子元素自己的 display 决定是否往下走 |
| `resolve.rs` `needs_layout` / `resolve_layout_children`, `layout/damage.rs` | 没有布局损伤不跑 taffy; 只走有构造损伤的子树; hover 有变化才刷新 |
| `layout/inline/layout.rs` | min / max-content 宽度缓存; shrink-to-fit 断行宽度放宽 0.01 |
| `stylo.rs` `resolve_stylist` | 被取消的动画 / 过渡, 元素再遍历一次 |
| `stylo.rs`, `layout/construct.rs`, `node/text.rs` | placeholder 排版, `::placeholder` 按需计算 |
| `config.rs` `default_lang`, `tree.rs`, `document.rs`, `iframe.rs`, `stylo.rs` | 根没有 lang 时用默认语言 (-x-lang 继承, 传给 parley) |
| `node/node.rs` `hit_inner` | transform 不可逆的元素命中不到 |
| `config.rs` `skip_iframe_content`, `mutator.rs` `load_iframe` | `<iframe>` 不加载 |
| `document.rs` `set_canvas_picture` / `layout_canvas_text` | 存 `<canvas>` 的 2D 图; canvas 的文字排版 |
| `node/element.rs` `paint_epoch` / `repaint`, `node/text.rs` `generation`, `layout/list.rs` | 绘制缓存用的代数 |
| `document.rs` `cache_image` | 图片缓存 256 张 / 64 MB 淘汰最旧 |
| `document.rs` @font-face, `net.rs` `FontSources` | 同一字体只注册一次 |
| `document.rs` `geometry_generation`, `memory` / `MemoryStats` | 公开几何代数和内存统计 |
| `document.rs` `create_node` / `forget_layout_child` | 不记 `changed_nodes`; 删节点时从布局父节点的子节点列表去掉 |
| `mutator.rs` `flush`, `document.rs` `add_stylesheet_for_node` / `tree_order` | 跳过已删的节点; 样式表按文档顺序插进 stylist; 只有在文档里的 `<style>` 才加 |
| blitz-traits `net.rs` `NetHandler::destination` / `cors` / `failed` / `opaque`, blitz-dom `net.rs` `SheetLoad` / `SheetLoadState`, `document.rs` `fetch_linked_stylesheet` / `unload_linked_stylesheet` / `stylesheet_load` / `take_settled_stylesheets` / `stylesheet_source` / `media_list_of` / `update_stylesheet_media`, `mutator.rs` | 请求带 destination 和 `crossorigin` 的 CORS 设置; 取不到 (`failed`) 和空响应分开, 上游都当空表; 响应不透明 (`opaque`) 时记在 link 的表上, 连同取它的地址. `<link>` 的取表挪到 `BaseDocument`: 每个 link 记当前这一次取和它 `@import` 的表还差几个没答, 都答了报一次成败; 不是当前这次的答复丢掉 (上游后到的旧答复会盖掉新表); 取失败时去掉旧表; link 离开文档、没了 `href` 或 `rel` 里没了 `stylesheet` 时表和这次取一起去掉 (上游只在删 `href` 时去, 没有表就 panic); `rel` / `crossorigin` 变了也重取. `<link>` / `<style>` 的 `media` 属性是表的 media list (上游一律空 list), 改了即时生效 |
| `document.rs` `repair_after_panic` | panic 后去掉对已删节点的引用, 全部标脏 |
| `layout/damage.rs` `repaint_inline_root` / `clear_damage_and_dirty_flags` | 行内元素的损伤让 inline root 重画; `display: none` 子树的构造损伤留着 |
| `document.rs` `physical_unrounded_geometry` | 只减包含块的滚动偏移 |
| `layout/fieldset.rs` (新), `layout/mod.rs`, `layout/construct.rs`, `node/node.rs`, stylo_taffy `wrapper.rs` | fieldset 是 flow-root; rendered legend 是块级的直接布局子节点, 居中压在上边框上, 上边框占 max(边框宽, legend 高); 不建匿名 content box. 不做竖排、`align`、百分比定高 |
| `layout/list.rs` | outside 的列表标记按 `white-space: pre` 排 |
| `assets/default.css` | `::placeholder` 54% currentcolor; 表单控件默认黑字; `select` 是 inline-block, 列表框 (`[multiple]` / `[size]`, `size` 是 0 / 1 的除外) 纵向能滚 |
| `document.rs` `element_from_point` | 命中测试前加上视口的滚动偏移 (`hit` 从根元素的盒子算起, 是页面坐标): 上游整页滚动之后 `elementFromPoint` 回的是滚动前那个位置上的元素 |
| `resolved_style.rs` `css_supports_condition` / `css_supports_declaration` | `CSS.supports(条件)` 和 `CSS.supports(属性, 值)` 交给 stylo 的 `@supports` 求值; 两个参数的那种属性名要原样是标识符 (先过 `Declaration::parse`: stylo 的 `Declaration::eval` 对不是「标识符 + 冒号」开头的文字会 panic), 值里不许 `!important` |
| `node/select.rs` (新), `layout/select.rs` (新), `layout/construct.rs`, `layout/mod.rs`, `node/element.rs` `SpecialElementData::Select` | `<select>` 是叶子: 构造时按 list of options 给每个 option 和 optgroup 标签排一行 (select 的字体, 组标签加粗), 选中看 option 的 `CHECKED` 状态 (嵌入方设); 下拉框宽是最宽的标签加箭头, 高一行; 列表框高 `size` 行, `scrollable_overflow_rect` 含全部行; `SelectPicker` 是打开的列表的几何 |
| `scrolling.rs` `clamp_scroll_offsets`, `resolve.rs`, `document.rs` `scrolled_nodes` | 布局后把超出新滚动范围的偏移拉回来 (节点和视口; CSSOM View), 只看偏移不是 0 的节点; 上游内容变短后一直停在末尾之外, 顶上空着又滚不回去 |
| `layout/damage.rs` `propagate_damage_flags`, `document.rs` `rebuild_select` | select 下面有损伤 (option 的文字、属性、状态) 时重建它; 样式看不出的变化 (selectedness) 由嵌入方 `rebuild_select` |
| `assets/default.css`, `node/node.rs` `has_native_appearance`, `layout/construct.rs`, `layout/mod.rs`, `layout/damage.rs` | 复选框和单选框默认 `appearance: auto`, 不带 input 的边框、内边距和背景 (控件的边框盒仍是 20px / 18px); `appearance: none` 时是普通的盒子: 不当叶子、`::before` / `::after` 照常构造和排版, `appearance` 变了重建盒子 |
| 删掉的 | `events/`、`form.rs`、`debug.rs`、devtools、parallel-construct、`<canvas src=数字>`、CSS 光标、custom-widget |

**`src/webgl/glsl`**: naga 30.0.1 的 GLSL 前端整个搬进来 (原 `front/glsl`, MIT OR Apache-2.0), 跑在 crates.io 原版 naga 的公开接口上
(naga 私有的几样在 `glsl/compat.rs` 重写); 改动处加 `acde:` 注释. 加的是 WebGL 方言 (`Frontend::parse_webgl`、`WebGlOptions`: `front/glsl` 的 `Frontend::parse_webgl`、`WebGlOptions`: ES 1.00 / 3.00、attribute / varying、组合采样器拆开 (数组按常量下标、采样器参数带隐藏的采样器参数)、散 uniform 成块、`gl_FragColor` / `gl_FragData` / int `gl_VertexID`、矩阵 attribute 按列、出口翻 y 改 z、宽点、丢掉的接口变量), 另外 `const in` 参数、`modf`、精度语句接受采样器、嵌套深度上限 128 (表达式和语句; 深的源码报错而不是栈溢出)、数组层数 / 采样器数组长度的上界.
std140 下比步长窄的 scalar / vec2 数组元素包进 16 字节的结构体 `acde_padded { value }` (`offset::PADDED`), 取元素时自动接上 `.value`,
webgl 枚举 uniform 时 (`shader.rs` `unpadded`) 解开这层; 这样各后端 (MSL 没有自己的数组步长) 都保持 std140 布局, 不用改 naga 后端.
