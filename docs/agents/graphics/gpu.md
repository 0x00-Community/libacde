# GPU 与性能 (硬约束)

整条链路不许 CPU 拷像素、不许 GPU 回读; 网页优先在 GPU 上画. 下面是硬约束、性能预算与 `src/render` 的已知问题.

- **整条链路不许 CPU 拷像素、不许 GPU 回读**. 例外: RDP 后端 (回读损坏区域)、截屏 / capture、标准接口要同步给像素的 (`getImageData` /
  `toDataURL`).
- **网页优先在 GPU 上画, 没有 GPU 时回退 CPU** (用户定): 没有 vello_cpu; `acde::gpu::adapters` 真 GPU 排前, CPU 类型的设备 (lavapipe)
  排最后, 只有它时用它 (虚拟机 / VNC, 慢). 这时页面的 WebGPU 适配器 `isFallbackAdapter` (`GPUAdapter` 和 `GPUAdapterInfo` 上都有) 为 true, 页面可以据此降级 (src/host/webgpu `adapter_info`: 设备类型是 CPU). 什么设备都没有时网页图层不可用 (日志 `web layer unavailable`), 页面不出帧.
  合成器自己 (GLES) 不在此列. acdes 的会话不走这条: 没有 GPU 时它在 CPU 上画页面, 不用 lavapipe (见它的 `docs/agents/window-paint.md`).
- **两种后端, 按 feature 选** (用户定): wgpu (src/vello) 和 detour (还没有, 以后在 `src/detour`). acdev 只开 wgpu: 三个平台一致, 给开发者用;
  acdes 开 detour 加 wgpu: Xe3 上走 detour, 别的 GPU 走 wgpu, 没有 GPU 时在 CPU 上画, WebGPU 总是 wgpu. wgpu 每个 OS 只编一个后端: Windows DX12、Linux Vulkan、
  macOS Metal (`acde::gpu::BACKEND`). Linux 另编进 wgpu 的 GL 后端 (`gles` 特性), 不进 `BACKEND`, 只经 `acde::vello::Device` 用
  (用户定, GL): 会话由 acdes 开始时探测一次
  (`ACDE_RENDER`: `detour:<节点>`、`wgpu:vulkan:<节点>`、`wgpu:gles:<节点>`、`cpu`, acdes 解析), 页面进程照它调
  `Device::open(Backend::Vulkan | Backend::Gles, 节点)`; 探测用 `Device::probe(backend, 节点)` (只开设备、不建 vello 管线, 成功时回 `describe()` 那一行).
  OpenGL ES 默认关: 开关 `ACDE_GL_ES=1` 由 acdes 的探测读, libacde 不读环境变量, 调了 `open(Backend::Gles, …)` 就开 (取证: 液态玻璃静止时还在出帧,
  开了之后静止时桌面进程 CPU 是 CPU 画页面时的十几倍; dopame 让它静止不出帧后再改默认).
  选适配器: 读 `/sys/class/drm/<节点名>/device/{vendor,device}`, 和 wgpu `AdapterInfo` 的 `vendor` / `device` 对上的用它; 对不上 (GL 后端常报 0) 时,
  非软件适配器恰好一个就用它; 有几个时 (WSL 的 D3D12 设备没有 PCI 编号, 渲染节点挂在 faux_driver 下) 先挑名字含 `MESA_D3D12_DEFAULT_ADAPTER_NAME`
  的 (Mesa 在 WSL 里按名字选卡的同一个变量), 再挑独显, 记一行 info. EGL 无表面平台由 Mesa 自己挑设备, 不能按节点指定, 所以 GLES 下这个比对只是核对.
  软件适配器 (wgpu `DeviceType::Cpu`, 即 lavapipe 或 llvmpipe) 在 `open_hardware` 和按节点选的 `pick` 里默认排除 (没有 GPU 时页面走 vello_cpu).
  只为开发和测量 (2026-11-23 实验): 环境变量 `ACDE_GPU_ALLOW_CPU=1` (`gpu::ALLOW_CPU`, `gpu::allow_cpu()`) 时
  不再排除, 排在所有硬件设备之后, 选中时记一行 warn (设备名、类型、驱动). 这不是正式路线, 默认不开, 哪条路线由用户定.
- 不合规的 Vulkan 驱动 (用户要 WSL 用真显卡): wgpu-hal 把一致性版本为 0 的驱动 (WSL 的 Dozen, 即 dzn) 藏起来, 除非实例带
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
  重叠, 挪窗口不重新光栅化, 闲 2 秒 (`Raster::idle`) 放掉中间纹理和 vello 的中间缓冲 (见下「vello 的中间缓冲」). 一帧里所有窗口、所有图层按损伤区切出的块 (`Part::pieces`) 排进一张图集
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
- **vello 的中间缓冲** (闲时放掉): vello 0.12 每次画都按写死的大小 (`vello_encoding` `BufferSizes::new`, 和场景、目标尺寸无关) 从 renderer 自己的缓冲池
  取 bump 分配的中间缓冲: lines、segments 各 48 MiB, ptcl 32 MiB, tiles、seg_counts 各 16 MiB, blend_spill 4 MiB, bin_data 1 MiB, 共 165 MiB; 画完放回池里,
  池只增不减, vello 没有放掉它的接口. 场景里有图片时 renderer 还留着图片图集 (从 1024² 起翻倍, 页面上有铺满窗口的 WebGPU canvas 时 4096², 64 MiB).
  统一内存的机器上这些都是进程的物理内存 (macOS `footprint` 的 `Owned physical footprint (unmapped) (graphics)`). 进程里每个画过的 renderer 各留一份:
  出帧的 (宿主窗口每个 web 一个 `Presenter`; 会话每个 web 进程一个 `Raster`, `Raster::new` 预热时就画一次) 和进程级 `Offscreen` (canvas 当图片源、
  `getImageData`、`putImageData`、`shadowBlur`、`ctx.filter`、`capture` 用到它). `Vello::trim` 重建 renderer (同一设备、同样的建管线线程数; 着色器走驱动缓存; 会话的 Vulkan 设备有 `PIPELINE_CACHE` 时管线还走 `vello::pipelines` 的 wgpu 管线缓存, 存在 `$XDG_CACHE_HOME/acde/wgpu/` 下, 两次预热之后大小变了才写回; 这个设备的队列在有 `VK_KHR/EXT_global_priority` 时要低优先级, 好让合成器先走,
  M4 上 5–16 ms) 再 `poll` 一次, 旧的缓冲池和图集随旧 renderer 放掉; 上次建好以来没画过就不重建 (回 `false`), 建不出来时留着旧的 (回错误).
  每个 `Vello` 建出来有个进程内唯一的编号, `Overrides` 记着给哪个编号登记过 GPU 图片 (`follow`), 换了就清掉重登 (不然新 renderer 把 GPU 图片当成
  没有字节的普通图片). 闲时调它的: 宿主窗口的出帧线程, 最后一次画出新内容 2 秒后 (acdev `docs/agents/hostwin.md`); 会话的 `Raster::idle` (acdes 光栅化线程
  闲 2 秒时调); 进程级 `Offscreen` 由 `acde-offscreen-trim` 线程在最后一次离屏绘制 2 秒后做 (`gpu::Offscreen::trim`, 拿着 `Offscreen` 锁, 这十几毫秒里
  画布绘制要等), 设备丢了时不做. 代价在闲下来之后的第一帧: 重新分配并清零这些缓冲 (`tests/vello_trim.rs` 打出时间, M4 1280×800 含 GPU 执行: 平时 1.4–1.7 ms, 重建后第一次
  8.2–8.5 ms); 一直在画的窗口不受影响.
  宿主窗口 (M4, 1323×808@2x, 空闲 30 s) 进程的 GPU 部分: 只有文字的页面 239.6 → 53.3 MiB, 用了离屏画布 (`putImageData` / `getImageData`) 409.0 → 53.6 MiB,
  铺满窗口的 WebGPU canvas 336.1 → 89.4 MiB; 剩下的是窗口大小的页面纹理、草稿和合成器画布.
  「闲」要按 vello 算: 页面只有 WebGPU canvas 在变时, 前端可以不经 vello 出帧 (`vello::Hole` 找出场景里那张不透明、一对一、整像素的 GPU 图片和
  它的矩形, `Hole::above` 给出盖在它上面的命令, `Kept` 判断这一帧除了图片是哪一张以外和上一帧是否相同; `Composer::place_from` 从任意纹理贴),
  宿主窗口的出帧线程这样做 (acdev `docs/agents/hostwin.md`「只有 WebGPU canvas 变了的帧」), 中间缓冲因此在 canvas 动着时也能放掉. WebGPU canvas 的
  `present` 按 `alphaMode` 标 `GpuImage::set_opaque`; `opaque` 模式下 alpha 置 1 的那一遍推迟到第一个要读 alpha 的人 (`GpuImage::settle`:
  `Overrides::apply`、`Hole::find`、`render::gpu::read`、`image_pixels`、canvas 读回), 只贴 rgb 的路径不付这一遍.
  会话的 `Raster` 也走这条路 (2026-11-24): 平的段里每块用 `Hole::largest` (可缩放、取最大的一张, 采样同 vello 的 Low / Medium) 找出洞,
  洞里 `Composer::opaque_from` 不混合地贴图片 (alpha 记 1, 不清区, 不进 vello、不拷进 vello 的图集), 洞外的边条照常画 (去掉这张图片);
  盖在上面的内容 (`Hole::above`, 只许在新开的层里用非 SrcOver 混合) 画一次存进按 (平面, 段, 洞) 的缓存纹理, `Kept` 认出没变时只按它的绘制范围
  (至多 32 块) 混合贴回. `Content::segments` 给出内容的命令段 (`Drawing::segments`, 前端的 `Content` 转给它) 时, 除了图片所在的那段以外各段都没变 (同一份 `Arc` 或 64 条以内逐条相等且没有 GPU 图片)、那段只换了图片, 就不再取整块命令 (`scene_for`) 和逐条比对. 一块覆盖了此前没画过的区域时不清区、不混合地贴 (`Op::Cover`); 一批里只有一块且铺满时 vello 不再套裁剪层.
- 帧预算: 120 fps 下每帧 8 ms. 盯 acde-dom `test/cost.test.ts` / `test/caches.test.ts`、shell / DomNative 的 `stats`、`drive.sh perf`; 提交里写测到的数.
- WebGPU (`navigator.gpu`, Linux 和 macOS; Windows 上还关着 (没编过也没跑过, 见 acdev #56)): src/host `webgpu/`, 跑在光栅化线程的设备上; 页面的 `mapAsync` / `onSubmittedWorkDone` 回调由 `acde-gpu-poll` 线程办: 设备能导出 sync file 时 (会话里 `VK_KHR_external_semaphore_fd`, src/gpu `Gpu::wait_fence`) 它在提交锁里挂一个 `vello::Signals` 信号量、空提交一次、取出 sync file, `poll` 等它可读后非阻塞 `poll` 一次设备, 不用定时器; 没有时 (宿主窗口、没开这个扩展的设备) 才每 2 ms 非阻塞轮询. canvas 的帧数在途不经它 (见 `docs/agents/graphics/webgl.md`) (每帧路径上不用
  `poll(Wait)`, 它拿着 snatch 读锁会卡住别的线程的提交). WebGL 也只经 wgpu, 见 `docs/agents/graphics/webgl.md`.

## src/render 的已知问题和测试数据

- canvas 的 `createPattern` 在第一次用时才取来源的位图 (规范是创建那一刻).
- vello 0.11 没有图片的场景把图集当 1x1 重建、已上传的图片画空; 每个场景垫一块 1x1 渐变绕开 (src/vello `Vello::render`), 升级 vello 时看
  上游修了没有.
- `ACDE_PAINT_VERIFY=1` 下机器忙时 `tests/render_damage.rs` 偶发等帧超时, 单独重跑都过.
- `tests/fixtures/replay/*.ops.gz` 是旧 acde-host 的 DOM 操作流 (`tests/render_replay.rs` 回放, 格式见它), `svg/*.svg` 是简单 SVG 的对比用例, 元素的特殊渲染
  在 `tests/render_elements.rs`. `release_keeps_memory_flat` 只在 Linux 上跑.
