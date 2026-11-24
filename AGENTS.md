# libacde

libacde 是 Dopame 运行时库: 内嵌 V8、自研 DOM 与排版绘制
(Blitz fork + acde-scene / vello)、网页存储、媒体、WebGPU / WebGL 绑定和 `acde:` 系统模块, 拥有全部跨仓契约 (包清单与清单规则、SDK `@acde/api`、
`acde_*` 协议). 对外是 Rust 包 `libacde` (库名 `acde`, 代码里 `use acde::…`; rlib + `libacde.so` + `include/acde.h`), 两个前端都和本仓库并排检出、以 path 依赖 `../libacde` 链它:

- [acdev](http://gitea.dopamine/AC/acdev): 跨平台调试运行时 (宿主窗口)、多平台模拟器和开发者命令行 `acdev`.
- [acdes](http://gitea.dopamine/AC/acdes): Dopame OS 的系统态运行时 (Wayland 合成器、DRM/KMS、会话、RDP), 只在 Linux.

两边都用的放这里, 只有一边用的放那一边. 以 AGPL-3.0-only 开源 (`LICENSE`), 公开在 GitHub [0x00-Community/libacde](https://github.com/0x00-Community/libacde).
给人看的介绍是根目录的 `README.md` / `README.zh-CN.md` 和 `docs/humans/`; 给 Agent 的说明在本文件和 `docs/agents/`, 子目录不放 AGENTS.md.

> **第一原则: 不许在任何层次上魔改 web 接口.**
>
> - web 接口 (DOM、`window`、`fetch`、`localStorage`、事件、Web Audio、WebGPU …) 和浏览器**完全兼容**: 行为、签名、事件、错误都照标准.
>   不给标准对象加私有方法 / 字段 / 事件, 不改标准语义. 做不到的就是缺口, 记成「还没有」, 不拿长得像的私有版本顶替.
> - **全局只有两个**: `window` (web 兼容) 和 `globalThis` (node 兼容). 没有 `acde` 全局, 不往全局上挂非标准的东西.
> - 系统能力只有一个入口: `acde:` 内置模块, 照 `node:fs` 的写法导入 (`import { fs } from 'acde:fs'`、`import self from 'acde:window'` …;
>   类型在 SDK `@acde/api`). 清单的 capabilities 决定能导入哪些, 没开的导入直接失败. 「魔改」只许发生在 `acde:` 这一层.
> - **`acde:` 模块是进程内注入, 不是 RPC**: 像 workerd 的 binding, 直接注入页面所在的 V8 isolate, 调用直接进 Rust 原生实现, 不经 socket /
>   IPC / 别的进程代办.
> - 兼容的是标准接口, 不是 Chromium: 渲染器是自研的 (Blitz 的 fork, `vendor/blitz` 子模块, 没有 JS 引擎), 页面 JS 在本库自己的 V8 里. 没有
>   Chromium 的浏览器外壳; 正式运行没有远程调试端口, 只在开发模式 (`ACDE_DEV`) 下在本机 `127.0.0.1` 开 CDP 端点 (见 `docs/agents/dev/devtools.md`).
> - 现有的债 (不是先例, 新代码不许再加, 动到时往这个方向收): web 里除 fs / terminal / network / office / picker 以外的 `acde:` 模块
>   还是经 `rpc.ts` / `remote.ts` 调到 Broker (见 `docs/agents/runtime/webs.md`「通信」); 服务的 `ctx.spawn` 起的进程不受清单约束.

契约级别只有一个来源: `acde-api/api-level` (只写一个数字的纯数据文件; 最低支持级别在旁边的 `min-api-level`), `src/host` 编译时 `include_str!` 读它,
清单规则 `manifest/level.ts` 也读它. SDK `@acde/api` 在 `acde-api/`, 只有类型声明 (见 `docs/agents/acde-api.md`). 清单规则在 `manifest/`, 运行时的页面一侧和
acdev 的命令行共用. 网页接口层和页面胶水在 `js/`, 作运行时内部组件嵌进二进制 (见 `docs/agents/runtime/layout.md`「js」).

`acde:` 模块接口的版本化标准在 [AC/acde-RFC](http://gitea.dopamine/AC/acde-RFC) (并排检出 `../acde-RFC`): 每个版本从 `acde-api/` 冻结一份声明, 配接口验证器和
一致性测试. `acde-api/` 是正本, 在这里改; 运行时是否符合某个版本, 以 acde-RFC 的 `runner/` 在运行时里跑的结果为准.

## 代码约定

- 许可证: 自己的代码是 AGPL-3.0-only (`LICENSE`), package.json 写 `"license": "AGPL-3.0-only"`, Cargo.toml 经 `license.workspace = true` 继承.
  `libacde` 的 crate 发到 cargo 源 ac (`publish = ["ac"]`, 见 `docs/agents/dev/release.md`「cargo 源 ac」). `vendor/`、`src/napi` 和 `src/webgl/glsl` 保留上游的许可证文件和版权行.
- Rust 和 TypeScript 代码不写注释, 要说明的写进 `docs/agents/` 里对应的文件. 例外: 版权和许可证行 (`src/napi` 的 Deno 版权)、工具指令
  (`/// <reference>`, `@ts-expect-error`)、`vendor/` 改动处的 `acrender:` (blitz fork 里) / `acde:` 标记、SDK 声明文件 (`acde-api/` 的 `.d.ts`) 里过时接口上方单行的
  `/** @deprecated 改用 … */` (见 `docs/agents/acde-api.md`「版本与兼容」). derive 要读的文字写成属性.
- 改了包清单、`acde:` 模块或别的对外可见的行为, 同时告知 acdev (它的命令行和 skill `npm/skills/acdev/` 要跟着改).
- 系统调用走 rustix, 不直接依赖 libc; rustix 没有的用专门的 crate (signal-hook、jiff). 按平台的代码用 `#[cfg]` / `cfg_select!`, 先用 std.
- 依赖版本在 `Cargo.toml` 里精确锁定 (`wgpu = "=30.0.2"`), 和并排的 acdes 共用的包锁同一个版本; 升级就是改这些版本号 (本仓库与 acdes 一起改), 单独一个提交.
  blitz-* 锁 `=0.3.0-beta.5` (path 依赖). stylo (`style`、`style_dom`、`style_traits`)、parley、cssparser 不自己写版本, 经 blitz-dom 的重新导出用
  (`blitz_dom::style::…`), 版本跟着 blitz.
- `Cargo.lock` 不进 git: 直接依赖按上面锁定的版本, 间接依赖每次构建全新解析; 前端 (acdev、acdes) 用它们自己的锁.
- `js/bun.lock`、`scripts/wpt/bun.lock` 进 git, 改依赖时放在同一个提交里.
- 诊断用 `log` 的宏, 不用 `eprintln!` (测试、命令行输出除外).
- 格式化: Rust 只格式化自己改的文件: `rustfmt --edition 2024 --config-path rustfmt.toml <文件...>` (宽 160). 不跑不带参数的 `cargo fmt` / `cargo fmt --all`:
  blitz 是路径依赖, `vendor/blitz` 会被一起改写.
- 构建不从外网取东西: 依赖都从内网 Gitea 取 (blitz 子模块、rusty_v8 预编译库、CI 的 action), 见 `docs/agents/dev/build-run.md`「构建不从外网取东西」.
  Cargo.toml / package.json 里的仓库地址写 gitea (`http://gitea.dopamine/AC/libacde`).

## 提交与推送

- 仓库在内网 Gitea (`origin` = `ssh://git@gitea.dopamine:2333/AC/libacde.git`), 是主仓库; `main` 和发版标签同步推到 GitHub `git@github.com:0x00-Community/libacde.git`.
  远端只有 `main`, 不推别的分支; 不 force push, 不 amend / rebase 已推送的提交.
- 提交信息英文 Conventional Commits, 每行不超过 72 字符, 正文写原因和验证情况 (性能数字注明负载), 不加 Co-Authored-By. 只提交自己改的路径.
- 改了 acdev / acdes 跟着要改的接口 (Rust 公开项、C 接口、`protocol/`、清单规则、`acde-api/`), 先推本仓库, 再推它们的配套提交; 两个前端都按 `../libacde` 的 main 编.
- 改了 `js/` 时在 `js/` 跑 `bun run typecheck`. 构建、运行和测试见 `docs/agents/dev/testing.md`, 做了就在提交信息里写清已验证 / 未验证.
- 改 CI 工作流或准备发版, 先在本机或执行机上复现 (本机用 `cargo xwin` 等把各平台构建与打包跑一遍), 全部通过后才动云端; 云端失败就回本地修, 不连续重跑.

## 文件路由

每篇什么时候读见 [docs/agents.md](docs/agents.md). 第一次接手先读 `docs/agents/runtime/architecture.md`; 改说明文档先读 `docs/agents/writing-docs.md`.
