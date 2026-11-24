# Shortcuts, the lock screen, notifications and the session

Synced with docs/agents on 2026-11-24

Besides windows and panels, a desktop handles the parts of a session that cut across apps: global keyboard shortcuts, the lock screen, notifications and permission prompts, drag and drop, and the session itself (logging out, switching desktops). This page describes each of them as the runtime offers them to a desktop.

## Global keyboard shortcuts

- `acde:windows.shortcut(combo, scope)` registers a shortcut and `unshortcut(combo)` removes it. Combinations are written `ctrl+alt+t` or `super+k`, with key names from xkb (Shift is part of the combination; `mod` is ⌘ on macOS and Ctrl elsewhere). A lone `super` means "Super pressed and released on its own", not combined with another key.
- **Scopes**: `session` cannot be overridden by anything; `desktop` yields to an app that registered the same combination for itself. The full matching order is session → the focused window's menu shortcuts → its app shortcuts → desktop.
- A matching key press (and its release) never reaches the window. The desktop receives `acde:desktop`'s `on('shortcut')` with `{window, key, title}`: `key` is the combination as registered and `title` holds the scope (a historical field name).
- A combination already taken fails to register, although a rejection is not guaranteed (in acdes a compositor refusal is only logged).
- Shortcuts are given back when the desktop page reloads; register them again. While the screen is locked no shortcut works. Super+Tab belongs to the compositor (next window).
- **Temporary shortcuts** such as "Esc closes this menu" are registered when the menu opens and removed when it closes, because the keyboard may still be in the window underneath and the page would never see the key.
- Key names in acdes today: letters, digits, F1–F12, arrows, Home / End / PageUp / PageDown / Insert / Delete, Esc, Tab, Enter, Space, Backspace, Print and a few symbols; media keys are not recognised yet.

## The lock screen

- **Only the runtime locks.** The desktop calls `acde:session.lock()`, or the runtime locks by itself following the power policy (idle, closing the lid, before sleep, logind's Lock). A desktop never draws a fake lock screen.
- When locking, the runtime starts a **separate process** with the desktop's `lock` page (or its own built-in one), shown on the main screen; other screens are plain colour. The lock page shares nothing with the desktop page: it has its own HTML, CSS and JS, follows `navigator.languages` and `prefers-color-scheme`, and gets only `acde:lock` and `acde:services`.
- **`acde:lock`**: `user()` returns `{name, fullName, avatar}` (`avatar` is a `data:` URL or `null`). `unlock(password)` unlocks and ends the lock process on success, so the page does nothing afterwards. Failures carry a `code`: `auth` (403, wrong password), `busy` (409, the previous attempt is still being checked) or `throttled` (429, with `retryAfter` seconds). `status()` returns battery, network and the number of items waiting for the user, for display. A page opened without `acde:lock` should disable input and say so.
- **Power menu**: the lock page may offer power off, reboot and suspend through the `session` service the session gives it; it cannot log out.
- While locked, the keyboard belongs to the lock page and shortcuts do not work. Before the machine sleeps, the runtime waits for the lock page's first frame, so it should paint quickly and not wait for the network or big assets. If the lock page crashes the screen stays locked (black) and it is restarted a limited number of times.
- **The system confirmation page**: acdes reuses the lock process to ask the person to confirm a risky operation. That page's `acde:lock` additionally has a `confirm` description, `unlock(password, {remember?})` where `remember` is `'once'`, `'task'`, `'scope'` or `'until'` (how long the approval lasts; the password may be empty when none is needed), and `cancel()` to refuse.
- **Designing a lock page**: `acdev dev --lock` opens a desktop's lock page alone with a stand-in `acde:lock`: the password `right-password` unlocks, `wrong-password` is refused, `busy-password` reports a check in progress and `throttled-password` asks to wait 3 seconds.

## Notifications, downloads and permission prompts

- Apps' standard `Notification`s go to the desktop as `acde:desktop`'s `on('notification')` (`app`, `label`, `title`, `body`, `icon`, `tag`). With a desktop present the runtime shows nothing itself: display, do-not-disturb and history are the desktop's. Finished downloads arrive as `on('download')` (acdes does not support app downloads yet).
- Capabilities that need the user's consent (camera, microphone, idle detection) reach the desktop through `acde:permissions`: `on('prompt')` and `on('answered')`, and `pending()` for prompts already waiting when the page starts. **The desktop must answer** with `answer(id, allow)`, or the app waits forever. `list(app?)` and `revoke(app, capability)` support a settings page. acdes has nothing to ask yet and answers 501.

## Drag and drop

- Everything is standard HTML drag and drop. Files dragged from other windows into the desktop page or a panel arrive in `drop` as `File`s (regular files up to 64 MiB) and read-only `FileSystemHandle`s, never as paths: local `text/uri-list` and `text/plain` entries are removed. Read `items` synchronously inside the `drop` handler. To place what was dropped somewhere, the desktop reads it and writes it itself (see [files](../apps/files.md)).
- Dragging out of the desktop page works for `draggable="true"` elements; their string data goes to the system's drag and drop and other windows receive it normally. If a new press arrives before the previous system drag ended, the page ends the old drag as not dropped and starts the new one.

## The session and desktops

- `acde:session`: `lock()`, `logout()`, `reboot()`, `poweroff()`, `setLanguage(tag)` (writes the system locale and fires `languagechange`), `own()` (whether this is the user's own session rather than, say, a login screen) and `others()` (other users' sessions still open).
- `acde:desktops`: `list()` returns the installed desktops and the current one; `choose(id, remember?)` switches.
- `acde:packages`: `refresh()` rescans installed packages, `install(...)` installs one, `natives()` and `runNative(id)` list and start native programs.
- `acde:software` (system packages through acdes): list, refresh, install and remove community store packages, and update everything.

## For developers

[runtime/desktop.md](../../../agents/runtime/desktop.md) ("快捷键", "锁屏页", "拖放", "通知、下载和权限询问"), [runtime/window-model.md](../../../agents/runtime/window-model.md) ("全局快捷键"), [runtime/layout.md](../../../agents/runtime/layout.md) ("锁屏页", "锁屏页预览", "会话与桌面包").
