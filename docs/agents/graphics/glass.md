# 模糊与系统玻璃

模糊是降采样 / 升采样链, 半径由 `Blur::of(sigma)` 一处决定; 系统玻璃 (`--acde-glass`) 的预设、材质表、玻璃组与透镜都写在这里, 参数只此一份.

## 模糊链与系统玻璃

模糊 (src/vello `Composer`、shell `effects.rs`) 是降采样 / 升采样链, 几层、采样偏移多大由 acde-model `Blur::of(sigma)` 一处决定
系统玻璃: 元素有 `backdrop-filter` 又声明了作者自定义属性 `--acde-glass` (`regular`、`clear`; `blur` (`Glass::BLUR`) 是和窗口材质 `blur` 同一份的背景模糊: 取 `Glass::blur(档位, 深浅)`, 没有透镜、高光、轮廓和折射, 跟着系统档位、深浅和"减少透明度" (此时是 `Opaque` 档, 着色不透明) 变, 页面的菜单、提示条、搜索框用它; 其余分项
`--acde-glass-refraction`、`-blur`、`-bleed`、`-highlight`、`-white-point`、`-smoothing`、`-tint` 盖在预设上; `-highlight: <角度> <强度> [<宽度>]` 的角度按 CSS
的约定, 0deg 朝上、顺时针, 表示光从哪边来, 打亮朝向光源的边, 对侧的边另有一条约三分之一强度的亮线; 宽度是从边缘往里的一条细带 (长度, 随缩放, 不写沿用预设),
强度在带内从边缘线性降到 0 (2026-11-22 改: 原来带宽借折射的 inner_height, clear 档 18 px 一整条加白, 面积太大; 苹果的镜面光是 1–2 px 的细边, 上亮下暗,
参考 https://github.com/ALEXalesha/LiquidGlass 、https://dev.to/vitalbadjo/liquid-glass-css-the-apple-liquid-glass-effect-with-css-only-plus-svg-refraction-1hkd ), 预设 315deg 是左上来光 (2026-11-22 改, 原来按数学角且 y 朝下, 135deg 打亮了左下;
`-tint` 的颜色认 `#rgb`、`#rgba`、`#rrggbb`、`#rrggbbaa` 与 `rgb(r g b / a)`) 时, blitz-paint (`render/glass.rs`) 从计算样式取这几个
自定义属性的文本, 交 `acde_scene::glass::Glass::from_properties` 解析 (解析只此一处; 写错的分项留空并记 warn), 和角形一起作为 `LayerShape` 随
`push_shaped_layer` 带出. `render::Backdrop::glass()` 给出它 (长度已乘图层变换的缩放), `gpu::Backdrop` 多一个 `glass` 字段. 玻璃的算法、线上格式
(`to_wire` / `from_wire`, 31 个小端 f32; 第 26–31 个是透镜厚度、色散、乘暗、斜面宽度、高光宽度、折射率, 只收到前 25 个的旧对端按 0 处理); `Glass::with_overrides(base, &[f32])` 用同一线上格式的数组覆盖 base, NaN 表示这一项不覆盖 (先认出 NaN, 再整体夹取), 多出的项忽略, 合成器 v4 的 `add_glass_area_role` 用它、`scaled` / `reach`、CPU 简化版 (`glass::cpu::paint`: 最小和最大两层模糊按两层的不透明度归一、在预乘空间混合, 区域外的渗出是预乘底色乘渗出系数, 和 GPU 一致; 高光、白点、着色按直通色算; 不做折射. 画不了折射就退成磨砂 : 玻璃靠透镜或折射成像 (透镜厚度大于 0 且折射率大于 1, 或内外圈折射量大于 0)、而自己的模糊比 `blur` 材质通透档 (6/12 px) 弱时, 两层模糊换成 `Glass::blur(Level::Clear, _)` 的两层, 其余参数 (着色、高光、轮廓、渗出) 照旧; 不然通透档 0.5 px 的玻璃在没有 GPU 的客户机上等于全透明, 能读清下面窗口的字. 只此一处 (`Under::of`), 合成器的 CPU 合成、acdes 页面的 CPU 光栅、组的 `paint_cpu` 都经它; 取样外扩仍按原玻璃的 `reach()`, 外扩之外按边缘像素延伸, 和 CSS `backdrop-filter` 的边缘处理一致. GPU 路径不变) 和
逐像素的 WGSL (`glass::WGSL`, 要先拼 `corner::WGSL`) 都在 acde-scene; CPU 的盒式模糊也只此一份 (`acde_scene::blur`, 原 acompositor `frost.rs` 原样搬来).
合成器经 `acompositor_backdrop_v1` 版本 3 的 `add_glass_area` 收玻璃 (acdes). 进程内的 GPU 合成 (`src/vello/compose.rs`, 宿主窗口用) 也画玻璃
: 取样区域是元素外扩 `reach()`; 五层里不透明度大于 0 的每层用现有的模糊管线 (`blur_tagged`) 各模糊一次, 按 `Pipe::Mix` 依次插值成按不透明度归一的
混合 (第 i 层的权重是它的不透明度除以已混合各层之和), 再过一遍 `backdrop-filter` 的颜色矩阵 (`Pipe::Filter`), 最后 `fs_glass` 逐像素做折射取样、高光、白点、
着色、平滑边缘和渗出; 着色器拼的是 acde-scene 的 `corner::WGSL` 和 `glass::WGSL`, 和合成器同一份函数. 参数经 `Params`: 矩阵前三列放折射、渗出与高光、
白点与平滑, `bias` 是取样区域, `color` 是着色.
玻璃底下取到透明像素 (底下的真实背景在这一层之外, 如钉住面板自己的透明底) 时不当成黑色 (胶囊和总管球发暗): CPU 的 `cpu::over` 和 WGSL 的
`acde_glass_over` 把已知底色和"只有着色"按底下的不透明度 a 加权, 颜色 = (着色后的底色 × a + 着色 × 着色不透明度 × (1 − a)) / 合成不透明度,
合成不透明度 = a + 着色不透明度 × (1 − a). 底下不透明时和原来一样; 全透明时就是着色本身, 由合成器盖在真实背景上. 页面合成 (`compose.wgsl`)、组 (`group.wgsl`)、CPU 都走这一处.
轮廓线 (`Glass.outline: Outline {width, color, merged}`): 写法 `--acde-glass-outline: <宽度> <颜色> [pieces | merged] | none`
(宽度随缩放, 颜色同 `-tint`, 第三项缺省 `pieces`). 预设各档默认开, 1 px pieces, 亮色页面 `rgb(255 255 255 / 0.5)`、暗色页面 `rgb(255 255 255 / 0.25)`:
`Glass::preset_in(role, level, dark)` 按 `preference().dark` 取 (渲染器在页面深浅变化时写进去并整页重画), `preset(role, level)` 是亮色; 不透明档
的着色也跟着深浅. 线是两侧都截止的带: 边缘往里 `width`, 外侧在边缘处 0.5 px 抗锯齿收掉 (`clamp(width + 0.5 + d) · clamp(0.5 − d)`; 只截内侧时, pieces 画法会把别块
"外面"的整片区域当成轮廓, merged 的线宽也会变成玻璃平滑边的宽度),
颜色按覆盖率盖在玻璃色上, 透明度取两者较大的. 两种画法:
- `pieces` (默认, 为了按钮区分度): 每块按自己的距离场画自己的轮廓, 取各块覆盖率的最大值; 融合时背景玻璃照样连成一团, 轮廓各画各的, 可以交叉.
- `merged`: 沿合成场整体外缘画, 用归一化距离 `d / max(|∇d|, 0.05)` (只在有块相互作用的地方算梯度, 其余就是 d), 脖子处平滑最小值把梯度压小
  也不会把线画粗, 线连续; 原来每块的内部边界离合成边缘远, 不画.
单块两种一样. 线上格式 31→37 个 f32 (末六个是轮廓宽度、颜色和画法), 组每块 9 个 vec4 (2304 字节; 轮廓宽度在 `corner.w`, 颜色在 `outline`,
画法在 `mode.x`), 页面合成的 `Params` 多一个 vec4 (160 字节; 再加查找表的 `table` 后 176 字节), 轮廓宽度在 `shift.y`. "逐位等于单块" 的几条测试都在轮廓关掉 (`none`) 时比; 组的
CPU 测试覆盖 merged 脖子连续、旧边界无线, pieces 旧边界都有线、脖子是玻璃无线, none 无线.

## 薄膜干涉

薄膜干涉 (`Glass.film: Film {amount, cell}`, `glass/film/`):
写法 `--acde-glass-film: <强度> | auto | none` (强度 0–1, `auto` 是 `Film::AMOUNT` = 0.2, 缺省 none 即不画). 页面是深色时强度乘 `1 − Film::NIGHT` (0.6),
解析时按 `preference().dark` 算好 (深浅变了渲染器整页重画, 重新解析). 公式和常量搬自 dopame 原来的液态玻璃 (`engines/film.ts` 的 `FILM`), 只在这里一份:
膜厚 `240 + 260·noise(P / 64px + t·(0.05, −0.035)) + 120·h` nm, 颜色 `0.5 − 0.5·cos(2π·2·1.33·膜厚·cosθt / λ)`, λ 取 (650, 532, 450) nm,
结果 `mix(c, c·(0.55 + 0.9·颜色), 强度·(0.25 + 0.75·smoothstep(0.2, 0.95, sinθi)))`, 夹到 0–1. h 和 sinθi 取自到边缘的距离 e: `x = e / 半短边`,
`h = √(1 − (1 − x)²)`, 斜率 `(1 − x) / h` (至多 7), `sinθi = 斜率 / √(1 + 斜率²)`, `cosθt = √(1 − sin²θi / 1.33²)`; 所以中间偏白, 越靠边颜色越浓.
组里 e 和半短边取融合后的合成场 (`AcdeGlassField.d` / `bevel`, CPU 的 `Field.bevel`), 颜色沿融合后的整体边缘走. 噪声是值噪声 (每格四角散列加平滑插值),
格子 64 px 随缩放 (`cell` 是长度). 加在着色 (白点、染色、乘暗、高光) 之后、轮廓之前. 漂移: t 是 `Film::clock()` (进程启动起的秒数; 减少动态效果时为 0),
只在这块玻璃本来就要画的时候取当前时间, 不为漂移出帧, 所以静止的玻璃不动. WGSL 的 `acde_glass_film(rgb, p, d, 半短边, film)` (`film` 是 (强度, 格子, t)) 拼在
`glass::WGSL` 末尾; 常量两边各写一次, 单测逐个比对 `film/film.wgsl` 与 `Film` 的常量. CPU (`cpu::paint`、`GlassGroup::paint_cpu`, 经 `Look::pixel_lined`)
用 `Film::shaded` 同一公式 (单块的 P 相对元素中心, GPU 是画布坐标, 噪声图案不同, 形态相同). 线上格式 37→39 个 f32 (第 38、39 个是强度和格子);
组每块 `mode.y`、`mode.z`、`mode.w` 是强度、格子和 t (块大小不变); 页面合成的 `Params` 多一个 `film` (192 字节). 合成器自己的单块着色器
要把 `Glass.film` 的 `Film::uniform()` 交给 `acde_glass_film`; 组走 `acde_glass_group_shade` 已经带上. 窗口材质的 `overrides` 也认 `film` (第 38、39 个).

## 预设表

预设表 (通透度档位; `glass/preset.rs`, 参数表只此一份): `Glass::preset(Role, Level)`,
`Role` 是 `--acde-glass` 的 `regular` / `clear`; `Level` 是 `Clear` (通透, 默认, 线上值 0: 模糊 0.5 px (背景有文本时页面自己切到 8 px, 不进预设), 内圈折射 0.8 (2026-11-23 用户定), 不染色、不渗出, 乘暗 0.05, 透镜斜面 34 px、厚度 22 px、折射率 1.5、色散 0.01 (用户定在调参页定), 观感照 ShengzhiWu/liquid-glass)、`Medium` (适中, 1, clear-3, 内圈折射按比例从 0.4 提到 0.8)、`Tinted`
(着色, 2, regular-2, 内圈折射按比例从 0.35 提到 0.7)、`Opaque` (减少透明度, 3: 重模糊加接近不透明的着色, `Glass::opaque(dark)` 按深浅给色, 预设取浅色). 名字和线上值都在 `Level`
(`parse` / `from_wire` / `to_wire`, 照 `motion::Preset`), 认不得的线上值当通透. regular 取当前档, clear 再通透一档 (`Level::clearer`, 到通透为止,
不透明不变). 高光各档一律 `330deg 0.45 4px` (用户定在调参页定). `Glass::from_properties_at(level, …)` 按档位取预设再盖分项;
`from_properties` 是默认档 (通透).

## 材质表

材质表 (`acde_scene::material`): 窗口级材质按名字登记, 不是封闭的枚举. 名字是 `[a-z0-9._-]`、不超过 64 字节
(`Material::valid`, `MOST_NAME`); 内置 `glass` (液态玻璃, 取 `Glass::preset_in(role, 档位, 深浅)`)、`blur` (背景模糊加着色, 没有透镜、高光、轮廓和折射, `Glass::blur(档位, 深浅)`)、
`none` (不画) 第一次用表时登记. `Material::lookup(name)` 查表, `register(name, look)` 留给以后的自定义效果 (名字不合规或已占用回 None; 画法 `Look` 是
`#[non_exhaustive]`). `Material::resolve(name, warn)` 给合成器、宿主窗口和页面胶水用: 空名是 `none`, 查不到按 `none` 画, 每个名字只 warn 一次.
`glass(role, preference)` 按档位、深浅、减少透明度取参数 (结构复用 `Glass`; 减少透明度时 blur 着色不透明度为 1, glass 是 `Opaque` 档),
`with_overrides(role, preference, overrides)` 再经 `Glass::with_overrides` 盖分项 (线上格式, NaN 不覆盖), blur 和 glass 同一个入口.
blur 各档: 通透 6/12 px、着色 0.15, 适中 10/20/32 px、0.35, 着色 16/32/48 px、0.6, 不透明 24/48 px、1; 浅色着 (0.97, 0.97, 0.98), 深色着 (0.11, 0.11, 0.12), 和 `Glass::opaque` 同色.
窗口级材质经 surface 交给宿主 (`SurfaceCommand::Material(WindowMaterial)`, `src/render/surface.rs`; 页面 `acde.window.setMaterial` → 原生 `setMaterial(name, options)` →
`WindowMaterial::from_options`): `name` 是 `Material::resolve` 之后实际生效的名字 (`none` 即取消), `role`、`group` (已查格式)、`spacing` (缺省 10, 0–256)、`titlebar`
(只铺标题栏, 高度由宿主按窗框定), `overrides` 已由 `acde_scene::material::overrides` 从"参数名 → CSS 写法"换成线上格式 (NaN 不覆盖; 两词的 highlight 不盖宽度, lens 只盖给了的几项;
写错的项 warn 后不盖). 会话里 acdes 把 `glass` 交给 `add_glass_area_role`, 其余交给 `set_window_material(name, height, overrides)`; C 接口暂不转发 (`ffi.rs` 丢掉).

## 玻璃组

玻璃组 (融合; `acde_scene::glass::group`): `GlassGroup` 是最多 `MOST_PIECES` (16) 块
`Piece {center, half, radius, exponent, glass}` 加间距 `spacing`, 满了 `push` 回 false (调用方让多出的块不参加融合, 各画各的, 记一次 warn).
各块的距离场按 Inigo Quilez 的多项式平滑最小值合成 (https://iquilezles.org/articles/smin/ , `h = clamp(0.5 + 0.5 (d − dᵢ) / k)`,
`d = mix(d, dᵢ, h) − k h (1 − h)`, k 是间距): 只在两块的距离差小于 k 处起作用, 所以间隙不小于 k 时各块的轮廓原样不动, 小于 k 时开始鼓出,
小于 k/2 时连成一片. 各块的参数 (除模糊层以外的全部玻璃参数) 按同一权重 h 混合, 做法同 www 的 `public/landing/proto/h/liquid.js` (它又参照
Liquid Glass Studio, MIT, https://github.com/iyinchao/liquid-glass-studio ; 只借思路, 没有拷代码). 法线: 一块的组用单块玻璃原来的法线 (中心指向该点, 按半宽归一), 结果和单块玻璃逐位相同; 两块及以上的组, 在有块相互作用的地方
(`merging = max 4h(1 − h)` 大于 0, 即两块距离差小于间距 k) 用合成场的梯度 (中心差分, 步长 0.5 px; 权重 `min(1, 4·merging)`, h 约在 0.07–0.93 之间即全用梯度),
所以融合处的高光、边缘折射和透镜沿融合后的整体外轮廓; 离别块不小于 k 的地方 h 严格为 0 或 1, 场值、参数、法线都是那一块自己的, 结果和单块逐位相同
(2026-11-23 改: e0eb46f 在整个组都用数值梯度, 远处那块的高光和单块的解析法线差一个末位, 远离的块不再逐位不变). `fs_glass_group` 的透明度、平滑边、高光与透镜全部只读
`acde_glass_group_field` 的合成场 d 和组梯度, 不再读单块的距离或角形; 两块相交后, 重叠内部原边界处保持不透明且没有亮线, 新外边界才有高光 (CPU 回归测试).
一块的组回归测试继续对照 `cpu::paint`, 远离的另一块不改变这一块任何像素的回归测试也要过.
模糊层全组一份, 取组里最实的一块 (`GlassGroup::frost`, 2026-11-24: 先比显示的各层里最大的模糊半径, 再比各层半径乘不透明度之和, 再按整份线上格式逐项比, 所以和块的排序无关; 原来取第一块, 组里角色不同时观感随排序变). 不按块逐像素混合模糊: 模糊是取样之后、逐像素着色之前对底图的多遍处理, 每块一套模糊就要给着色器绑定多张模糊结果, WebGL2 级别没有纹理数组绑定, 合成器也同样受限. 着色、透镜等其余参数仍按块逐像素混合. WGSL (`group::WGSL`, 要先拼 `corner::WGSL` 和 `glass::WGSL`) 只引用一个模块级数组 `acde_glass_pieces`,
由使用方自己声明绑定: `var<uniform> acde_glass_pieces: array<AcdeGlassPiece, 16>`; 每块 9 个 vec4 (144 字节, `to_uniform` 给出整块 2304 字节):
`shape` (中心、半宽高)、`corner` (半径、指数、折射率)、`refraction`、`bleed` (渗出三项加高光角度)、`look` (高光强度、白点、平滑、高光宽度)、`tint`、`lens` (透镜厚度、色散、乘暗、斜面宽度). 用 uniform 定长数组
不用存储缓冲: WebGL2 级别的 wgpu 限额里存储缓冲为 0 (https://wgpu.rs/doc/wgpu/struct.Limits.html `downlevel_webgl2_defaults`), uniform 块至少
16384 字节 (https://registry.khronos.org/OpenGL-Refpages/es3.1/html/glGet.xhtml), 页面合成和合成器都能用. 块数和间距由使用方的参数传入
`acde_glass_group_field(p, count, spacing)`; `acde_glass_group_offset` / `acde_glass_group_shade` 按场做折射偏移和着色, 和单块的 `fs_glass` 是同一套函数
(`acde_glass_offset_along`、`acde_glass_shine`). CPU 版 `GlassGroup::paint_cpu`, 同样不做折射.

## 透镜

透镜 (边缘折射, `Glass.lens {strength, width, dispersion}`、`Glass.darken`): 观感照用户指定的 ShengzhiWu/liquid-glass (MIT,
https://github.com/ShengzhiWu/liquid-glass , 演示 https://shengzhiwu.github.io/liquid-glass ; 只借做法, 没有拷代码): 中间不模糊、不加白, 颜色只乘
`1 − darken` (它是 0.95); 边缘那圈"发光"不是高光, 是只在很窄一圈边缘的强折射把背后亮的颜色挤成一条细线 (它的放大按场值 `exp(−20·(sum − 0.7))`,
离阈值一点就几乎为 0); 红绿蓝分三次取样出彩边 (它是 485 / 500 / 515, 即绿色上下各 3%). 我们在距离场上做: e 是到边缘的距离, w 是宽度,
早退 : 透镜厚度为 0、折射率不大于 1、在斜面以外或玻璃外面时 (`acde_glass_bends` 为假) 不求梯度也不进斜面计算. 玻璃底图只有一层模糊时
(含模糊为 0, 只拷一份底图) 不再画混合那一遍, 直接用这一层; `backdrop-filter` 没有颜色函数时也不画颜色矩阵那一遍.

## 透镜查找表

透镜查找表 (`glass::lens`): 斜面剖面加右侧下包络每像素要算 64 段, 改成按 (厚度/斜面宽 `ratio`、折射率、色散) 预先算好的一维表.
一张表三行 (折射率 `n − Δ`、`n`、`n + Δ`, 对应红、绿、蓝), 每行 `TABLE_SIZE` = 2048 个 f32, 第 k 个采样点在 `x = (k/2047)³` (外缘陡,
采样点集中在那里), 值是归一化源深度 g(x)/w; 查表时 `at = x^(1/3)·2047` 线性插值. 和逐段算法的误差在 1/255 以内、仍严格递增 (单测按六组参数、三个通道
各取 4001 点比对). CPU 侧 `LensTable` 有 `TABLE_SLOTS` = 8 个槽, 按 `Key` (ratio、折射率、色散) 找槽, 命中就复用, 没命中算一张新表放进最久没用的槽
(本帧用过的槽不换; 全满且都在本帧用过时返回 None, 这一块退回每像素算法). 纹理格式和绑定约定 (页面合成已接, 合成器照此接):
- 纹理 `R32Float`, 宽 2048、高 `8 × 3` = 24, 第 `slot × 3 + 通道` 行 (通道 0 红、1 绿、2 蓝), 用途 `TEXTURE_BINDING | COPY_DST`, 不过滤,
  着色器 `textureLoad` 取相邻两点自己插值; 更新只在槽换了参数时整三行 `write_texture`.
- 着色器拼上 `lens::TABLE_WGSL` (在 `glass::WGSL` 和 `group::WGSL` 之后), 由使用方声明 `var acde_glass_lens_rows: texture_2d<f32>` (页面合成是绑定 4).
- 每次绘制给一个 `vec4 table = (槽号或 −1, ratio, 折射率, 色散)` (页面合成放在 `Params.table`); 着色器发现槽号小于 0 或这块的参数和表不一致
  (组里各块参数不同、按权重混合后) 时退回 `acde_glass_lens` 逐段算法, 结果相同, 只是慢. 单块用 `acde_glass_lens_rgb_tabled`, 组用
  `acde_glass_group_lens_tabled`.

## 斜面透镜

斜面透镜 (2026-11-23 用户定方向: 照苹果, 中间平、清楚、不动, 只在边缘一圈斜面弯折, 不翻转; 取代 b210a8a 的"整块放大"): 剖面照 kube.io
(https://kube.io/blog/liquid-glass-css-svg/ ), x = e/w ∈ [0, 1] (e 到边距离, w 斜面宽度, 不大于半短边), 高度 `y = ⁴√(1 − (1 − x)⁴)` (凸超椭圆),
x ≥ 1 平坦, 位移严格为 0. 斜率 `y′ = (1 − x)³ (1 − (1 − x)⁴)^(−3/4)` 用解析式 (原文中心差分). 入射光垂直于背景, 表面法线与竖直的夹角
`θ1 = atan(r·y′)` (r = 厚度/w), 斯涅尔定律 `sin θ1 = n·sin θ2`, 折射光与竖直的夹角 `θ1 − θ2`; 落点按"光从表面高度 h = 厚度·y 落到背景"算,
朝内偏移 `h·tan(θ1 − θ2)` (原文没给这一步的公式, 这里的推法等价于原文按剖面求出位移再沿边扫一圈). 源深度 `g(e) = e + 偏移`.
保序: 外缘附近曲面陡, 偏移会先升后降, 若 g 下降就会对折出倒像. 所以把 g 在 64 段上取"右侧下包络" `ĝ(x) = min_{u ≥ x} (g(u) − 0.1 (u − x))`
(节点之间线性插值), ĝ′ ≥ 0.1 处处成立、ĝ ≤ g、在 x = 1 处等于 1, 所以严格单调、不越过斜面、也不越过中心. 强度来自厚度 (越厚越弯),
折射率 1 或厚度 0 时严格恒等. 色散: 红、绿、蓝各用折射率 `n − Δ`、`n`、`n + Δ` 算一遍 (`acde_glass_lens_rgb`), 三条都单调.
`Lens::source_depth` 是 CPU 同算法; 单测对默认值、大厚度 (200 px)、窄斜面 (2、4 px)、宽斜面 (256 px, 夹到半短边)、折射率 1–3 与三种半短边各采样
2000 段, 断言严格递增且不越过中心, 断言平坦区 (e ≥ w) 严格不动, 默认值最大偏移大于 2 px, 厚度 0 和折射率 1 恒等.
n 取距离场的梯度 (中心差分 0.5 px; 单块是 `acde_glass_corner_gradient`, 组是合成场的梯度), 在直边上垂直于边、在圆角上沿径向. 不用"中心指向该点"
那个法线: 它在长条中段是斜的, 第一版 (496fbc2) 用它加上宽衰减, 把中间的字和色块拉成波浪、出重影. 位移朝里取样. 三通道各自的位移由 `glass_under` 分别取样. 它的元球是硬阈值, 我们照旧用平滑边缘抗锯齿. 写法: `--acde-glass-lens: <斜面宽度> <厚度> [<折射率>] [<色散>]`
(斜面宽度、厚度是长度, 随缩放; 折射率 0–3, 1 及以下即无透镜; 色散 Δ 是折射率差, 0–0.1; 后两项不写沿用预设. 2026-11-23 定的最终语义), `--acde-glass-darken: <数>`. 强度为 0 时三次取样落在同一点, 结果和没有透镜时逐位相同. 页面合成
(`fs_glass`, 参数放 `Params.matrix` 第四列: 厚度、色散、乘暗、斜面宽度; 折射率放 `Params.shift.x`; 组里每块的折射率放 `corner.z`) 与组的 WGSL (`acde_glass_lens`、`acde_glass_group_lens`、`acde_glass_tone_darkened`)
同一套函数; 组只在边缘一圈内求梯度.
CPU 版只做乘暗, 不做透镜.
页面怎么成组 : 容器写 `--acde-glass-merge: <长度>` (对应苹果 `GlassEffectContainer(spacing:)`; 不写就不融合, 属性本身没有内置默认; 推荐 / 系统默认写 `--acde-glass-merge: 10px`, 用户定), 元素写 `--acde-glass-union: <名字>`
(`glassEffectUnion`)、`--acde-glass-id: <名字>` (`glassEffectID`, 出现、消失的动画用); 都是作者自定义属性, 不认它们的浏览器照常显示 backdrop-filter.
blitz-paint (`render/glass.rs`) 给有系统玻璃的效果图层带出 `LayerShape.group: Option<Membership>` (`acde_scene::glass::Membership`):
自定义属性会继承, 所以容器是从父元素起往上、值和父元素相同的那一串祖先里最外面的一个 (它的节点号, 间距按 CSS px 乘 scale); 嵌套的两层容器写了同一个
值时算作外层一个. 同一个 `--acde-glass-union` 名字不看 DOM 层级、大小与形状, 因此调参页要让大胶囊、小胶囊和圆钮融合时统一写 `--acde-glass-union: tuner`. 单测把三种形状放在三个不同的父节点下, 最终仍是一个三块的 GlassGroup. 玻璃分层 (用户定): 每块页面玻璃属于一层, 层是离它最近的、自己也是玻璃 (有 `backdrop-filter` 且解析出系统玻璃) 的祖先元素的节点号,
没有这样的祖先就是顶层 0 (`Membership.layer`, blitz-paint `glass_layer` 推断); 不是玻璃的包装元素 (`li`、布局用的 `div`) 不产生新层. 只有同一层的玻璃才可能

## 融合

融合 (`Membership::joins` 先比层, 再看同容器或同 union 名): 工具条和它里面的按钮即使在同一容器、写同一 union 名也不合, 按钮之间照常合.
页面层也不和窗口层 (窗口材质) 融合, 不跨窗口; 原定的 `--acde-glass-group` 已取消. 送合成器时 (`acompositor_backdrop_v1` v4 `add_glass_area_grouped`, 页面层, 组名只在本表面内有效) 组名用 `render::Backdrop.group_name`:
`merge_glass` 给每个连通组 (同层, 且同容器或同 union 名) 一个在整页内稳定的名字 (`Membership::group_name`): 组里有成员写了 union 名时取第一个这样的成员, 名字是 `u<union 名的 FNV-1a 散列, 16 位十六进制>.<层号十六进制>`; 只靠容器成组时是 `c<容器节点号>.<层号>` (都是十六进制). 同一页的各钉住面板里同一个 union 名、同一层的组因此同名, 合成器只要按页面而不是按表面登记页面层的组, 两个面板里的玻璃就能融合 (现在的协议版本 4 仍按 (页面层, 表面, 组名) 登记, 跨面板要等 acdes 改). 名字只用 `[a-z0-9.]`, 最长 34 字节, 合协议的组名规则. 原来按 `g0`、`g1`… 每帧依次编号, 跨面板会撞名.
libacde 自己还没有发这条请求的路径 (会话里由 acdes 的窗口客户端发), 前端照这个名字发即可. 名字存成 FNV-1a 64 位散列 (`Name`), `none` 和空串当没写. 没有容器也没有 union 名字的玻璃不成组. src/render `Split` 切完图层后
(`merge_glass`): 同一容器或同一 union 名字 (`Membership::joins`, 可传递) 的玻璃图层, 按绘制顺序把各块 (`Backdrop::piece`: 外框、半径、指数、玻璃) 放进
第一块那层的 `Backdrop.group`, 外框和裁剪取各块的并集, 其余几块的图层去掉 backdrop (只剩内容). 间距取组内最大 (只靠 union 成组时为 0, 各块不鼓出).
超过 16 块时多出的块各画各的, 记一次 warn. 只有一块的成员也成一组 (一块的组和单块画出来相同, G4 有对照测试), 好让出现、消失的动画有处可放. 底图在第一块的位置取一次, 所以后面几块之前画的内容 (多半没有) 不会出现在玻璃里.
前端把 `render::Backdrop` 换成合成用的 `gpu::Backdrop` 只调 `Backdrop::compose()` (宿主窗口再 `.moved((0.0, dy))`), 以后加字段两边不用改;
自己造 `gpu::Backdrop` 的 (窗口毛玻璃) 写 `..Backdrop::default()` (默认指数 2、没有玻璃).

## 进程内合成画组

进程内合成画组 (`src/vello/compose.rs`): `Composer::backdrop` 见到非空的 `group` 走 `glass_group`: 取样区域是 `Backdrop.rect` (各块并集) 外扩
`GlassGroup::reach()`, 模糊层与颜色矩阵和单块同一段 (`glass_under`, 模糊层取 `GlassGroup::frost` 那块的玻璃), 再用 `fs_glass_group` 逐像素按组场着色. 全部管线共用一个
绑定布局: 绑定 3 是块数组 (`acde_glass_pieces`, 动态偏移的 uniform, 每组 `GlassGroup::to_uniform` 的 1792 字节一段), 不画组的绘制给偏移 0; 参数缓冲和块缓冲
是同一个 `Uniforms` (满了翻倍重建, 已录的绑定组仍指向旧缓冲, 旧缓冲先写好), 任一个重建都清掉绑定组缓存. 组的块数组不重复传 : 同一批里内容相同的组复用已有那一段的偏移; 提交时若这一批的内容和上次写进缓冲的完全相同 (前缀一致), 不再 `write_buffer`, 所以组静止时每帧不重传 2304 字节; 缓冲重建时这份记录一并清掉. 块数和间距放在 `Params.shift` (x 块数, y 间距),
`Params.bias` 是取样区域. 宿主窗口和会话里画页面都经这里, 所以两边跟着生效.

## 出现、消失

出现、消失 (`--acde-glass-id`; `acde_scene::glass::Appearance`, 渲染侧 `Engine::animate_glass`): 每张平面 (整页一份, 每个钉住的面板各一份, 面板撤掉时丢掉)
记着上一帧各组里带 `--acde-glass-id` 的块 (`Piece.id`). 新出现的名字从同组里离它最近的那块的中心长出来 (半宽高和半径从 0 按曲线到实际大小, 中心从那块的中心
移到自己的位置); 消失的名字留一个影子块, 加进离它最近的那块所在的组, 按曲线缩回那块的中心后丢掉. 曲线是 `motion::Preset::Standard` (临界阻尼弹簧,
约 0.51 秒), 和合成器窗口动画同一份. 第一帧已有的块、没写名字的块、同组里没有别的块可依附的新块直接出现, 不做动画. 动画进行时, 平面的损伤加上各组外框
外扩 `reach()`, 再并上上一帧的这块范围; 动画结束的那一帧也补画上一帧的范围, 否则缩到最后的影子会留在画布上 (测试 Agent 2026-11-22 报过: 删掉的胶囊一直在), 帧标 `animating`, 引擎每个 tick 都要重画 (`glass_moving` 计入 `is_dirty`), 停下后恢复. 档位取系统的"减少动态效果" (`motion::system()`, 来自门户 `org.freedesktop.appearance` 的 `reduced-motion`):
`Preset::Reduced` (`moves()` 为假) 时不做形状动画, 新块直接出现、消失的块直接去掉, 进行中的影子也立即丢掉.
(也给出窗口之间毛玻璃的取样外扩 `margin`): 偏移在约 0.5–1.3 之间连续调 (一层时从 0 起, 五层时最多到 2), 强度随 sigma 连续变,
近似同 sigma 的高斯 (`SPREAD` 是 DX12 上量出的每档方差, `tests/vello_compose.rs` 按相位平均量边缘的展宽对照); sigma 小于约
0.96 px 时一层不偏移, 再按 `mix` 和原图混合. sigma 为 0 不模糊 (drop-shadow、backdrop 只拷一份). shell 的 GLES 链不做 `mix`.
