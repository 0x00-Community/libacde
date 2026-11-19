# libacde

libacde 是 Dopame 运行时库 (原 AC/acdert, 2026-11-17 并进 acdev 的 `acdert/`, 2026-11-18 连历史拆成本仓库): 内嵌 V8、自研 DOM 与排版绘制
(Blitz fork + acde-scene / vello)、网页存储、媒体、WebGPU / WebGL 绑定和 `acde:` 系统模块, 拥有全部跨仓契约 (包清单与清单规则、SDK `@dopame/acde-api`、
`acde_*` 协议). 对外是 Rust 包 `libacde` (库名 `acde`, 代码里 `use acde::…`; rlib + `libacde.so` + `include/acde.h`), 两个前端都和本仓库并排检出、以 path 依赖 `../libacde` 链它:

- [acdev](http://gitea.dopamine/AC/acdev): 跨平台调试运行时 (宿主窗口)、多平台模拟器和开发者命令行 `acdev`.
- [acdes](http://gitea.dopamine/AC/acdes): Dopame OS 的系统态运行时 (Wayland 合成器、DRM/KMS、会话、RDP), 只在 Linux.

两边都用的放这里, 只有一边用的放那一边. 闭源. 说明都在本文件和 `docs/` 里: 没有 README, 子目录不放 AGENTS.md. 按下面的文件路由选读.

> **第一原则: 不许在任何层次上魔改 web 接口.**
>
> - web 接口 (DOM、`window`、`fetch`、`localStorage`、事件、Web Audio、WebGPU …) 和浏览器**完全兼容**: 行为、签名、事件、错误都照标准.
>   不给标准对象加私有方法 / 字段 / 事件, 不改标准语义. 做不到的就是缺口, 记成「还没有」, 不拿长得像的私有版本顶替.
> - **全局只有两个**: `window` (web 兼容) 和 `globalThis` (node 兼容). 没有 `acde` 全局, 不往全局上挂非标准的东西.
> - 系统能力只有一个入口: `acde:` 内置模块, 照 `node:fs` 的写法导入 (`import { fs } from 'acde:fs'`、`import self from 'acde:window'` …;
>   类型在 SDK `@dopame/acde-api`). 清单的 capabilities 决定能导入哪些, 没开的导入直接失败. 「魔改」只许发生在 `acde:` 这一层.
> - **`acde:` 模块是进程内注入, 不是 RPC**: 像 workerd 的 binding, 直接注入页面所在的 V8 isolate, 调用直接进 Rust 原生实现, 不经 socket /
>   IPC / 别的进程代办.
> - 兼容的是标准接口, 不是 Chromium: 渲染器是自研的 (Blitz 的 fork, `vendor/blitz` 子模块, 没有 JS 引擎), 页面 JS 在本库自己的 V8 里. 没有
>   Chromium 的浏览器外壳; 正式运行没有远程调试端口, 只在开发模式 (`ACDE_DEV`) 下在本机 `127.0.0.1` 开 CDP 端点 (见 `docs/dev/devtools.md`).
> - 现有的债 (不是先例, 新代码不许再加, 动到时往这个方向收): web 里除 fs / terminal / network / office / picker 以外的 `acde:` 模块
>   还是经 `rpc.ts` / `remote.ts` 调到 Broker (见 `docs/runtime/webs.md`「通信」); acde-dopame 还直接用 napi 导出.

`web/` (acde-api、acde-dom、acde-host) 已于 91e7dd1e 删除 (CTO 决定). SDK 的替代品是 `acde-api/` 的 `@dopame/acde-api`, 只有类型声明
(见 `docs/acde-api.md`). 契约级别只有一个来源: `acde-api/api-level` (只写一个数字的纯数据文件, 用户 2026-11-17 定), `src/host` 编译时
`include_str!` 读它, 清单规则 `manifest/level.ts` 也读它; 清单规则在 `manifest/`, 运行时的页面一侧和 acdev 的命令行共用. 网页接口层 (原 acde-dom)
和页面胶水 (原 acde-host 的页面一侧) 在 `js/`, 作运行时内部组件嵌进二进制 (见 `docs/runtime/layout.md`「js」); 管理中枢 (broker.js) 没有取回.

## 代码约定

- 闭源 (用户定 2026-11-08): 自己的代码是专有的 (`LICENSE`), package.json 写 `"license": "UNLICENSED"`, Cargo.toml 不写 `license`.
  `libacde` 只发到内网 cargo 源 ac (`publish = ["ac"]`, 见 `docs/dev/release.md`「cargo 源 ac」); 仓库和 release 不公开.
  `vendor/` 和 `src/napi` 保留上游的许可证文件和版权行.
- Rust 和 TypeScript 代码不写注释 (`//`、`/* */`、`///`、JSDoc 都不写), 要说明的写进 `docs/` 里对应的文件. 例外: 版权和许可证行 (`src/napi` 的
  Deno 版权)、工具指令 (`/// <reference>`, `@ts-expect-error`)、`vendor/` 改动处的 `acrender:` (blitz fork 里) / `acde:` 标记. derive 要读的文字写成属性.
- 改了包清单、`acde:` 模块或别的对外可见的行为, 同时告知 acdev (它的命令行和 skill `npm/skills/acdev/` 要跟着改).
- 系统调用走 rustix, 不直接依赖 libc; rustix 没有的用专门的 crate (signal-hook、jiff). 按平台的代码用 `#[cfg]` / `cfg_select!`, 先用 std.
- 依赖版本 1.0 及以上只写主版本号 (`wgpu = "30"`), 0.x 写到次版本 (`log = "0.4"`), 用最新的; 例外是换成 `vendor/` 的 crate 和配套的 (blitz-*、stylo*、
  parley、html5ever、cssparser) 用 `=` 钉住.
- `Cargo.lock` 进 git (用户定 2026-11-18, 同 acdev): 依赖只在主动更新时变. 更新时 `cargo update` 单独一个提交, 测过再推; 改了依赖的提交
  同时更新锁文件. 前端 (acdev、acdes) 以 path 链本仓库时用的是它们自己的锁文件: 本仓库改了依赖, 它们要 `cargo update -p libacde` 跟上.
  `mach-dxcompiler-rs` (wgpu-hal 在 Windows 的静态 DXC) 锁在 0.1.4: 0.1.6、0.1.7 的 build.rs 编译时匿名查 GitHub API, 被限流就失败
  (acdev `docs/runtime-build.md`「DXC 钉版本」); 更新依赖后若它变了, 用 `cargo update -p mach-dxcompiler-rs --precise 0.1.4+2024.11.22-df583a3.1` 退回.
  0.x 写到次版本正是因为这个: 只写 `"0"` 时, workspace 的版本号一变, cargo 会重新解析这些 crate 的依赖, 它们会落到旧锁里已有的老版本上, 只在 Linux
  编的 `voice`、`office` 就编不过 (`rand::rng` 找不到, `Hmac<Sha256>` 的 trait 对不上); macOS 和 Windows 不编这两个模块, 看不到这个错.
- `js/bun.lock`、`scripts/wpt/bun.lock` 进 git, 改依赖时放在同一个提交里.
- 诊断用 `log` 的宏, 不用 `eprintln!` (测试、命令行输出除外).
- 格式化: Rust 只格式化自己改的文件: `rustfmt --edition 2024 --config-path rustfmt.toml <文件...>`, 或加 `--check` 只看不改 (根目录
  `rustfmt.toml`: 宽 160, `use_small_heuristics = "Max"`). 不要不带参数跑 `cargo fmt` / `cargo fmt --all`: blitz 是路径依赖,
  `vendor/blitz` 会被一起改写 (2026-11-18 总仓检出里出过一次, 用户定).
  Guard 的行宽同为 160 (`git config --local akira.guardMaxLineLength 160`, 全仓).
- 构建不从外网取东西 (2026-11-14): 依赖都从内网 Gitea 取 (blitz 子模块、rusty_v8 预编译库、CI 的 action), 见 `docs/dev/build-run.md`「构建不从外网取东西」.
  Cargo.toml / package.json 里的仓库地址写 gitea (`http://gitea.dopamine/AC/libacde`).

## 提交与推送

- CI 与发版不在云端试错 (用户定 2026-11-18): 改 CI 工作流或准备发版前, 先读 `~/.agents/references/github-release.md`; 先在本机或执行机上
  复现 (本机用 `cargo xwin` 等把各平台构建与打包跑一遍, 执行机环境先登录核对或问用户), 全部通过后才动云端; 云端 CI 只用于本地通过后的一次
  不发布预演和正式打标签发版, 失败就回本地修, 不连续重跑. CI 的触发 (手动运行、推发版标签) 只由测试 Agent 或主会话做, 开发 Agent 只改代码.
- 仓库在内网 Gitea (`origin` = `ssh://git@gitea.dopamine:2333/AC/libacde.git`, 网页 `http://gitea.dopamine/AC/libacde`), 只推这里.
- 远端只有 `main`, 不推别的分支. 本地需要时在总仓 `.worktrees/libacde-<后缀>` 开工作树和本地分支 `Akira/<后缀>` (基于 `origin/main`), 做完普通合并回
  `main` 再推 `origin main`. 不开工作树时几个会话共用工作区和 git index: 只改自己负责的路径, 别人没提交的文件先等.
- 提交只经总仓的 Guard: `uv run <总仓>/.agents/skills/akira/scripts/guard.py commit -m '<信息>' <自己的路径...>` (它做暂存语法检查、行宽和提交格式,
  再按给的路径提交). 推前 `git show --stat HEAD` 和 `git log origin/main..HEAD` 看一眼. `git add` / `git rm` 之后立刻提交. 做完就提交并推送.
- 改了 acdev / acdes 跟着要改的接口 (Rust 公开项、C 接口、`protocol/`、清单规则、`acde-api/`), 先推本仓库, 再推它们的配套提交; 两个前端都按
  `../libacde` 的 main 编, 没有钉住的版本.
- 不 force push, 不 amend / rebase 已推送的提交.
- 提交信息英文, Conventional Commits, 每行不超过 72 字符, 正文写原因和验证情况 (性能数字注明负载), 不加 Co-Authored-By.
- 不在总仓的 libacde 检出 (`<总仓>/libacde`) 里改动或格式化任何文件, 改就开工作树 (用户定 2026-11-18): 那个检出是总仓记录的版本, 也给测试联调用.
- 提交前: 按上面的写法格式化自己改的文件; 改了 `js/` 时在 `js/` 跑 `bun run typecheck`; 总仓的质量检查 `bun scripts/quality/check.ts <本仓库> origin/main`
  (在总仓跑, 结果须为 0 条); 提交时 Guard 的暂存检查.
- 构建、运行和测试按工程需要直接做 (见 `docs/dev/testing.md`), 做了就在提交信息里写清已验证 / 未验证. 汇报时也分开写「已验证 / 未验证」.

## 文件路由

| 什么时候读 | 文件 |
| --- | --- |
| 改说明或新增说明文档 | `docs/writing-docs.md` |
| 第一次接手运行时, 要弄清各仓的分工、两种跑法 (会话 / 宿主窗口) 或进程与线程 | `docs/runtime/architecture.md` |
| 找运行时的某个模块或目录归谁, 新加文件前确认放哪 | `docs/runtime/layout.md` |
| 改 C 接口 (`src/ffi.rs`、`include/acde.h`)、`Platform` 回调或 so 的导出表 | `docs/runtime/ffi.md` |
| 改窗口表、窗口模式与状态、焦点 / 模态 / grab、全局快捷键, 或 `protocol/` 里的 `acde_window` / `acde_shell` | `docs/runtime/window-model.md` |
| 改包清单的 `acde` 字段、桌面服务、契约级别 `apiLevel`、选哪个桌面、交出会话或登录页 | `docs/runtime/packages.md` |
| 改 SDK `@dopame/acde-api` (`acde-api/`, 只有类型声明) 或清单规则 (`manifest/`), 或在别的工程里引用它们 | `docs/acde-api.md` |
| 改 web 的出帧、窗框、钉住的面板、`acde:` 模块的注入 (层级、能力、批准)、沙箱或 web 的生命周期 | `docs/runtime/webs.md` |
| 改应用入口和脚本加载, 或某个浏览器接口 (表单控件、XML、通知、声音、视频、设备、摄像头和麦克风) | `docs/web/web-apps.md` |
| 改网页存储 (localStorage、IndexedDB、cookie、Cache API、`navigator.storage`) | `docs/web/storage.md` |
| 改应用后端的起停、代理变量、Windows 后端契约, 或 fetch / XHR / WebSocket 的路由 | `docs/web/backend.md` |
| 改 `acde:fs`、应用的根、拖放和打开请求的句柄、文件夹和文件选择器 | `docs/web/fs.md` |
| 改默认应用、文件类型判断、打开方式或 Windows 的文件关联 | `docs/web/default-apps.md` |
| 改 `acde:inspect` 的语义树、ref 和操作, 或原生窗口的 AT-SPI | `docs/web/inspect.md` |
| 改 `src/render` 的接口、canvas 2D、窗框绘制、模糊、GPU 设备或性能预算 | `docs/graphics/render-gpu.md` |
| 改 WebGL (`src/webgl`、`js/dom/src/webgl/`) 或 GLSL 前端 | `docs/graphics/webgl.md` |
| 改运行时自带的 JS (`js/`: 网页接口层、页面胶水、单应用启动器) 或它的打包 | `docs/runtime/layout.md`「js」 |
| 改 `vendor/blitz`, 或升 stylo / parley / html5ever / naga 这类钉住版本的依赖 | `docs/graphics/vendor.md` |
| 构建运行时、在 Windows 上编、升 v8 或改依赖镜像、设环境变量、跑运行时的命令和 doctor、看日志和 `perf::` 计时 | `docs/dev/build-run.md` |
| 改 dev 的 IPC、调试请求 (eval / shot / route / clock / input …)、dev 日志或热重载 (运行时一侧) | `docs/dev/dev-ipc.md` |
| 改 DevTools 端点、页面一侧的 CDP 域 (DOM / CSS / Network / Overlay)、断点或重载不断开 | `docs/dev/devtools.md` |
| 写运行时的回归测试、跑或更新 WPT 期望 (`scripts/wpt/`; CI 在 acdev 的 `.github/workflows/test.yml`) | `docs/dev/testing.md` |
| 把 `libacde` 发到 cargo 源 ac, 或了解运行时怎么随 acdev 发版 | `docs/dev/release.md` |
