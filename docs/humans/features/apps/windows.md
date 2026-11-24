# The app window: `acde:window`, title bars and lifecycle

Synced with docs/agents on 2026-11-24

Each app window is one page with its own DOM, styles, JavaScript heap and surface. The window itself (where it is, how big, whether it is minimised or full screen, who has focus) belongs to the window system, not to the page: in a Dopame OS session that is acdes's compositor, under `acdev dev` the host window. A page sees its window through `acde:window`, which every app has without declaring it, and can only **request** changes; the window system decides.

## The window and its states

- **Identity**: `self.id` is the window's number in this run.
- **States**: `normal`, `minimized`, `filled` (fills the work area, what other systems call maximised) and `fullscreen` (fills the whole output). `self.state` is the current one; `requestState(state)` asks for another; `on('state', {state, previous})` reports every change, including ones the page did not ask for (the desktop minimising it, a title bar button, the user activating a minimised window). A page must follow these events rather than remember what it requested: a refused request simply changes nothing and gets no answer.
- **Going back**: leaving `normal` remembers the window's rectangle, and going full screen remembers whether it was `normal` or `filled`, so normal → filled → full screen → back gives the original rectangle again. Activating a minimised window restores its previous state.
- **Modes**: `requestMode('free' | 'fixed' | 'pinned')`: a `free` window can be moved and resized, a `fixed` one only moved (and can only be minimised), a `pinned` one neither. Full screen is a state, not a mode.
- **`minimize()`** minimises the window; `minimizable` (and `on('minimizable')`) tells whether that is possible here.
- **Activation**: `requestActivate()` may be refused or turned into an attention hint; apps cannot take focus by themselves. `on('activate')` / `on('deactivate')` report focus.
- **Grabs**: `requestGrab('pointer' | 'keyboard')` and `releaseGrab(...)`; the answer comes as `on('grab', {kind, granted})`, and a lost grab as `on('grablost', {reason})`. In a host window a grab is refused while someone else holds one, and is taken back when the user clicks another window, presses Esc or focus moves away.
- **The output**: `self.output` is `{width, height, scale, workArea}` of the screen the window is on, in logical pixels, and `on('output')` reports changes. The work area is the output minus the edges that panels reserve (a menu bar, a Dock). Window positions are hints relative to the current output; there are no absolute screen coordinates in the app interface.
- **Parent and modal windows**: a window opened with a parent is centred above it; a modal child keeps focus away from its parent, and clicking the parent focuses the child.

## Where new windows appear

A new window is centred and cascaded by the number of windows open; a window with a parent is centred above it. When an app's last window closes, its rectangle is remembered (`$XDG_STATE_HOME/acde/windows.json`; full-screen windows remember their size before going full screen), and the next time it opens with no other window of that app it comes back there, unless less than 48 pixels would be on screen or the title bar would be above the top edge.

## The title bar

- Apps get a title bar drawn by the runtime, **outside the page's document**: `<html>`, `innerHeight`, `100vh` and `resize` only describe the content area below it. The runtime draws the title, hit-tests it, moves the window when it is dragged and resizes it from the edges (4 pixels) and corners (10 pixels).
- **The title** is `document.title`, or `setTitle(text)`, or the app's label.
- **The buttons** are traffic lights or Windows-style minimise / maximise / close, as the desktop chose. Minimise and full screen are handled by the runtime; close asks the page (below).
- **Its looks** (colours, height, button style and side, alignment) are declared by the desktop for all windows (see [frames and window looks](../desktop/frames-and-looks.md)). When no desktop set a theme, a page that renders dark gets a dark title bar. A page that does not support dark mode gets the desktop's `light` variant when there is one. A page counts as dark the way browsers decide it: its root `color-scheme` contains `dark`, or a `<meta name="color-scheme">` does, and if `light` is also allowed, only while the system prefers dark.
- **`frame: false`** in the manifest removes the title bar; the app draws its own and should hide its own window buttons when `hostControls` is true (a host that draws real window controls, such as a single app under `acdev dev`, which uses the host system's own title bar).
- `setGrabHint(rect, edges?)` marks areas of a frameless page that move or resize the window; the runtime's own hints win where they overlap.

## Menus

`setMenus(menus)` gives the window its application menus (items with labels, shortcuts, checked / disabled states and `onSelect`).

- **With a desktop**, the runtime does not draw menus: they go to the desktop, which may show them in a menu bar or a window menu and calls back when an item is chosen. A desktop that shows no menus is allowed; the menus then have no entry point.
- **Without a desktop** (a single app in a host window), the menus are drawn in the title bar, and items with a `shortcut` get it registered as a menu-scope shortcut: pressing it selects the item, once, even if the page registered the same combination with `shortcut()`. In a Dopame OS session the same menu shortcuts work through the page itself.

## Keyboard shortcuts

`self.shortcut(combo)` registers an app-scope shortcut such as `ctrl+shift+p` (`mod` means ⌘ on macOS and Ctrl elsewhere); `on('shortcut')` reports it, and `unshortcut(combo)` gives it back. Session-wide and desktop shortcuts are checked first, then the focused window's menu shortcuts, then its app shortcuts. In a Dopame OS session app-scope shortcuts are not available yet (they answer 501); in a host window they work.

## Background material, accent colour and wallpaper tone

- `setMaterial(material, options?)` / `material()` puts a material behind the window's content: `glass` (liquid glass, roles `regular` / `clear`, can merge across windows with a `group`), `blur` (blurred and tinted, cheaper, no lens or highlight) or `none`; `region: 'titlebar'` limits it to the title bar. The manifest's `window.material` sets the default. The page background must be transparent for it to show. See [system glass](../graphics/glass.md) for the details and costs.
- `setBackdrop(rect, effect)` blurs what is behind one region of the window.
- `accent()` returns the system accent colour as `#rrggbb`, or `null` when the user never set one; `on('accent')` reports changes. CSS's `AccentColor` cannot tell "not set" from "chose the default blue", which is why this exists.
- `wallTone()` says whether the wallpaper under the window is mostly `light` or `dark` (so text on a transparent window can pick a colour), or `null` when unknown; `on('wallTone')` reports changes, also when the window moves to another screen. Outside a Dopame OS session it is always `null`.

## Closing, crashing and hanging

- **Closing** asks the page first: `on('beforeclose', handler)` may return `false` (or a promise of it) to keep the window. A page that does not answer within 3 seconds is closed anyway. Then the page thread stops, the surface is released and any native windows it started close with it. `window.close()` and `self.close()` close from inside.
- **Reloading**: `location.reload()` restarts the page in the same window. During `acdev dev`, reloads keep form values, scroll positions, focus and selection (see [the dev channel](../dev/dev-channel.md)).
- **Memory**: each page has its own heap limit (`heapMb`). A page that exceeds it is stopped alone; its window reports a crash with reason `memory`, and nothing else is affected.
- **Hangs**: a task that runs longer than 5 seconds marks the window as not responding; the desktop can offer to end it.

## Suspending

The system can suspend an app that is not in use. The window gets `on('suspend')` first; after that:

- an app with `"lifecycle": {"suspendable": true}` is **frozen**: no timers, no frames, no code at all until it resumes, so anything that must be saved must be saved synchronously inside the `suspend` handler;
- any other app is **throttled**: timers run at most once a second and no animation frames are produced.

`on('resume')` comes after timers are running again. `"endsAnytime": true` tells the system it may end the app without asking. There is no separate "about to end" event: the usual `beforeclose`, `beforeunload` and `pagehide` apply. The runtime side is in place; the signals from the session manager are **in development**.

## Visibility

A hidden window (minimised, or hidden by the desktop for an overview) draws nothing and releases its canvas. Its page receives `visibilitychange`, and `requestAnimationFrame` callbacks wait until it is visible again, as in a background browser tab.

## Services, icons and sounds

- **`acde:services`** lists the desktop services this page can reach (`names()`) and reports when they appear or go away (`on('changed', {names, added, removed})`). Services may start after the page, especially in a guest system, so re-import in that handler instead of polling.
- **`acde:icons`** and **`acde:sounds`** look up icons and sounds by freedesktop name in the current theme, and report theme changes.

## For developers

[runtime/webs.md](../../../agents/runtime/webs.md) (frames, input, frame pacing, window state, requests, suspend), [runtime/window-model.md](../../../agents/runtime/window-model.md) (states, modes, focus, grabs, shortcuts), [runtime/layout.md](../../../agents/runtime/layout.md) (menus and shortcuts in a session, services, visibility).
