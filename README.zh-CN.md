<a id="top"></a>
<div align="center">

# libacde

### 桌面就是一张网页.
**acde 背后的运行时库: 内嵌 V8、自研 DOM 与排版、GPU 绘制和 `acde:` 系统模块, 全在一个 Rust 包里.**

[![Rust](https://img.shields.io/badge/Rust-2024-orange?style=for-the-badge&logo=rust)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/version-1.0.0-blue?style=for-the-badge)](https://github.com/0x00-Community/libacde/releases/tag/v1.0.0)
[![License](https://img.shields.io/badge/License-AGPL--3.0--only-brightgreen?style=for-the-badge)](LICENSE)

[![V8](https://img.shields.io/badge/JS-%E5%86%85%E5%B5%8C%20V8-4B8BF5?style=flat-square&logo=v8)](#-libacde-是什么)
[![Web](https://img.shields.io/badge/Web%20%E6%8E%A5%E5%8F%A3-%E6%A0%87%E5%87%86%E3%80%81%E4%B8%8D%E9%AD%94%E6%94%B9-F7DF1E?style=flat-square&logo=javascript&logoColor=black)](#-第一原则)
[![GPU](https://img.shields.io/badge/GPU-wgpu%20%C2%B7%20vello-262D3A?style=flat-square)](#-有什么不同)

[🇺🇸 English](README.md) · **🇨🇳 简体中文**

</div>

---

## ⚡ libacde 是什么

libacde 是 **acde 的运行时库**. acde 是一个桌面环境, 桌面本身和每个应用都是网页.
它内嵌 V8 跑 JavaScript, 用自己的 DOM ([Blitz](https://github.com/DioxusLabs/blitz) 的 fork) 排版, 经 wgpu 和 vello 在 GPU 上绘制,
并给页面提供网页存储、媒体、WebGPU / WebGL 和一组 `acde:` 系统模块.

各部分之间的契约也都归它: **包清单**及其规则、**窗口模型**、**桌面包**、SDK 类型声明 `@acde/api` 和 `acde_*` 协议.

🧩 **一个包, 两个前端:** 对外是 Rust 包 `libacde` (库名 `acde`: rlib, 外加 `libacde.so` 和 C 头文件 `include/acde.h`). 两个前端链接它:

- **acdev**: 跨平台调试运行时 (Linux、Windows、macOS 上的宿主窗口) 和命令行 `acdev`.
- **acdes**: Dopame OS 在 Linux 上的系统态运行时 (Wayland 合成器、DRM/KMS、会话、RDP).

平台特化的部分只经接口进来 (`src/host` 的 `Platform` / `Window`), 所以同一套运行时既能跑完整的系统会话, 也能在别人的桌面里开一个调试窗口.

## 🎯 第一原则

> **不许在任何层次上魔改 web 接口.**

- DOM、`window`、`fetch`、`localStorage`、事件、Web Audio、WebGPU …: 行为、签名、事件、错误都照标准. 不给标准对象加私有方法、字段或事件.
  还没做的就是缺口, 不拿长得像的私有版本顶替.
- **全局只有两个:** `window` (web 兼容) 和 `globalThis` (node 兼容). 没有 `acde` 全局.
- **系统能力只有一个入口:** `acde:` 内置模块, 照 `node:fs` 的写法导入:

```js
import { fs } from 'acde:fs'
import self from 'acde:window'
```

- `acde:` 模块是**进程内注入**, 像 workerd 的 binding: 直接注入页面所在的 V8 isolate, 调用直接进 Rust 原生实现.
- 清单的 `capabilities` 决定页面能导入哪些模块, 没开的导入直接失败.

## 🔥 有什么不同

| | |
| --- | --- |
| 🌐 **桌面就是页面** | 桌面是一个包, 入口是一张 HTML, 满屏铺在所有窗口后面. 任何桌面包都能当桌面, 运行时不认识任何一个具体的桌面. |
| 🦀 **底下没有浏览器** | 没有 Chromium, 没有 Electron, 没有 Node. JS 在 libacde 自己内嵌的 V8 里; 排版和绘制是 Blitz 的 fork 画进 `acde-scene`, 由 vello 在 wgpu 上渲染. |
| 🔌 **进程内的系统模块** | `acde:fs`、`acde:terminal`、`acde:window`、`acde:display`、`acde:audio`、`acde:network`、`acde:power` …, 类型在 `@acde/api`, 每个应用按清单授权. |
| 🪟 **真正的窗口模型** | 窗口表、模式与状态、焦点、模态对话框、grab 和全局快捷键都在这里定义一次, 所有前端共用 ([docs/runtime/window-model.md](docs/runtime/window-model.md)). |
| 📦 **有契约的包** | 包的 `package.json` 声明桌面、应用、资源和后台服务; 运行时和命令行共用同一套清单规则 (`manifest/`). |
| 🔒 **带版本的契约级别** | 只有一个来源 `acde-api/api-level`, 编译时读入; 清单写明自己需要的级别. |
| 🛠️ **自带 DevTools** | 开发模式 (`ACDE_DEV`) 下只在 `127.0.0.1` 开 CDP 端点; 正式运行没有远程调试端口. |

## 📦 一个桌面包

```json
"acde": {
  "desktops": [{ "id": "mydesk", "name": "My Desk", "entry": "dist/ui/desktop/index.html", "services": ["assistant"] }],
  "apps": [{ "id": "term", "label": "终端", "icon": "term", "entry": "/ui/terminal/term.js", "capabilities": { "terminal": true } }],
  "assets": { "/ui/": ["dist/ui"] },
  "services": [{ "name": "assistant", "module": "dist/services/assistant.js" }]
}
```

- **desktops**: 一张 HTML 入口, 满屏在所有窗口后面; 还可以给锁屏页 (`lock`).
- **apps**: 每个应用有入口和它要的 `capabilities`.
- **services**: 随桌面起停的 ES 模块; 应用要写明服务名才能调用.

完整规则见 [docs/runtime/packages.md](docs/runtime/packages.md) 和 [docs/runtime/desktop.md](docs/runtime/desktop.md).

## 🧱 目录

| 路径 | 是什么 |
| --- | --- |
| `src/` | `libacde` 包: `host` (`Platform` 接口, 平铺导出)、`runtime`、`render`、`gpu`、`vello`、`webgl`、`storage`、`media`、`model`、`napi`、`log`; C 接口在 `src/ffi.rs` |
| `scene/` | `acde-scene`: Blitz 画进去、GPU 后端画出来的 2D 场景 |
| `dirs/` | `acde-dirs`: XDG 用户目录, 只用 std |
| `wire/` | `acde-wire`: 前端共用的会话控制通道和合成器客户端协议 |
| `js/` | 运行时自带的 JS (网页接口层、页面胶水、单应用启动器), 构建时打进二进制 |
| `acde-api/` | `@acde/api`: SDK, 只有类型声明 (`acde:*` 模块、清单、配置、服务、能力) |
| `manifest/` | 清单规则, 运行时和命令行共用 |
| `protocol/` | Wayland 协议 |
| `vendor/blitz` | 我们的 Blitz fork (git 子模块) |
| `docs/` | 各部分的设计说明 (中文) |

## 🚀 开始

```sh
git clone --recursive https://github.com/0x00-Community/libacde.git
```

> **构建前提: 即将公开.** libacde 1.0.0 的 Blitz fork (`vendor/blitz`)、配套的 crate 和 V8 预编译库目前还从我们自己的包源取, 这些包源还没公开.
> 我们正在发布它们; 在那之前, 公开环境下的全新构建预计跑不通. 请关注本仓库的更新.

现在就能读、能用的:

- `acde-api/`: 写 acde 桌面和应用用的 SDK 类型.
- `docs/runtime/architecture.md`: 运行时、前端、进程与线程怎么分.
- `docs/runtime/ffi.md`: C 接口和 `include/acde.h`.

## 📚 文档

| 读这个 | 什么时候 |
| --- | --- |
| [docs/runtime/architecture.md](docs/runtime/architecture.md) | 第一次来: 前端、跑法、进程与线程 |
| [docs/runtime/layout.md](docs/runtime/layout.md) | 哪个模块或目录归谁 |
| [docs/runtime/packages.md](docs/runtime/packages.md) | 清单的 `acde` 字段、桌面服务、契约级别 |
| [docs/runtime/desktop.md](docs/runtime/desktop.md) | 写一个桌面: 窗口、面板、锁屏页、拖放、快捷键 |
| [docs/runtime/webs.md](docs/runtime/webs.md) | 出帧、`acde:` 模块注入、能力、沙箱、web 生命周期 |
| [docs/acde-api.md](docs/acde-api.md) | SDK `@acde/api` 和清单规则 |
| [docs/graphics/render-gpu.md](docs/graphics/render-gpu.md) | `src/render`、canvas 2D、模糊、GPU 设备 |
| [docs/web/](docs/web) | 存储、fs、后端、默认应用、语义树 (`acde:inspect`) |
| [docs/dev/devtools.md](docs/dev/devtools.md) | DevTools 端点和 CDP 域 |

## 📄 许可证

libacde 以 **[AGPL-3.0-only](LICENSE)** 授权.

`vendor/` 下的第三方代码 (包括 `vendor/blitz` 的 Blitz fork) 和 `src/napi` 保留各自原有的许可证和版权声明, 见这些目录里的许可证文件.

<div align="center">

**🌐 桌面就是一张网页. 网页还是网页.**

[⬆️ 回到顶部](#top) · [🇺🇸 English](README.md)

</div>
