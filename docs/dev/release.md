# 发版

运行时不单独发版: 页面资源和 SDK 随 acdev 的一次发布 (`.github/workflows/release.yml`, 见 `docs/release-distribution.md`) 发到
`pkgs.dopame.ai/acde/<版本>/` 和 npm (用户定 2026-11-14 起同一个版本号). 原 acdert 仓库推 tag 触发 acdev runtime release 的
`.gitea/workflows/release.yml` 随并入 acdev 删除: acdev 的 release 自己认 `v*` tag, 构建时把本仓库检出在 acdev 旁边, 版本号由
acdev 的 `scripts/set-version.sh` 一起写进本仓库的 `Cargo.toml` 和 `acde-api/package.json`. acdert 并入 acdev 前的 tag 在 acdev 上加了前缀 `acdert/`
(`acdert/v0.0.11` 等), 本仓库 (2026-11-18 从 acdev 的 `acdert/` 拆出, 连历史) 不带它们. 以前在 `pkgs.dopame.ai/acdert/<版本>/` 的 0.0.9 及更早的版本还留在那里.

Rust 包发到 cargo 源 ac 是手动的, 见下面「cargo 源 ac」那条的顺序 (`cargo publish --registry ac`, 从 HEAD 导出的干净副本发).

## cargo 源 ac

- 发布到 gitea.dopamine AC 组织的 cargo 源 `ac` (`.cargo/config.toml`; 读也要令牌, 匿名 401):
  `CARGO_REGISTRIES_AC_TOKEN="Bearer <令牌>" cargo publish -p <包> --registry ac`. ac 上除了 acdert 只有它不得不单独发的依赖: blitz fork 的包
  (blitz-dom / blitz-paint / blitz-traits / stylo_taffy / debug_timer, 原名), blitz 的 taffy (上游 Blitz 钉的 git 提交, `0.15.0-git.4e59968`);
  2D 场景是 detour 的 (crates.io 的 `detour-gpu`, GitHub 0x00-Community/detour 自己发, 不开 `compiler`), acde-gpu 已并进 `src/gpu`. 顺序: blitz 那五个 (在 `vendor/blitz` 里发) → acdert.
  升版本时改仓库根 `Cargo.toml` 的 `[workspace.package] version`; blitz 升版本时它自己的 `[workspace.package]`、`[workspace.dependencies]` 和这里的 `=0.3.0-beta.N` 一起改.
