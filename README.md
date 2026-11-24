<a id="top"></a>
<div align="center">

# libacde

### 🌐 A desktop is a web page. 🤯 The *whole* desktop. 🚀
**🧬 The hyper-integrated runtime engine behind acde: ⚡ embedded V8 · 🌳 its own DOM and layout · 🎮 GPU-native painting · 🔌 `acde:` system modules. 🦀 All in ONE Rust crate.**

[![Rust](https://img.shields.io/badge/Rust-2024-orange?style=for-the-badge&logo=rust)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/version-1.0.0-blue?style=for-the-badge)](https://github.com/0x00-Community/libacde/releases/tag/v1.0.0)
[![License](https://img.shields.io/badge/License-AGPL--3.0--only-brightgreen?style=for-the-badge)](LICENSE)

[![V8](https://img.shields.io/badge/JS-V8%20embedded-4B8BF5?style=flat-square&logo=v8)](#-what-is-libacde)
[![Web](https://img.shields.io/badge/Web%20APIs-standard%2C%20unmodified-F7DF1E?style=flat-square&logo=javascript&logoColor=black)](#-the-first-rule-the-web-is-sacred)
[![GPU](https://img.shields.io/badge/GPU-wgpu%20%C2%B7%20vello-262D3A?style=flat-square)](#-why-its-next-level)

[![Zero Chromium](https://img.shields.io/badge/Chromium-zero-FF4136?style=flat-square&logo=googlechrome&logoColor=white)](#-why-its-next-level)
[![acde modules](https://img.shields.io/badge/acde%3A-system%20modules-00C2A8?style=flat-square)](#-the-first-rule-the-web-is-sacred)
[![DevTools](https://img.shields.io/badge/DevTools-CDP%20built%20in-FF851B?style=flat-square&logo=googlechrome&logoColor=white)](#-why-its-next-level)

**🇺🇸 English** · [🇨🇳 简体中文](README.zh-CN.md)

</div>

---

## ⚡🧠 What is libacde

libacde is **the runtime library of acde**, the desktop environment where the desktop itself and every app are web pages.
It embeds V8 for JavaScript, lays out and paints pages with its own DOM (a fork of [Blitz](https://github.com/DioxusLabs/blitz))
on the GPU through wgpu and vello, and gives pages web storage, media, WebGPU / WebGL, and a set of `acde:` system modules.

It also owns every contract between the pieces: the **package manifest** and its rules, the **window model**, **desktop
packages**, the SDK type declarations `@acde/api`, and the `acde_*` protocols.

🧩💪 **One crate, two front ends:** it ships as the Rust package `libacde` (library name `acde`: an rlib, plus `libacde.so`
and the C header `include/acde.h`). Two front ends link it:

- ✨ **acdev**: the cross-platform development runtime (a host window on Linux, Windows and macOS) and the `acdev` CLI.
- ⚡ **acdes**: the system runtime of Dopame OS on Linux (Wayland compositor, DRM/KMS, session, RDP).

Platform-specific parts come in only through interfaces (`Platform` / `Window` in `src/host`), so the same runtime drives
a full OS session and a debugging window on someone else's desktop.

## 🎯⚔️ The first rule: the web is sacred

> **⚔️ Never bend the web APIs. At any layer. Ever. 🛡️**

- 🔥 DOM, `window`, `fetch`, `localStorage`, events, Web Audio, WebGPU …: behaviour, signatures, events and errors follow the
  standards. No private methods, fields or events on standard objects. What is not done yet is a gap, not a look-alike.
- 💎 **Only two globals:** `window` (web compatible) and `globalThis` (node compatible). There is no `acde` global.
- 🚀 **System access has exactly one door:** `acde:` built-in modules, imported the way you import `node:fs`:

```js
import { fs } from 'acde:fs'
import self from 'acde:window'
```

- 🎯 `acde:` modules are **injected in-process**, like workerd bindings: straight into the page's V8 isolate, calls go
  directly into native Rust.
- 🌟 The manifest's `capabilities` decide which modules a page may import; anything not granted fails to import.

## 💥🔥 Why it's next-level

| | |
| --- | --- |
| 🌐🖥️ **The desktop IS the page** | A desktop is a package with an HTML entry that fills the screen behind every window. Any desktop package can be the desktop; the runtime knows none of them by name. |
| 🦀🚫 **Zero browser. Zero Chromium. Zero Electron.** | No Chromium, no Electron, no Node. JS runs in libacde's own embedded V8; layout and painting are a Blitz fork drawing into `acde-scene`, rendered by vello on wgpu. |
| 🔌⚡ **Native system superpowers** | `acde:fs`, `acde:terminal`, `acde:window`, `acde:display`, `acde:audio`, `acde:network`, `acde:power` … typed in `@acde/api`, gated per app by the manifest. |
| 🪟👑 **A first-class window model** | Window table, modes and states, focus, modal dialogs, grabs and global shortcuts are defined here, once, for every front end ([docs/agents/runtime/window-model.md](docs/agents/runtime/window-model.md)). |
| 📦📜 **Contract-grade packages** | A package's `package.json` declares desktops, apps, assets and background services; one set of manifest rules (`manifest/`) is shared by the runtime and the CLI. |
| 🔒🧬 **One versioned contract level to rule them all** | A single source of truth, `acde-api/api-level`, read at compile time; manifests state the level they need. |
| 🛠️🔬 **DevTools, built right in** | In dev mode (`ACDE_DEV`) a CDP endpoint opens on `127.0.0.1` only; production runs have no remote debugging port. |

## 📦✨ A desktop package, end to end

```json
"acde": {
  "desktops": [{ "id": "mydesk", "name": "My Desk", "entry": "dist/ui/desktop/index.html", "services": ["assistant"] }],
  "apps": [{ "id": "term", "label": "Terminal", "icon": "term", "entry": "/ui/terminal/term.js", "capabilities": { "terminal": true } }],
  "assets": { "/ui/": ["dist/ui"] },
  "services": [{ "name": "assistant", "module": "dist/services/assistant.js" }]
}
```

- 💥 **desktops**: an HTML entry, full screen behind all windows; may also give a lock screen page (`lock`).
- ✨ **apps**: each with an entry and the `capabilities` it asks for.
- ⚡ **services**: ES modules that run alongside the desktop and start and stop with it; apps must name a service to call it.

The full rules are in [docs/agents/runtime/packages.md](docs/agents/runtime/packages.md) and [docs/agents/runtime/desktop.md](docs/agents/runtime/desktop.md).

## 🧱🗺️ Layout

| Path | What it is |
| --- | --- |
| 🦀 `src/` | The `libacde` crate: `host` (the `Platform` interface, exported flat), `runtime`, `render`, `gpu`, `vello`, `webgl`, `storage`, `media`, `model`, `napi`, `log`; the C interface in `src/ffi.rs` |
| 🎨 `scene/` | `acde-scene`: the 2D scene Blitz paints into and the GPU backends draw |
| 📁 `dirs/` | `acde-dirs`: the XDG user folders, std only |
| 🔗 `wire/` | `acde-wire`: the session control channel and compositor client protocols shared by the front ends |
| 🟨 `js/` | The runtime's own JS (web API layer, page glue, single-app launcher), bundled into the binary at build time |
| 📘 `acde-api/` | `@acde/api`: the SDK, type declarations only (`acde:*` modules, manifest, config, services, capabilities) |
| 📜 `manifest/` | Manifest rules, shared by the runtime and the CLI |
| 🌊 `protocol/` | Wayland protocols |
| ⚙️ `vendor/blitz` | Our Blitz fork (git submodule) |
| 📚 `docs/agents/` | Design notes per area (in Chinese) |

## 🚀🛫 Getting started

```sh
git clone --recursive https://github.com/0x00-Community/libacde.git
```

> **Build prerequisites — coming soon.** libacde 1.0.0 still resolves the Blitz fork (`vendor/blitz`), its companion crates
> and prebuilt V8 archives from our own package sources, which are not public yet. We are publishing them; until then a
> clean public build is not expected to work. Watch this repository for the update.

What you can read and use today:

- 🔥 `acde-api/`: the SDK types for writing desktops and apps against acde.
- 💎 `docs/agents/runtime/architecture.md`: how the runtime, the front ends and the processes and threads fit together.
- 🚀 `docs/agents/runtime/ffi.md`: the C interface and `include/acde.h`.

## 📚🧭 Docs

New here? Start with [docs/humans.md](docs/humans.md). The authoritative notes, kept in step with the code, are listed in [docs/agents.md](docs/agents.md).

| Read this | When |
| --- | --- |
| 🏛️ [docs/agents/runtime/architecture.md](docs/agents/runtime/architecture.md) | First time here: front ends, how it runs, processes and threads |
| 🗺️ [docs/agents/runtime/layout.md](docs/agents/runtime/layout.md) | Which module or directory owns what |
| 📦 [docs/agents/runtime/packages.md](docs/agents/runtime/packages.md) | The `acde` manifest field, desktop services, contract level |
| 🖥️ [docs/agents/runtime/desktop.md](docs/agents/runtime/desktop.md) | Writing a desktop: windows, panels, lock screen, drag and drop, shortcuts |
| 🕸️ [docs/agents/runtime/webs.md](docs/agents/runtime/webs.md) | Frames, `acde:` module injection, capabilities, sandboxing, web lifecycle |
| 📘 [docs/agents/acde-api.md](docs/agents/acde-api.md) | The SDK `@acde/api` and the manifest rules |
| 🎮 [docs/agents/graphics/render-gpu.md](docs/agents/graphics/render-gpu.md) | `src/render`, canvas 2D, blur, GPU devices |
| 🌐 [docs/agents/web/](docs/agents/web) | Storage, fs, backends, default apps, accessibility tree (`acde:inspect`) |
| 🔬 [docs/agents/dev/devtools.md](docs/agents/dev/devtools.md) | The DevTools endpoint and CDP domains |

## 📄⚖️ License

libacde is licensed under **[AGPL-3.0-only](LICENSE)**.

Third-party code under `vendor/` (including the Blitz fork in `vendor/blitz`) and `src/napi` keeps its original
licenses and copyright notices; see the license files in those directories.

<div align="center">

**🌐🖥️ The desktop is a web page. 🔥 The web stays the web. 🚀**

[⬆️ Back to top](#top) · [🇨🇳 简体中文](README.zh-CN.md)

</div>
