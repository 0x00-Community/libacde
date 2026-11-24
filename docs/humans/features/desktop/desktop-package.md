# Writing a desktop: the desktop package and its services

Synced with docs/agents on 2026-11-24

In libacde the desktop itself is an ordinary package: a web page that fills the screen behind all windows, plus optional services and a lock page. The runtime knows no particular desktop; Dopame is one example, and any web front end can be one. This page explains what a desktop package declares, what it may rely on, how its page lives and dies, and how its background services work. The pages next to it cover windows, panels, title bars and the other parts of the desktop contract.

## What a desktop may rely on

Only three things, and nothing else:

1. the `acde` field of `package.json` (the SDK's `PackageManifest`);
2. `acde:` modules, typed by the SDK entry `@acde/api/shell`;
3. standard web interfaces, which behave as in a browser.

The compositor's private protocols, acdes's processes and channels, and the way other desktops do things are not part of the contract and change without notice. Methods and events marked optional (`?`) in the SDK may be missing on some runtime or version: a desktop checks for them and degrades when they are absent. The [status table](#what-each-runtime-supports-today) lists today's gaps.

## What a desktop package declares

`acde.desktops[]` has one entry per desktop:

| Field | Required | Meaning |
| --- | --- | --- |
| `id`, `name`, `description` | `id`, `name` | Shown on the login screen and in `acde:desktops.list()`. |
| `entry` | yes | The desktop page: an HTML file in the package (relative, no leading `/`, no `..`). |
| `lock` | no | The lock page, written like `entry`. Without it the runtime's own lock page is used. |
| `services` | no | Background services started and stopped with the desktop; `{name, modules, backend}` also lists the shell modules the service may import and its role towards a desktop backend. |
| `backend` | no | `{unit}`: a systemd system unit template (ending in `@`) for a desktop backend process. Only honoured for system packages. |
| `heapMb` | no | The desktop page's JavaScript heap, at least 32 MB. |

The same package needs an `apiLevel`, and may carry `apps` (apps in a desktop's package are **system apps**: they may declare shell modules, and system apps may be marked `sensitive`), `assets` (URL prefixes to folders) and `services` (service name to module file). The desktop page needs no capabilities: it gets every module.

## Who gets which modules

| Caller | Gets |
| --- | --- |
| Desktop page | All app modules, all shell modules, all system capability modules and all services. It has **no** `acde:window`, because it is not an app window. |
| System app | App modules plus the shell modules and capabilities it declares. |
| Third-party app | App modules plus declared capabilities and services; declaring a shell module refuses the app. |
| Lock page | `acde:lock` and `acde:services` only. |
| Desktop service | Through `ctx.import`, only the shell modules listed in its `modules`. |

The shell modules are: `acde:windows` (every window), `acde:desktop` (panels, title bar theme, desktop events), `acde:packages` (rescan packages, list and run native programs), `acde:session` (lock, log out, reboot, power off, language), `acde:desktops` (list and switch desktops), `acde:permissions` (answer permission prompts), `acde:setDefault`, `acde:inspect` (see [inspecting the desktop](inspect.md)), `acde:software` (the system's packages: list, refresh, install and remove community store packages, update everything, executed by acdes's system steward) and `acde:voice` (speech input, services only).

## The desktop page's life

- **Start**: once the session is up, the runtime opens the desktop page, one per process, filling the main screen behind every window. Other screens only show a wallpaper colour (`acde:windows.setWallpaper({color})`), and windows can be placed on them. Anything that must float above windows (a menu bar, a Dock) has to be pinned as a panel (see [panels](panels.md)); whatever is drawn in the page itself is covered by windows.
- **Never exit on purpose**: if the desktop page exits normally, the whole session ends with its exit code. To log out, call `acde:session.logout()`; to switch desktops, `acde:desktops.choose(id, remember?)` (the runtime stops the apps, services and old desktop and starts the new one without restarting the compositor).
- **Crashes** restart only the desktop page; app windows are not affected. acdes gives up after the fourth crash within 60 seconds and ends the session. If the compositor restarts, the desktop restarts with it.
- **Starting from scratch**: after a restart or reload the page's window list is empty, and the runtime reports every existing window again as a `first: true` snapshot. In acdes the window numbers change but each window's `key` does not, so anything a desktop wants to remember across reloads (which virtual desktop a window belongs to) is keyed by `key`. To tell a first boot from a restart, wait briefly for re-reported windows (Dopame waits half a second).
- **Services may come later** than the page, especially in a guest: follow `acde:services.names()` and `on('changed')` instead of reading once at load.
- **Reloading**: `location.reload()`, F5 and Ctrl+R reload the desktop page (unless the page prevents the keys). Shortcuts the page registered are given back on reload, so registering them again works.

## Background services

A desktop service is an ES module whose default export is a factory. It runs without a window, uses only its `ServiceContext` and standard web interfaces, and returns an `api`: one or more namespaces that pages import as `acde:<namespace>`.

- **Start and stop**: services start with the desktop and stop with it. In acdes each runs in its own process without a window; a crashing service is restarted, but not after the fourth crash within 60 seconds. On stop, the service is disconnected and given 3 seconds before its process ends.
- **The context**: `name`, `package` (the package folder), `state` (a private state folder the session prepared), `env`, `log`, `spawn(argv, {cwd, env, onLine, onExit})` for heavy work in a child process (its stderr goes to the log prefixed with the service name; it gets SIGTERM when the service closes and SIGKILL 3 seconds later), `inspect`, and `import('acde:<module>')` for the shell modules the manifest lists.
- **Declaring modules**: in the manifest a service is `{name, modules}`; in `acde.config.jsonc` (which acdev builds into the manifest) it is a module file or `{"module": file, "modules": [...]}`. A module not listed is refused with a message naming the missing declaration.
- **What services get**: `acde:windows` behaves as on the desktop page (list, calls, all window events); `acde:packages`, `acde:session`, `acde:desktops`, `acde:inspect`, `acde:setDefault` likewise; `acde:desktop` only has `apps()` (panels and title bar themes need a page); `acde:software` has no change events; `acde:permissions` is not available to services; `acde:voice` (speech input: `listen(onEvent)` with `partial`, `final` and `error` events, and `stop(id)`) exists only for services.
- **Callbacks from pages**: a page can pass callbacks to a service (subscriptions). When the page goes away or reloads, those callbacks are released; `ctx.releaseSignal(callback)` gives the service a standard `AbortSignal` that aborts at that moment, so it can clean up.
- **A desktop backend**: a service with a `backend` role can `ctx.backend.connect()` to the desktop's backend process; the session manager makes the connection, so the service never handles the backend's socket or tokens. `ctx.backend.up` and `onChange` say whether the backend is there; connecting while it is down fails with 503.
- **Who may call**: apps must list the service's name in their own `services` to use any of its namespaces.

## Choosing and switching desktops

The runtime starts `--desktop <id|folder>`, else `ACDE_DESKTOP`, else the desktop used last time (`$XDG_STATE_HOME/acde/desktop`), else the first acde desktop, else the first other session found. Other sessions are the Wayland sessions installed on the system (`wayland-sessions/*.desktop`), listed as `session:<file name>`. Choosing one of those hands the session over: the runtime writes a hand-off file and exits with code 3, and the session entry point executes that program instead.

The login screen (`<runtime> --greeter`, a greetd greeter) lets people pick an account, a desktop and type a password, and remembers each account's last desktop. Other display managers can use desktop entries the runtime generates with `<runtime> sessions`.

## Must, may, must not

A desktop **must**: have an `entry` and not exit by itself; pin anything that must be above windows as a panel and reserve edges that windows should avoid; keep its window list from `window` snapshots and `close` / `crash` events, trusting the runtime's states; remember things by `key` across reloads; answer `acde:permissions` prompts where they exist; check optional methods before using them.

A desktop **may** (leaving them out is legal; the feature then has no entry point): a task bar or Dock, window menus, virtual desktops, application menus, a notification centre, screenshot previews, an "open with" dialog, a title bar theme and double-click behaviour, global shortcuts, its own lock page, wallpaper colours on other screens.

A desktop **must not**: draw app windows' content or title bars, put windows into its own DOM, place or focus windows behind the compositor's back, use the compositor's private protocols, or read paths from drops.

## What each runtime supports today

| Feature | acdes session | acdev host window |
| --- | --- | --- |
| Virtual desktops, `setWallpaper` | yes | no (emulate with `hide` / `show`) |
| `place` | yes (only `normal` windows) | yes |
| `kill` | ends the window's process | same as `close` |
| `animate` | completes at once, no motion | interpolated every frame |
| `setFrameTheme`, `setDoubleClick` | yes, delivered to every app process | yes |
| `setWindowLook` | yes (needs a recent compositor) | ignored |
| `on('edge')` (pointer at a screen edge) | not sent | sent |
| Shortcut key names | letters, digits, F1–F12, arrows, Home / End / PageUp / PageDown / Insert / Delete, Esc, Tab, Enter, Space, Backspace, Print and some symbols; no media keys yet | xkb keysym names |
| An app's own `acde:window.shortcut` | 501 | yes |
| `windows.stats()` | `{}` | yes |
| `windows.screenshot()` | whole screen, one screen, one window, with pointer | the whole host window only |
| `acde:permissions`, `capabilities.request` | 501 (nothing to answer yet) | lists are empty; declared capabilities are `available` |
| `apps.open`, open / close / menu events | yes | yes |
| `openPath`, `open-file`, `handlers`, `setDefault`, lent folders | yes | yes; with no acde app, macOS and Windows hand over to the system |

In acdes, system apps can use only `session`, `desktops`, `setDefault`, `packages.refresh` and `software` among the shell modules today; `windows`, `permissions` and `inspect` answer 501, so interfaces that need them stay on the desktop page for now.

Not yet anywhere: forwarding apps' attention requests to the desktop, wallpaper images on secondary screens, and embedding other programs into the desktop across processes.

## For developers

[runtime/desktop.md](../../../agents/runtime/desktop.md) (the whole desktop contract), [runtime/packages.md](../../../agents/runtime/packages.md) (the package, services, contract levels, choosing a desktop, the login screen), [runtime/layout.md](../../../agents/runtime/layout.md) ("服务模式", "连桌面后台").
