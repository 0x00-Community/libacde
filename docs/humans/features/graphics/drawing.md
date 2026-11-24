# Drawing: canvas, WebGL, WebGPU and the GPU

Synced with docs/agents on 2026-11-24

Pages are painted on the GPU. The DOM is laid out by a fork of Blitz and drawn with vello; canvas 2D, WebGL and WebGPU draw on the same GPU device. Pixels stay on the GPU from the page to the screen, except where a standard interface demands them on the CPU. This page describes what pages can draw with and what happens when there is no GPU. How a page becomes pixels in general is explained in [how the runtime draws a web page](../../explanation/rendering.md).

## Hard rules behind the drawing

- **No CPU copies and no GPU read-backs** on the way to the screen. The exceptions are the remote desktop (which reads back damaged areas), screenshots and capture, and standard interfaces that must hand pixels to the page (`getImageData`, `toDataURL` and the like).
- **GPU first, CPU when there is none**: real GPUs are preferred, and CPU-type devices (lavapipe) are only used when allowed (`ACDE_GPU_ALLOW_CPU=1`, for development and measurement). acdes can paint pages on the CPU on machines without a GPU.
- **Backends**: the runtime draws through wgpu, which gives the same results on Linux, Windows and macOS; acdes can add a second backend for particular GPUs. On Windows the DirectX shader compiler is linked in.
- **Budget**: at 120 frames a second each frame has 8 ms. Changes are measured against that.

## Frames

A page asks for a frame (`requestAnimationFrame`, a style change, an animation) and gets the next one as soon as the previous frame has been handed over, but no sooner than 0.9 of the fastest display's refresh period. Hidden windows produce no frames. On macOS host windows frames follow the display's refresh; `ACDE_WEB_FPS` fixes a rate. CSS animations and transitions run on the render thread's clock.

## Canvas 2D

- Canvas 2D is recorded as a command stream and drawn on the GPU. Very long recordings (over 20000 commands) are flushed into a texture and continued. A canvas used as an image source stays on the GPU.
- `getImageData`, `toDataURL` and `toBlob` read back synchronously. Without a GPU device, a CPU rasteriser registered by the front end draws the canvas for these calls; if none is registered they return transparent pixels and an empty image.
- `ctx.filter` supports `blur`, `brightness`, `contrast`, `grayscale`, `hue-rotate`, `invert`, `opacity`, `saturate`, `sepia` and `drop-shadow` (not `url()`); `shadowBlur` uses the same GPU blur.
- `drawImage` of an SVG `<img>` draws the SVG at its natural size, as `<img>` does.
- `createImageBitmap` accepts `<img>`, `Blob`, `ImageData`, canvases and other bitmaps, with crop rectangles, resizing, `imageOrientation: 'flipY'` and `premultiplyAlpha`; `<video>` is not accepted yet.
- Known gap: `createPattern` takes its picture at first use rather than at creation; canvases are never marked "tainted" by cross-origin images.

## WebGL 1 and 2

- Both are available on every operating system (Windows has not been verified), implemented as a GL state machine on top of wgpu on the shared device: no ANGLE and no system OpenGL.
- Shaders in GLSL ES 1.00 and 3.00 are translated to wgpu's shader form directly. The default framebuffer is RGBA8, with 4x multisampling when `antialias` is set. Occlusion queries and fences work after the task that ended them.
- Triangle fans, line loops and 8-bit indices, which wgpu lacks, are expanded on the CPU.
- **Not yet**: transform feedback, pixel pack / unpack buffers, `EXT_disjoint_timer_query`, uploading data into 24-bit or 32-bit float depth textures, a few format conversions and blits, wide points with indexed or instanced drawing, bool arrays in uniforms, stricter shader validation like ANGLE's, origin-clean checks, and restoring a lost context.
- `RUST_LOG=perf::webgl=debug` logs each new render pipeline and how long it took to build (the first compile on Metal can take tens of milliseconds).

## WebGPU

`navigator.gpu` exists on Linux and macOS, on the same device the page is rasterised on. Callbacks such as `mapAsync` and `onSubmittedWorkDone` are woken by GPU fences where the device supports them (in a Dopame OS session), and by a 2 ms poll elsewhere. On Windows WebGPU is still switched off.

## Window decorations and the frame-rate overlay

The runtime draws app title bars itself, in the style the desktop declared (see [title bars and window looks](../desktop/frames-and-looks.md)). In a host window, Shift+F12 shows a frame-rate overlay drawn by the same renderer.

## Finding out how a page is drawn

During development, `acdev render` reports the path a page actually takes: the GPU adapter and driver (or CPU rasterisation, a lost device, no device), and every layer with a backdrop filter or glass, how it was drawn, and why it was degraded (for example "the CPU cannot refract" or "the glass group is full"). See [the dev channel](../dev/dev-channel.md).

## For developers

[graphics/gpu.md](../../../agents/graphics/gpu.md) (hard rules, budgets, WebGPU, known issues), [graphics/render.md](../../../agents/graphics/render.md) (the renderer, frames, canvas 2D, decorations), [graphics/webgl.md](../../../agents/graphics/webgl.md), [graphics/vendor.md](../../../agents/graphics/vendor.md) (the Blitz fork).
