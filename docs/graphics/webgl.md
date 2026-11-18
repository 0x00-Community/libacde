# WebGL (`src/webgl` `acde-webgl`, acdert `webgl.rs`, acde-dom `webgl/`)

WebGL 1 和 2 在每个 OS 上都有 (Windows 没验过), 是 wgpu 上的一台 GL 状态机, 用 `Gpu::shared()` 那台设备 (和画布、页面共用), 不用 ANGLE、不用系统 OpenGL.

- **分层**: acde-dom `webgl/index.ts` 是标准的 `WebGLRenderingContext` / `WebGL2RenderingContext` 和 WebGL 对象 (`WebGLBuffer` …), 只管参数换算、
  对象包装和扩展对象; 状态、校验和 GL 错误都在 acde-webgl 的 `Context` (Rust, 不依赖 V8, 能单测); acdert `webgl.rs` 把它绑成 `natives.webgl`
  (每个 canvas 一个 `Context`, 调用的第一个参数是上下文编号). `getError` 同步, 所以每个调用自己校验; 会失败的 wgpu 调用 (建纹理 / 渲染缓冲 /
  绘制缓冲 / 管线 / 绑定组、提交) 都在错误范围 (`Context::scoped`) 里, 失败时报 GL 错误 (`OUT_OF_MEMORY` / `INVALID_OPERATION`) 并记日志.
- **着色器**: GLSL ES 1.00 / 3.00 经 `src/webgl/glsl` 的 WebGL 方言 (`Frontend::parse_webgl`, 见 `docs/graphics/vendor.md` 末尾) 变成 naga IR, 直接交给 wgpu
  (`ShaderSource::Naga`). `compileShader` 只解析校验, `linkProgram` 按两个着色器重新解析一遍: attribute 位置 (`bindAttribLocation` / layout / 依次)、
  varying 按名字配位置、不活跃的 attribute 和对面没有的 varying 降成普通全局. 活跃与否 (`getActiveUniform` 等) 按 `live.rs` 从语句往回追的表达式算.
  每个阶段的散 uniform 是一个 uniform 块 (第 0 组, 顶点 0、片元 1, 动态偏移; std140, 但 matCx2 按 WGSL 的自然布局, bool 存成 uint);
  采样器拆成纹理和采样器两个全局 (第 1 组), uniform 块也在第 1 组. 拿常量下标的采样器数组可以, 循环下标不行 (编译错误).
- **坐标**: 顶点着色器出口把 y 取反、z 从 [-w, w] 换到 [0, w], 所以纹理和帧缓冲里第 0 行是 GL 的最下面一行: 视口、剪裁、`gl_FragCoord`、
  `readPixels`、拷贝都和 GL 一样, 正反面反过来 (GL 的 CCW 是 wgpu 的 `Cw`), 只有把绘制缓冲交给页面时翻一次 (`read.rs` `present`).
- **录制**: 绘制攒进渲染通道 (`pass.rs`), 目标、需要排序的上传 (缓冲 / 纹理在本次提交里已经被用过时走编码器拷贝, 否则 `queue.write_*`)、
  读回、换帧时结束. 散 uniform、常量 attribute、生成的索引放在每个通道一块数据缓冲里. 管线按 (程序、绑定、顶点格式、图元、混合 / 深度 /
  模板 / 剪裁状态、目标格式) 缓存, 绑定组按资源身份缓存. 清屏能用加载操作就用 (整幅、掩码全开), 否则画一个全屏三角形.
- **wgpu 没有的**: TRIANGLE_FAN / LINE_LOOP 和 8 位索引在 CPU 上展开 (ELEMENT_ARRAY 缓冲留一份 CPU 副本, 也用来算索引范围); 非归一化整数当
  float、3 个分量的 8 / 16 位、打包的 2_10_10_10、divisor > 1 的顶点数据由计算着色器 (`helpers.rs` `CONVERT`) 转成 32 位四分量; 不对齐的
  `bufferSubData` 用按字节拷贝的计算着色器; GL 缓冲在 wgpu 那边多留 256 字节尾巴 (Metal 按整个步长检查最后一个顶点). RGB / LUMINANCE / ALPHA
  和 565 / 4444 / 5551 存成 RGBA8, 上传时在 CPU 上展开. 写 `gl_PointSize` 的程序另链一份点变体: 非索引、非实例的 `POINTS` 每点画一个
  实例 (6 个顶点), 属性按实例步进 (`ALIASED_POINT_SIZE_RANGE` [1, 1024]). 多重采样的深度经 `blitFramebuffer` 由 frag_depth 解析 (模板不解析).
- **画布**: 默认帧缓冲是 RGBA8 (`antialias` 时 4 倍多重采样), `alpha: false` 时不写 alpha. 绘制缓冲在渲染那一步 (`frame`, 2D 画布刷新之前)
  交成 canvas 的 `GpuImage` (`Dom::canvas_image`), `preserveDrawingBuffer: false` 时下一次绘制先清; 2D `drawImage` 一个 WebGL canvas 前先交.
  `toDataURL` / `toBlob` / WebGPU 外部拷贝读交出去的图. 纹理来源: `ImageData`、`<img>` (位图按 CPU 字节、SVG 在 GPU 上画)、2D canvas、
  `<video>` 的帧、WebGL / WebGPU canvas、`ImageBitmap` (render `Dom::image_pixels`), GPU 上的来源经 blit 着色器翻转 / 预乘写进去.
- **查询和同步**: 遮挡查询每个绘制一个查询位, 提交后映射回读; fence 等 `on_submitted_work_done`. 两者都要在结束它们的任务之后才可用 (acde-dom 按任务计数).
- **还没有**: transform feedback (`beginTransformFeedback` 报 `INVALID_OPERATION`)、`PIXEL_PACK_BUFFER` / `PIXEL_UNPACK_BUFFER` 上传和读回、
  `EXT_disjoint_timer_query`、带数据上传到 depth24 / depth32f (只有 depth16 能写)、copyTexImage 到 LUMINANCE / ALPHA 不换算、模板的 blit 解析、
  索引或实例化的宽点 (一个像素)、uniform 里的 bool 数组和含 bool 的结构、std140 块里的 matCx2、着色器校验比 ANGLE 宽松 (隐式转换照
  GLSL 4.x), origin-clean (跨源图片能读出来, 和 2D canvas 一样还没有), 上下文丢失后不恢复 (`restoreContext` 什么也不做).
- **检查**: `cargo test --release -p acdert webgl` (加 `--test 'webgl_*'` 只跑根 `tests/` 里的) (着色器翻译、渲染、uniform、查询、多重采样、宽点); `ACDE_WEBGL_SHADERS=<目录>` 时再把目录里
  `name_NNN.vert` / `name_NNN+1.frag` 一对对链接并生成 MSL / SPIR-V / HLSL (three.js 0.187 的 36 个程序是从 Chrome 里拦 `shaderSource` 得到的).
  2026-11-12 three.js (标准 / 物理 / Phong / Lambert / Toon / Matcap / 法线 / 深度材质、三种阴影、实例化、变形、蒙皮、点、线、精灵、
  EffectComposer + UnrealBloomPass + 4 倍多重采样的半浮点目标、PNG / SVG / 2D canvas 纹理) 和 pixi 8.23 在 macOS (Metal) 和 .234 (Vulkan)
  上逐个物体和 Chrome 对比一致.
