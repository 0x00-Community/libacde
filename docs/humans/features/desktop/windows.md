# Managing windows from the desktop: `acde:windows`

Synced with docs/agents on 2026-11-24

Windows belong to the compositor, not to the desktop page's DOM. The desktop sees a table and a stream of events, and can only make requests; where a window goes, what is on top and who has the keyboard is finally decided by the compositor. This page explains that table, what the desktop can ask for, virtual desktops and screenshots. It is written for people building a desktop; apps see their own window through `acde:window` (see [the app window](../apps/windows.md)).

## The model underneath

The window system knows two things: **canvases** (work spaces; one is shown at a time, each with its own stacking order and focus) and **windows**. A window has a mode, a rectangle, a stacking position, content, an input region, a parent and focus. The desktop page is the lowest full-screen window; panels and menus are pinned windows that are always above free and fixed windows. Title bars, borders and shadows are content drawn by the client side, not concepts of the window system.

Clients hand over finished buffers; compositing only produces a plan of layers and damage; presenting (GPU, display, remote desktop) executes it. The window system never reads pixels, and a slow client simply keeps showing its previous buffer.

## The table and its events

- **Identity**: `id` is a window number, unique for this run and never reused (in acdes it changes when the desktop page reloads). `key` is a stable identity from the compositor that survives desktop reloads (not compositor restarts). App windows carry `appId` (an acde app's id, or a native program's own app id, which may be empty), `instance` (which instance of the app) and `pid`; `web: true` marks acde apps.
- **`list()`** returns the current rows; every row has a `state`.
- **`on('window')`** delivers one complete snapshot per window and batch of changes: `appId`, `title`, `state`, `previous`, `hidden`, `rect`, `active`, `parent`, `desk`, `topmost`, `first` (a new window, or one re-reported after a reload) and `changed` (the fields that changed). The older per-field events (`open`, `title`, `state`, `activate`, `parent`, `desk`, `geometry`) are deprecated and still sent, derived from the snapshots; new desktops listen to `window` only.
- **`close`** (the window is gone) and **`crash`** (its process crashed) both remove the window, may both arrive, and must both be handled as "gone". `menus` delivers an app's menus; `currentdesk` the current virtual desktop.
- `ready`, `show`, `attention`, `hang` and `responsive` are not guaranteed (acdes sends none of them today).

## Opening apps

`acde:apps.open(id, request?, {parent, modal, newWindow, root, size})` answers `{window, instance?, running?}`.

- The `window` in the answer may be 0 because the window does not exist yet (acdes always answers this way). The desktop claims the window from the following `first` snapshot by `appId` (and `instance`), and must cope with the snapshot arriving before or after the answer.
- A single-instance app that already runs receives the request (`running: true`) instead of a new window; `newWindow: true` asks for another (honoured for apps with `multiple`).
- `size: [width, height]` sets this new window's page size (logical pixels, title bar excluded; each 1–16384), only when a new process really starts.
- `root` lends a folder to this launch: the session manager decides whether to grant it.
- `acde:openPath(path)` answering `no_app` (404) shows nothing; the desktop draws its own "open with" from `acde:handlers` and `acde:setDefault`.
- Native programs start with `acde:launch.start(argv)` or `acde:packages.runNative(id)`; `acde:packages.natives()` lists the visible `.desktop` entries with icons and keywords. Their windows are claimed from snapshots the same way.

## Placing, focusing and changing state

- **Placement**: new windows are centred in the work area and cascaded by window count; windows with a parent are centred above it. `place(id, [x, y, w, h])` moves a `normal` window (and asks the client to resize when width and height are given), in the compositor's logical coordinates; with several screens, convert using `acde:display`'s `bounds`.
- **Focus**: `focus(id)` is activation in one call: it switches to the window's virtual desktop, unhides it, restores it from minimised, raises it together with its modal dialogs and gives it the keyboard. New windows are **not** guaranteed focus (the keyboard may still be on the desktop because the user just clicked it), so a desktop that wants a new window in front calls `focus` once it has claimed it. Apps cannot take focus; programs hand focus to each other with the standard xdg-activation. Who has focus is the snapshot's `active`.
- **States**: `requestState(id, 'normal' | 'minimized' | 'filled' | 'fullscreen')`. The runtime arbitrates: fixed windows can only be minimised; panels and the desktop page have no state. Title bar buttons and apps' own requests change states too, so the desktop updates itself only from `state` / `previous` in snapshots.
- **Hiding**: `hide(id)` / `show(id)` (the snapshot's `hidden`) is separate from states: a hidden window is not drawn, not hit and gets no focus, but keeps its place and state. It is for full-screen desktop views such as an overview or launcher. Only the desktop can hide windows.
- **Always on top**: `setTopmost(id, on)` (desktop page only) moves a window, with its dialogs and pop-ups, into the topmost band above all ordinary windows (still below the shell's panels); rows show `topmost: true`. Runtimes without it answer `unsupported` (501).
- **Set aside**: `setAside(id, on)` (desktop page only) keeps a window out of Super+Tab and out of the compositor's choice when the keyboard needs a new home; clicking it still focuses it. It is meant for panels the desktop opens as windows. Also 501 where unsupported.
- **Closing**: `close(id)` asks politely (acde apps get `beforeclose` with 3 seconds to answer; native programs get the standard close request). `kill(id)` ends the window's process at once (never the desktop's or the session's own); offer it when `unresponsive` is set on a row (the compositor got no answer to a ping for 5 seconds).
- **Parents and modals**: the snapshot's `parent`. Modal dialogs stay above their parent, and activating the parent focuses the dialog. A task bar that lists only top-level windows groups by it.
- **Application menus**: with a desktop, the runtime draws no menus in title bars; apps' menus arrive through `on('menus')`, and the desktop shows them where it likes and calls `invokeMenu(id, menu, item)`. Whether menu items' shortcuts are bound is up to the desktop.
- **Animation**: `animate(id, {opacity, scale, dx, dy, duration, delay, easing})` plays a short animation on a window; in a host window it is interpolated frame by frame, in acdes it completes at once. Either way the completion event is sent.
- **Statistics**: `stats()` returns per-window figures in a host window (and `{}` in acdes).

## The Agent's stage (acdes)

Native programs that an Agent opens can live on the Agent's own stage: rows carry `stage: true`, the window is drawn on no screen, does not take the person's keyboard and has no virtual desktop. The desktop may show such windows in its task bar or not. `focus(window)` means the person takes over: the window moves to the current desktop, appears and takes the keyboard, and `stage` becomes false.

## Virtual desktops

`addDesk`, `showDesk`, `removeDesk` and `moveToDesk` are optional (host windows lack them):

- **Numbers belong to the desktop**: virtual desktops are non-negative integers; only 0 exists at first, and `addDesk(n)` creates others. Names, order and persistence are the desktop's business; the compositor knows only numbers.
- **Where windows go**: new windows land on the current desktop, windows with a parent on the parent's; `moveToDesk(window, desk)` moves a window with its dialogs. Panels belong to no virtual desktop and appear on all.
- **Switching**: `showDesk(n)` swaps the whole screen. The compositor also switches by itself (focusing a window on another desktop goes there first), so the current desktop is whatever `currentdesk` reports. Follow it; do not push back with `showDesk`. If `currentdesk` reports a number the desktop does not know (left over from before a reload), one `showDesk` to a known desktop is fine.
- **Removing**: `removeDesk(desk, into)` merges its windows into another; removing the current desktop also switches there.
- **Errors** carry a `code`: `unknown_desktop`, `last_desktop`, `same_desktop`, `unknown_window`, `unsupported`.
- Without these methods a desktop emulates virtual desktops with `hide` / `show`.

## Screenshots

- **PrintScreen**, pressed while the desktop page has the keyboard, makes the runtime capture the main screen (without the pointer) to `$XDG_PICTURES_DIR/截图 <time>.png` and send `acde:desktop`'s `on('screenshot')` with `{path, width, height}` or `{path, error}`. Previews, notifications and opening the image are up to the desktop.
- **`acde:windows.screenshot({screen?, window?, cursor?, path?})`** captures on demand and answers `{path, width, height}`: the main screen by default, `screen` by index, or one window's own image (`window`, desktop page only); `cursor: true` includes the pointer; `path` is an absolute file to write. A missing screen or window is 404; capturing a single window while locked is 403 `locked` (the whole screen still works and shows the lock screen); an unwritable path is 400. This call sends no `screenshot` event.
- In a host window only the whole host window can be captured (other targets are 501). From outside a session (ssh), acdes has its own `acdes screenshot` command.

## For developers

[runtime/desktop.md](../../../agents/runtime/desktop.md) ("窗口模型", "虚拟桌面", "截图"), [runtime/window-model.md](../../../agents/runtime/window-model.md) (canvases, modes, states, focus and modal rules), [runtime/webs.md](../../../agents/runtime/webs.md) (the window list after reloads, native apps).
