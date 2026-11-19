# C 接口 (`src/ffi.rs`, `include/acde.h`)

`src/ffi.rs` 是 C 接口, `build.rs` 用 cbindgen (`cbindgen.toml`, 只解析 `src/ffi.rs`) 每次构建重写 `include/acde.h` (进 git, 随 so 发):

- 入口: `acde_start(script, argc, argv, platform)` (script 为 NULL 即 Broker; argv 是 script 之后的参数) / `acde_common(argc, argv, platform, &code)`
  (argv 是整条命令行, 返回是否处理了) / `acde_version(program)` (用 `acde_string_free` 释放) / `acde_api_level()` / `acde_exit(code)`.
  `platform` 为 NULL 即 `Headless`.
- `AcdePlatform`: `ctx` + 回调表 (`claim` / `content_type` / `choose_folder` / `open_files` / `save_file` / `release`); 表按值拷进去, `ctx` 归 libacde,
  最后一个引用没了时调 `release(ctx)`; 回调可能在任意线程上来. 没给的回调走 Rust 的缺省 (claim 拒绝, 文件选择器走 portal, 内容类型无).
  选择器返回 `AcdeChoice` (CHOSEN / CANCELLED / FAILED), 路径或错误用 `acde_strings_push` 写进给的 `AcdeStrings`.
- `claim` 收到的 `AcdeRender*` 归 C (`acde_render_free`; `tick` / `pointer` / `scroll_at` / `commit` / `invalidate`), 成功时填 `AcdeWindow`
  (同样 `ctx` + 回调表 + `release`, 没给的回调取 trait 缺省), 失败时把原因 push 进 strings 并返回 false.
- 事件: `pump` 里 C 用 `acde_event_init()` 得到「全部缺省」的 `AcdeEvent` (整数 -1、浮点 NaN、指针 NULL 即没有), 填好后 `acde_events_push`.
  `set_waker` 给的 `AcdeWaker*` 归 C (`acde_waker_wake` / `acde_waker_free`, NULL 即清掉).
- 命令: `send` 收到 `AcdeCommand` (`kind` 决定哪些字段有意义; 指针只在回调期间有效). `READ_CLIPBOARD` 带 `AcdeReply*`, 必须恰好一次
  `acde_reply_send` 或 `acde_reply_free`. 还没进 C 的: `FRAME_THEME` 只有 kind, `START_DRAG` 不带拖拽图像, 帧本身 (`RenderHandle::frame`
  是 GPU scene) 拿不到, `Platform::exports` 收 V8 scope 只在 Rust 里. 指针锁定不进 C: `AcdeWindow` 没有 `pointer_lock`, 取 trait 缺省 (false),
  页面因此直接以 `NotSupportedError` 拒绝, 不会发出 `PointerLock` (`Outgoing::of` 遇到也丢掉).
- 导出表: `build.rs` 从 `src/ffi.rs` 收 `acde_*`, so 只导出它们 (Linux `--exclude-libs,ALL` + 版本脚本, macOS `-exported_symbols_list`, MSVC `.def`);
  V8 / napi / temporal 等依赖的符号都不导出 (napi 模块的 napi_* 只给静态链接的内置模块用, 不 dlopen 外部 .node).
