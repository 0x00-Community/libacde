# 发版

运行时不单独发版: 页面资源和 SDK 随 acdev 的一次发布 (`.github/workflows/release.yml`, 见 `docs/release-distribution.md`) 发到
`pkgs.dopame.ai/acde/<版本>/` 和 npm (用户定 2026-11-14 起同一个版本号). 原 acdert 仓库推 tag 触发 acdev runtime release 的
`.gitea/workflows/release.yml` 随并入 acdev 删除: acdev 的 release 自己认 `v*` tag, 构建时把本仓库检出在 acdev 旁边, 版本号由
acdev 的 `scripts/set-version.sh` 一起写进本仓库的 `Cargo.toml` 和 `acde-api/package.json`. acdert 并入 acdev 前的 tag 在 acdev 上加了前缀 `acdert/`
(`acdert/v0.0.11` 等), 本仓库 (2026-11-18 从 acdev 的 `acdert/` 拆出, 连历史) 不带它们. 以前在 `pkgs.dopame.ai/acdert/<版本>/` 的 0.0.9 及更早的版本还留在那里.

Rust 包发到 cargo 源 ac 是手动的, 见下面「cargo 源 ac」那条的顺序 (`cargo publish --registry ac`, 从 HEAD 导出的干净副本发).

## cargo 源 ac

- 发布到 gitea.dopamine AC 组织的 cargo 源 `ac` (`.cargo/config.toml`; 读也要令牌, 匿名 401):
  `CARGO_REGISTRIES_AC_TOKEN="Bearer <令牌>" cargo publish -p <包> --registry ac`. ac 上除了 libacde 只有它不得不单独发的依赖: blitz fork 的包
  (blitz-dom / blitz-paint / blitz-traits / stylo_taffy / debug_timer, 原名), blitz 的 taffy (上游 Blitz 钉的 git 提交, `0.15.0-git.4e59968`);
  2D 场景是本仓库的 `acde-scene` (`scene/`, 同样发到 ac). 顺序: acde-scene → blitz 那五个 (在 `vendor/blitz` 里发) → libacde.
  升版本时改仓库根 `Cargo.toml` 的 `[workspace.package] version`; blitz 升版本时它自己的 `[workspace.package]`、`[workspace.dependencies]` 和这里的 `=0.3.0-beta.N` 一起改.

## 第三方许可声明 (`THIRD-PARTY-NOTICES.txt`)

仓库根的 `THIRD-PARTY-NOTICES.txt` 列出运行时带进产品的第三方软件和它们的许可全文, 由 `scripts/notices.ts` 生成, 不手改. acdev 发版时把它
(或用同一脚本对 acdev 自己的锁文件再生成一份, 带上 `--extra scripts/notices/extra.json`) 打进产物.

- **生成 / 核对**: 在仓库根 `bun scripts/notices.ts` 重新生成; `bun scripts/notices.ts --check` 只核对, 下面任何一条都以退出码 1 失败并逐条列出:
  锁文件里的 crate 在本机 cargo 缓存 (`$CARGO_HOME/registry/src`) 里找不到 (先 `cargo fetch --locked`); 路径依赖既不在 `--vendor` 目录里也不在
  `--own` 里; 包里没有许可文件且 `scripts/notices/texts/` 没有它 license 字段的原文; 运行时的 JS (`js/dom/src`、`js/page/src`、`js/launcher/src`)
  引了不在 `js/package.json` 的 `dependencies` 里的 npm 包; 文件和重新生成的结果不一样. 脚本只读本地文件, 不联网.
- **什么时候重新生成**: 改了 `Cargo.lock` (加、删、升依赖, 含 blitz 子模块升级)、`js/package.json` 的 `dependencies`、`scripts/notices/extra.json`
  或 `scripts/notices/texts/`, 以及升 v8 (核对 `extra.json` 里 V8 静态库带的组件是否变了) 时. 发版前跑一次 `--check`.
- **覆盖范围**: Cargo.lock 里的全部包 (各平台的都算, 构建和测试才用的也列上, 多列无害), 自己的包 (`libacde`、`acde-scene`、`acss`) 除外;
  npm 只算 `dependencies` / `optionalDependencies` 及其传递依赖 (现在 `js/` 只有 devDependencies, 运行时的 JS 由 `js/bundle.ts` 打包, 不带 npm 包,
  所以没有 npm 条目). 许可文本取包里附带的 LICENSE / COPYING / COPYRIGHT / NOTICE / AUTHORS 文件 (和 `LICENSES/` 目录); 包里没有时用
  `scripts/notices/texts/<SPDX>.txt` 的标准原文加 `authors` 作版权行, 并注明这一点; 多选一的许可式按 `PREFERRED` 的次序选一个 (宽松的优先).
  相同的许可全文只印一次, 各条目按编号引用.
- **不在包里的组件** (`scripts/notices/extra.json`): rusty_v8 预编译静态库里的 V8、ICU、libc++ / libc++abi / libunwind / llvm-libc、Abseil、
  Highway、simdutf、fast_float、Dragonbox、FP16、PartitionAlloc; Windows 的 DXC (mach-dxcompiler-rs 构建时下载的静态库); blitz-dom 里
  Firefox 的 `-moz-bullet-font` 和默认样式表. 条目的 `within` 是带它进来的 crate, 锁文件里有这个 crate 才列. 这些条目的版权行是离线照上游
  惯例写的, 联网时对一次上游 LICENSE 再改; 升 v8 时对照 rusty_v8 的 `third_party/` 增删. jemalloc 不在依赖里, 没有条目.
- **`scripts/notices/texts/`**: 标准许可原文, 都拷自本机 cargo 缓存里某个 crate 附带的原文 (MIT、Apache-2.0、MPL-2.0、BSL-1.0、Zlib、
  BSD-3-Clause 去掉了原作者的版权行; Unicode-3.0、Apache-2.0 WITH LLVM-exception 原样), 只有 `NCSA.txt` 是按上游模板写的.
- **MPL-2.0**: 纯 MPL 的 crate (Stylo 系、cssparser、symphonia 等) 未修改, 条目里写明源码形式在 crates.io 的对应包和版本. 我们改过的 MPL 文件
  (现在只有 `vendor/blitz/packages/blitz-dom/assets/default.css`, 按文件头的 MPL 声明加改动处的 `acrender:` 标记识别) 改后的全文附在声明文件末尾:
  blitz 分叉和本仓库都不公开, 指向仓库提交收到产物的人拿不到, 随声明附上全文才满足 MPL 3.2 的「提供源码形式」. 以后改别的 MPL 文件,
  在改动处照样标 `acrender:`.
- **给别的仓库用**: `bun scripts/notices.ts --cargo <Cargo.lock>... [--vendor <目录>...] [--own <包名,...>] [--npm <目录>...]
  [--sources <目录>...] [--extra <清单>...] [--title <名称>] --out <文件> [--check]`; `--own` 同时用于 crate 名和 npm 包名.
  不带参数 (或 `--libacde`) 是本仓库的设定.
