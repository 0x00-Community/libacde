# C 接口 (`src/ffi.rs`, `include/acdert.h`)

`src/ffi.rs` 是 C 接口, `build.rs` 用 cbindgen (`cbindgen.toml`, 只解析 `src/ffi.rs`) 每次构建重写 `include/acdert.h` (进 git, 随 so 发):

- 入口: `acdert_start(script, argc, argv, platform)` (script 为 NULL 即 Broker; argv 是 script 之后的参数) / `acdert_common(argc, argv, platform, &code)`
  (argv 是整条命令行, 返回是否处理了) / `acdert_version(program)` (用 `acdert_string_free` 释放) / `acdert_api_level()` / `acdert_exit(code)`.
  `platform` 为 NULL 即 `Headless`.
- `AcdertPlatform`: `ctx` + 回调表 (`claim` / `content_type` / `choose_folder` / `open_files` / `save_file` / `release`); 表按值拷进去, `ctx` 归 acdert,
  最后一个引用没了时调 `release(ctx)`; 回调可能在任意线程上来. 没给的回调走 Rust 的缺省 (claim 拒绝, 文件选择器走 portal, 内容类型无).
  选择器返回 `AcdertChoice` (CHOSEN / CANCELLED / FAILED), 路径或错误用 `acdert_strings_push` 写进给的 `AcdertStrings`.
- `claim` 收到的 `AcdertRender*` 归 C (`acdert_render_free`; `tick` / `pointer` / `scroll_at` / `commit` / `invalidate`), 成功时填 `AcdertWindow`
  (同样 `ctx` + 回调表 + `release`, 没给的回调取 trait 缺省), 失败时把原因 push 进 strings 并返回 false.
- 事件: `pump` 里 C 用 `acdert_event_init()` 得到「全部缺省」的 `AcdertEvent` (整数 -1、浮点 NaN、指针 NULL 即没有), 填好后 `acdert_events_push`.
  `set_waker` 给的 `AcdertWaker*` 归 C (`acdert_waker_wake` / `acdert_waker_free`, NULL 即清掉).
- 命令: `send` 收到 `AcdertCommand` (`kind` 决定哪些字段有意义; 指针只在回调期间有效). `READ_CLIPBOARD` 带 `AcdertReply*`, 必须恰好一次
  `acdert_reply_send` 或 `acdert_reply_free`. 还没进 C 的: `FRAME_THEME` 只有 kind, `START_DRAG` 不带拖拽图像, 帧本身 (`RenderHandle::frame`
  是 GPU scene) 拿不到, `Platform::exports` 收 V8 scope 只在 Rust 里.
- 导出表: `build.rs` 从 `src/ffi.rs` 收 `acdert_*`, so 只导出它们 (Linux `--exclude-libs,ALL` + 版本脚本, macOS `-exported_symbols_list`, MSVC `.def`);
  V8 / napi / temporal 等依赖的符号都不导出 (napi 模块的 napi_* 只给静态链接的内置模块用, 不 dlopen 外部 .node).
