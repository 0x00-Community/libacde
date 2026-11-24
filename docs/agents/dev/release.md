# 发版

运行时不单独发版: 页面资源和 SDK 随 acdev 的一次发布发出 (acdev 的 `.github/workflows/` 四个工作流, 入口是 `tag.yaml`, 见 acdev 的 `docs/agents/release-distribution.md`), 产物在 `pkgs.dopame.ai/acdev/<版本>/`. 构建时把本仓库检出在 acdev 旁边, 版本号由 acdev 的 `scripts/set-version.sh` 一起写进本仓库的 `Cargo.toml` 和 `acde-api/package.json` (不提交).

Rust 包发到 cargo 源 ac 是手动的, 见下面「cargo 源 ac」那条的顺序 (`cargo publish --registry ac`, 从 HEAD 导出的干净副本发).

## cargo 源 ac

- 发布到 gitea.dopamine AC 组织的 cargo 源 `ac` (`.cargo/config.toml`; 读也要令牌, 匿名 401):
  `CARGO_REGISTRIES_AC_TOKEN="Bearer <令牌>" cargo publish -p <包> --registry ac`. ac 上除了 libacde 只有它不得不单独发的依赖: blitz fork 的包
  (blitz-dom / blitz-paint / blitz-traits / stylo_taffy / acss_taffy / debug_timer, 原名; blitz-dom 的 `acss` 特性要 acss_taffy, 它又要 ac 上的 acss), blitz 的 taffy (上游 Blitz 钉的 git 提交, `0.15.0-git.4e59968`);
  2D 场景是本仓库的 `acde-scene` (`scene/`, 同样发到 ac); XDG 用户目录的解析是 `acde-dirs` (`dirs/`, 只用标准库, acdes 的管家与会话以 path 共用, 同样发到 ac).
  顺序: acde-scene、acde-dirs → blitz 那六个 (acss_taffy 在 blitz-dom 之前) (在 `vendor/blitz` 里发) → libacde.
  升版本时改仓库根 `Cargo.toml` 的 `[workspace.package] version`; blitz 升版本时它自己的 `[workspace.package]`、`[workspace.dependencies]` 和这里的 `=0.3.0-beta.N` 一起改.
