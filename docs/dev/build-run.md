# 构建、运行、平台

```sh
cargo build --release -p libacde                # target/release/acdev (acdev 仓库)
cd web && bun install && bun run build         # packages/acde-host/dist/{broker,web}.js
../target/release/acdev desktops              # 装了哪些桌面
ACDE_PATH=<acde-dopame 的包目录> ../target/release/acdev --desktop dopame
```

页面包和单应用启动器构建时打进二进制 (见 `docs/runtime/layout.md`「js」), 运行时不再找 `broker.js`、`web.js`, `ACDE_APP` 不再有用.
验证平台: macOS 和 Windows 11 LTSC x64 (宿主窗口); Linux 的会话在有 GPU 的真机上, Linux 的宿主窗口只写不验.

**Windows**: 只出一个 x64 binary, MSVC v143 编. 要 VS 2022 17.11 以上 (MSVC 14.41+) 的 C++ 工具和 ATL 组件
(`Microsoft.VisualStudio.Component.VC.ATL`): 静态 DXC 的预编译库要新版 STL 的 `__std_*` 和 `atls.lib`, v142 链接不了, 不支持.
CRT 静态链接 (`.cargo/config.toml` 的 `+crt-static`). arm64 暂不做 (没有静态 DXC 的预编译库). `WGPU_VALIDATION=1` / `WGPU_DEBUG=1` 打开
D3D12 调试层 (要装「图形工具」可选功能). 没有: 通知、WebGPU、network / office / 语音. 依赖里不要 winapi
(fslock、async-dnssd 现在是 crates.io 原版, 经 v8 / sspi 会带进 winapi).

编好运行时和网页宿主后, 在有 GPU 的机器上用 `bash scripts/check-standalone.sh` 验证独立应用启动、重载、退出和服务清理.
脚本以无窗口模式运行, 测试包和日志写到 `logs/standalone.*`.

Windows 上用 `bash scripts/check-backends.sh` 验收应用后端 (见 `docs/web/backend.md`「Windows 后端契约」). 测试包 (`scripts/backends/`) 有 Node、Bun 两个带后端的应用和一个旧预加载的;
预加载从并排检出的 acdev (`../acde/src/listen.ts`, 或设 `ACDE_SRC` / 直接给成品 `ACDE_LISTEN`) 现场打包. 每个后端: 页面跑 GET、带体 POST、分块、8 个并发、
2 MiB 双向、WebSocket 文本和二进制 (Bun 另有 1 MiB 消息和发布订阅); 运行时、后端和后端的子进程没有 TCP 监听、UDP 端点、名字带它们 pid 的新命名管道;
Bun 的本代目录在默认 base 下, DACL 读回只有 SYSTEM 和当前用户, 启动时扫掉了预先放的残留目录; 杀后端后重起、子进程一起没了、旧目录删了、场景再跑一遍
(WebSocket 重连); 经 Broker 停止 3 秒内 (SHUTDOWN) 结束、子进程和目录没了; 关窗后运行时退 0, 进程和本代目录都不在. 旧预加载的 node 不起, 打 warn.
不设 `XDG_RUNTIME_DIR` (环境里有就沿用). 结果写到 `logs/backends.*/report.json`. 第二个本地账户打不开套接字目录这一项要手工验证, 脚本不做.

有图形会话的 Windows 上用 `bash scripts/audit-thread-progress.sh` 复查双窗口的 JavaScript 与出帧进展. 脚本会调整并关闭自己的宿主窗口,
在 `logs/thread-audit.*` 保存运行时日志、线程采样、截图和各步骤用时.

## 命令

命令行由前端的 `main.rs` 解析 (调试运行时在 acdev runtime 的 `src/main.rs`, 会话在 acdebin-system), 两边共用的部分交给 `acde_common` / `acde::common` (见 `docs/runtime/ffi.md`).
下面的 `<运行时>` 指前端程序: 宿主窗口是 acdev 的运行时程序, 会话是 acdebin-system (Arch 包里仍叫 `/usr/bin/acdert`, 改不改由 acdebin-system 定).

```sh
<运行时>                                    # 会话: 桌面按 `docs/runtime/packages.md`「桌面包」的规则选
<运行时> --desktop dopame                   # 那个桌面 (id 或包目录)
<运行时> --greeter                          # 登录页 (greetd)
<运行时> desktop <id|目录>                  # 调试桌面 (acdev dev)
<运行时> app <id|目录>                      # 调试窗口: 只跑一个应用
<运行时> desktops                           # 装了的桌面和外部会话 (不要显示器)
<运行时> --version                          # 发行版本和契约级别: <程序名> <版本> (contract level 1)
<运行时> --help                             # 用法 (main.rs 直接回, 不起 V8); 不认的选项: 一行原因加一行用法到 stderr, 退出 2
<运行时> doctor [--json]                    # 自检: 宿主文件、GPU (真画一帧)、Linux 的显示; 有错误退出 1 (见下)
<运行时> sessions DIR --package PKG         # 给包的每个桌面写 acdert-<id>.desktop
<运行时> applications DIR --package PKG     # `docs/web/default-apps.md`「默认应用」的 desktop entry 和 acde-mimeapps.list
<运行时> open [--app ID] PATH|URI...        # 在运行中的会话里打开
<运行时> run FILE [ARGS...]                 # 在一个 isolate 里跑一个模块 (runtime 的 tests/ 用)
<运行时> --size 1920x1080 --scale 1         # shell 报输出之前的 CSS 尺寸
<运行时> --launch foot --seconds 10 --stats # 起一个程序; 10 秒后停; 打印 stats 和窗口表
```

前端的 `doctor` 子命令见 acdev runtime `docs/doctor.md`.

环境变量: `ACDE_PATH`、`ACDE_DESKTOP`、`ACDE_STORAGE` (网页存储的根目录, 默认 `$XDG_DATA_HOME/acde/storage`, 空: 只在
内存, 见 `docs/web/storage.md`「网页存储」)、`ACDE_AUTOSTART=1/0`、`ACDE_DEBUG_DIR`、`ACDEV_DEBUG` (启动失败时 stderr 打整个调用栈; 不设、空或 `0` 时只打一行 `acdev: <message>`, 不带 `Error:`, 和 acdev 的 `Output.debug` 同一条规则)、`ACDE_BUS_NAMES=1/0`、`ACDE_COLOR_SCHEME=light/dark`、`ACDE_SIZE`、
`ACDE_DEV`、`ACDE_HOST_DUMP`、`ACDE_HOST_FAULT`、`ACDE_HUD=1` (宿主窗口开着帧率面板起来, 见 acdev `docs/hostwin.md`「帧率面板」)、`ACDE_GPU_ALLOW_CPU=1` (只为开发和测量: 没有硬件 GPU 时允许用 lavapipe 这类 CPU 设备, 见 `docs/graphics/render-gpu.md`)、`ACDE_HEADLESS=1`、`ACDE_SCALE` (headless 的缩放)、`ACDE_AUDIO=null`; shell 的见 `docs/graphics/render-gpu.md`「shell」.

- **配色** (launcher `js/launcher/src/app/appearance.ts`): `ACDE_COLOR_SCHEME` (light / dark), 否则 portal 的 `org.freedesktop.appearance` `color-scheme` (原生 `Appearance`, 只有 Linux; 读不到时浅色),
  进页面的初始化参数 `dark`, 变了发 `colorscheme` 给页面 (`matchMedia` 发 change). 开发通道的 `emulate` 设了值时它优先, 设回 null 时回到系统的值.
  **玻璃通透度** (2026-11-22, 总仓 `.scratch/libacde/glass-transparency-plan.md`): 同一个门户读取器还读 acdes 自家命名空间 `org.acde.appearance` 的 `glass` (档位号, 0 通透、1 适中、2 着色, 以后加档往后排) 和
  `reduced-transparency` (0 / 1), 读不到时按 0 和否; 进初始化参数 `glass: {level, reducedTransparency}`, 变了发 `{type: 'glass', level, reducedTransparency}`; 页面交 `dom.setGlass`,
  渲染器记进进程级的 `acde_scene::glass::preference()` 并整页重画, 解析 `--acde-glass` 时按它取预设. `Preference.dark` 是页面当前的深浅
  (`setDark` / 初始化参数 `dark`, 即门户的 `color-scheme` 或开发通道的模拟), 预设里按深浅分的值 (如轮廓颜色) 按它取.
  同一个 `reducedTransparency` 也是页面的 `@media (prefers-reduced-transparency: reduce)` / `no-preference` 和 `matchMedia` 的唯一来源 (2026-11-23):
  `engine.set_glass` 把它写进视口 (blitz `Viewport.reduced_transparency` → acss `Device.reduced_transparency`), 变了重算样式, `matchMedia` 立即看到,
  做法和 `setDark` 改 `color-scheme` 一样; 不另开读取路径.
  **减少动态效果**: 标准门户 `org.freedesktop.appearance` 的 `reduced-motion` 也由它读, 进初始化参数 `reducedMotion`, 变了发 `{type: 'motion', reduced}`; 页面交 `dom.setReducedMotion`,
  渲染器记进进程级的 `acde_scene::motion::system()` (`Reduced` / `Standard`), 玻璃出现、消失这类形状动画按它取曲线.
  **强调色** (2026-11-23): 标准门户 `org.freedesktop.appearance` 的 `accent-color` (`(ddd)`, sRGB 0..1) 也由它读 (原生 `Appearance.accent()`, 变化随 `watch` 来),
  三个分量都在 0..1 时转成 `#rrggbb`, 否则 (门户约定 -1 或越界表示没设) 是 `null`; 进初始化参数 `accent`, 变了发 `{type: 'accent', color}`; 页面交 `dom.setAccent`,
  `engine.set_accent` 写进视口 (blitz `Viewport.accent` → acss `Device.accent`), 系统颜色 `AccentColor` 取它、`AccentColorText` 按对比度取白或黑, 变了整页重算样式,
  `getComputedStyle` 立即读到; 没设或没有门户时两者是默认值 (`#0075ff`、白; 深色 `#99c8ff`、黑). CSS 里没有对应的媒体查询; 要分清没设和选了默认色、或要在变化时做事的页面用 `acde:window` 的 `accent()` / `on('accent')` (见 `docs/acde-api.md`).
  **壁纸亮暗** (2026-11-23): 同一个读取器还读 `org.acde.appearance` 的 `wall-tone` (`a{su}`, 屏幕名 → 0 未知、1 亮、2 暗) 和 `primary-output` (`s`, 主屏名),
  原生 `Appearance.walls()` 一次取两项, 变化随 `watch` 来 (`wallTones`、`primaryOutput`). 启动器的 `WallToneOf` (`appearance.ts`) 按自己窗口在 `shell.windows()` 那一行的 `output`
  取值, 这块屏没有值时用主屏, 都没有是 `null`; 门户变了、或每批窗口事件之后 (窗口可能换了屏) 重算, 值变了才发 `{type: 'walltone', tone}`, 页面交 `acde:window` 的
  `wallTone()` / `on('wallTone')` (见 `docs/acde-api.md`). 宿主会话没有门户, 一直是 `null`.
  **语言** (`language.ts`): 按 `LANGUAGE` / `LC_ALL` / `LC_MESSAGES` / `LANG`; 都没设时用系统的首选语言 (sys `languages()`, hostwin `system/languages.rs`:
  Windows `GetUserPreferredUILanguages`、macOS `NSLocale.preferredLanguages`, Linux 读系统区域 `/etc/locale.conf` 或 `/etc/default/locale`, #147);
  还是没有时 `en-US` (acde-dom 的默认同样是 `en-US`). `acde:session` 的 `setLanguage(tag)` 写 `locale.conf` 并发 `languagechange`.
  运行时自己的文字跟着页面语言 (acde-host `language.ts` `HostStrings`, 中文或英文, 别的语言用英文): `acde:fs` / `acde:office` 的根的 `label`
  (runtime 给英文, web 侧按 `source` / `place` 换: 应用数据、桌面、文档…; 选的文件夹用文件夹名不换), 文件夹 / 打开 / 保存选择器的标题.
- **远程桌面**: `ACDE_BACKEND=rdp` 离屏合成用 RDP 送出 (acdebin-system 的 shell `compositor/rdp.rs`, 见它的 `docs/rdp.md`). 要 `ACDE_RDP_USER` / `ACDE_RDP_PASSWORD` 和证书
  (`ACDE_RDP_CERT` / `ACDE_RDP_KEY`, 默认 `$XDG_CONFIG_HOME/agentos/rdp/{cert,key}.pem`); `ACDE_RDP_ADDR` (0.0.0.0:3390)、`ACDE_SIZE`
  (默认 1440x900)、`ACDE_RDP_SECURITY` (tls / nla). 还没有: 光标形状、Unicode 键和剪贴板、跟着客户端改尺寸、多个客户端.
- **调试运行中的会话** (`ACDE_DEBUG_DIR=<目录>`): 放进去的 `<name>.mjs` 由 Broker import, 默认导出拿 `DebugApi` (`debug.ts`: `broker`、`shell`、
  `web(name, code)`、`webs()`、`stats(reset?)`、`sleep`、`readText`、`readDir`), 结果写到 `<name>.out`. `ACDE_DEV` 设了时 web 的 debug eval 也开.
- **启动标记**: 每个里程碑一条 info (target `acde_host`) `startup: <标记> ms=<进程启动以来>`: Broker `shell created` / `services starting` /
  `services started` / `shell <里程碑>` (合成器的 `first commit` / `first vblank` / `first native window`) / `autostart`; 每个 web `page load started` / `desktop loaded` (或 `loaded`) / `shell running` / `first frame`.

## 日志和计时

Rust 一律 `log` 的宏, `src/log` (`acde-log`) 装 env_logger: `RUST_LOG` 追加在默认的 `warn,acdev=info,…` 后面; 每行
`<进程启动以来的毫秒> <级别> <线程名> <target>: <内容>`; 写 stderr 在 `acde-log` 线程上 (积压超过 8 MB 丢掉并记数), 退出和 panic 时
`acde_log::flush()`. smithay 的 tracing 进同一个 logger. JS 的 `console.*` 是 target `js`; acde-host 用 `log.ts` 的 `Log` (`Log.clock()` 和
`acde_log::now_ms()` 同一个零点).

页面错误 (`host.rs` `reportError`, 兜底 `rt.rs`) 每行以 isolate 名开头 (`[files#1048576] page error: …`); 不是 Error 的值写「类型 + 值」, 补上
`thrown at` / `rejected at:` 的位置.

计时是 debug 级、target `perf::<段>`, `RUST_LOG=perf=debug` 打开. acdebin-system 的 `packaging/qemu/perf-log.sh [日志]` (见它的 `docs/qemu.md`) 按种类汇总 (次数、ms 的 p50 / p95 / max).

| target | 哪里 | 一行是 |
| --- | --- | --- |
| `perf::latency` | acdebin-system 的 crates/window | 输入到上屏: `<kind> owner= ms=`, 分段 `queue` `js` `render` `raster` `present` |
| `perf::task` | src/host | 事件循环里超过 8 ms 的任务: `task ms= hook=`, V8 自己的 `v8` |
| `perf::frame` | render | `<web> commit= resolve= paint= finish= ms=`, 面板的 `panel <id> commands= damage ms=` |
| `perf::webgl` | src/webgl | 新建的渲染管线: `pipeline program= cached= ms= key=` (着色器缓存冷时 Metal 在这里同步编译, 一条几十毫秒) |
| `perf::audio` | src/host/audio | 每秒一行: 送到声音输出的混音 `out rms_db= peak= missed=` (`missed`: 页面占着混音器, 输出只好填静音的帧数) |
| `perf::raster` | acdebin-system 的 crates/paint、crates/window, acdev runtime 的 hostwin, src/vello | `job surfaces= wait= record= ms=`, `surface owner= ms=`, 闲时重建 vello 的 `vello trim ms=` |
| `perf::layers` | hostwin `present.rs` | 有 layers 的页面 / 面板: `window= region= layers=重画的/全部 ms=` |
| `perf::compose` | shell | `<输出> elements= ms=`, `rdp elements= rects= ms=`, `plan inputs= layers= damage= route=`, `backdrop window= w= h= blur= ms=` |
| `perf::rpc` | acde-host | web 侧 `call <服务>.<方法> error= ms=`, Broker 侧 `serve …` |

`perf::latency` 来自 `acde_log::Trace`: 进队列 → JS 取走 → commit → 出帧 → 光栅化完 → vblank.

JS 热点: `ACDE_PROFILE_DIR=<目录>` 让每个 isolate 开 V8 采样 profiler (`profile.rs`, 250 µs), 每 `ACDE_PROFILE_SECONDS` (默认 30) 秒和结束时写
`<名字>-<n>.cpuprofile`; acdebin-system 的 `bun packaging/qemu/profile-top.mjs [--top N] [--callers 函数名] 文件...` 汇总. 原生调用的时间多半记在调用它的 JS 函数上.

## 构建不从外网取东西

构建不从外网取东西 (2026-11-14): blitz 子模块在 Gitea `AC/blitz`; rusty_v8 的预编译库 (静态库 `.a.gz` / `.lib.gz` 和 `src_binding_*.rs`) 在公开组织
`mirror` 的 generic 包 `rusty_v8/v<版本>` (`.cargo/config.toml` 的 `[env] RUSTY_V8_MIRROR`, 不回落 GitHub). 升 v8 时把新版本的 5 个目标
(x86_64 / aarch64 Linux、两种 macOS、x86_64 Windows) 的这两种文件从 GitHub releases 下下来, `PUT` 到
`http://gitea.dopamine/api/packages/mirror/generic/rusty_v8/v<版本>/<文件>`. CI 的 action 用 `mirror` 组织里的副本 (`uses: http://gitea.dopamine/mirror/<名字>@<提交>`).
crates.io 和 npm 还是外网.
Cargo.toml / package.json 里的仓库地址写 gitea (`http://gitea.dopamine/AC/acdev`); npm 发布用令牌 (`NPM_TOKEN`), 不用 trusted publishing (用户定 2026-11-17).
