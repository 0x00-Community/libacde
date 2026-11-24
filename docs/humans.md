# Docs for people

Details live in `docs/agents/` (listed in [agents.md](agents.md)), the authoritative, up-to-date version. Suggested order: the two explanations, then the feature pages as you need them, then the how-to guide when you need it.

## Explanation

- [humans/explanation/overview.md](humans/explanation/overview.md): what libacde is and how it fits with acdev and acdes.
- [humans/explanation/rendering.md](humans/explanation/rendering.md): how the runtime turns a web page into pixels.

## Features

What the runtime offers apps and desktops, what each feature does, its defaults, options and limits. App developers start with "The app manifest" and "The app window"; desktop authors with "Writing a desktop".

### Apps

- [humans/features/apps/manifest.md](humans/features/apps/manifest.md): the package and app manifest fields, contract levels, capabilities, required and optional, the platform table, approval prompts, `acde:capabilities`, errors, declared actions.
- [humans/features/apps/windows.md](humans/features/apps/windows.md): `acde:window`, window states and modes, placement memory, the runtime-drawn title bar, menus, app shortcuts, materials, accent colour and wallpaper tone, closing, crashes and hangs, suspending, services, icons and sounds.
- [humans/features/apps/files.md](humans/features/apps/files.md): `acde:fs` roots (private, public places, lent folders, USB drives), operations, safe saving, watching, errors, large files, pickers, drag and drop, handing files to other apps.
- [humans/features/apps/opening.md](humans/features/apps/opening.md): how the app for a file or link is chosen, file types, "open with" on Windows, handing over to the system on macOS and Windows, system and private requests between apps.
- [humans/features/apps/system-modules.md](humans/features/apps/system-modules.md): `acde:terminal`, `acde:office`, network, system, Bluetooth, audio, devices, power, input and input methods, display, `acde:launch`.

### The web platform

- [humans/features/web/web-platform.md](humans/features/web/web-platform.md): loading pages, scripts and import maps, style sheets and subresources, forms, secret fields, XML, pointer events, cursors, pointer lock, clipboard, notifications, messages, workers, `crypto.subtle`, known gaps.
- [humans/features/web/media-and-devices.md](humans/features/web/media-and-devices.md): Web Audio, video, camera and microphone, battery, wake lock, idle detection, voice input.
- [humans/features/web/storage.md](humans/features/web/storage.md): storage partitions, `localStorage`, `sessionStorage`, IndexedDB, cookies, the Cache API, `navigator.storage` and the private file system.
- [humans/features/web/network.md](humans/features/web/network.md): app backends (Bun or Node, sockets instead of ports, restarts, proxies, Windows), where requests go, proxies for pages, CORS and other browser rules.

### Desktops

- [humans/features/desktop/desktop-package.md](humans/features/desktop/desktop-package.md): what a desktop relies on and declares, who gets which modules, the desktop page's life, background services, choosing desktops, must / may / must not, what each runtime supports today.
- [humans/features/desktop/windows.md](humans/features/desktop/windows.md): `acde:windows`, the window table and snapshots, opening apps, placement, focus, states, topmost, set aside, closing, menus, the Agent's stage, virtual desktops, screenshots.
- [humans/features/desktop/panels.md](humans/features/desktop/panels.md): pinning panels, reserving edges, keyboard modes, user-only cards, anchored pop-ups, pop-up windows, panel kinds, screen edges.
- [humans/features/desktop/frames-and-looks.md](humans/features/desktop/frames-and-looks.md): title bar themes, dark and light pages, double-click, corners, shadows, motion presets and title bar metrics.
- [humans/features/desktop/session.md](humans/features/desktop/session.md): global shortcuts, the lock screen and the system confirmation page, notifications and permission prompts, drag and drop, `acde:session`, `acde:desktops`, packages and software.
- [humans/features/desktop/inspect.md](humans/features/desktop/inspect.md): `acde:inspect`: the element tree, references, the text snapshot, actions, pixel windows, native windows, production guards.

### Graphics

- [humans/features/graphics/drawing.md](humans/features/graphics/drawing.md): GPU rules, frames, canvas 2D, WebGL, WebGPU, finding out how a page is drawn.
- [humans/features/graphics/glass.md](humans/features/graphics/glass.md): blur, system glass in CSS, levels, fine-tuning, merging, animations, window materials and their cost.
- [humans/features/graphics/appearance.md](humans/features/graphics/appearance.md): dark mode, accent colour, glass level, reduced transparency and motion, wallpaper tone, language.

### Development

- [humans/features/dev/devtools.md](humans/features/dev/devtools.md): opening Chrome DevTools, the endpoint and its protection, what works, staying connected across reloads.
- [humans/features/dev/dev-channel.md](humans/features/dev/dev-channel.md): hot reload, the development log, every debugging request, guest systems and production sessions.
- [humans/features/dev/running.md](humans/features/dev/running.md): ways to run, runtime commands, environment variables, logs and timing, embedding through Rust or C.

## How-to guides

- [humans/how-to/add-a-capability.md](humans/how-to/add-a-capability.md): give apps a new system capability.
