# How the runtime draws a web page

Synced with docs/agents on 2026-11-24

There is no browser inside libacde. Each window is a web page whose JavaScript runs in its own V8 isolate; the DOM lives in Rust, is styled and laid out by a Blitz fork, and is painted with vello on the GPU. The front end (acdev's host window, or acompositor in an acdes session) composites the finished surfaces onto the screen.

```mermaid
flowchart LR
  js["Page JS<br/>(V8 isolate, one per web)"] --> dom["Web API layer<br/>js/dom"]
  dom -->|DomNative calls| rdom["Rust DOM<br/>src/render"]
  rdom --> style["Style and layout<br/>(Blitz fork, vendor/blitz)"]
  style --> scene["2D scene<br/>acde-scene"]
  scene --> vello["vello on the GPU<br/>(src/vello)"]
  vello --> surface["The window's surface"]
  surface --> comp["Compositor<br/>hostwin (acdev) or acompositor (acdes)"]
```

## Why it is built this way

- **Web APIs stay standard.** Pages only see what a browser offers, so ordinary web code and frameworks work. System features come through `acde:` modules instead (see the rule at the top of [AGENTS.md](../../../AGENTS.md)).
- **One isolate per window.** A window that runs out of memory or hangs takes only itself down; the others keep running. See [runtime/architecture.md](../../agents/runtime/architecture.md) "进程与线程".
- **Pixels stay on the GPU.** The pipeline never copies pixels through the CPU except where a standard API demands them. The hard constraints are in [graphics/gpu.md](../../agents/graphics/gpu.md).

## Where each step lives

| Step | Read |
| --- | --- |
| Web API layer and page glue (`js/`) | [runtime/layout.md](../../agents/runtime/layout.md) "js" |
| Rust DOM, frames, canvas 2D, window decorations | [graphics/render.md](../../agents/graphics/render.md) |
| Blur and system glass | [graphics/glass.md](../../agents/graphics/glass.md) |
| Surfaces and the `Shell` object | [graphics/shell.md](../../agents/graphics/shell.md) |
| Frames, panels and module injection per window | [runtime/webs.md](../../agents/runtime/webs.md) |
