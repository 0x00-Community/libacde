# Chrome DevTools for pages

Synced with docs/agents on 2026-11-24

In development mode every page, and the isolate that runs desktop services, can be opened in Chrome's (or Edge's) DevTools: the console, breakpoints and stepping, the profiler and heap snapshots, the element tree, styles, network requests and element highlighting. libacde speaks the Chrome DevTools Protocol itself; there is no Chromium inside. Outside development mode there is no debugging port at all.

## Opening DevTools

- **F12** in a host window (fn+F12 on a Mac keyboard) opens DevTools for the page of the focused window: the runtime intercepts the key (the page never receives it) and tells `acdev dev`, which opens the browser. Because the key never goes through the page's thread, F12 works even while the page is stopped at a breakpoint. F12 with modifiers, and F12 outside development mode, reach the page as usual.
- **`acdev devtools [page]`** opens it from the command line, also for pages in a guest system.
- **Manually**: `chrome://inspect` with `127.0.0.1:<port>` added as a target discovery address; `acdev` prints or returns the port.

## The endpoint

- It exists only in development mode: a host window with `ACDE_DEV` set, or a process in an acdev guest system (detected from the virtual machine's firmware configuration, never from environment variables a user or Agent could set).
- It listens on `127.0.0.1` on a port the system chooses. `GET /json/version` and `GET /json/list` describe the targets: each page (`type: page`, id like `notes#7` or `desktop`, title with the app id) and, while services run, one `service` target for the isolate where they all run.
- **Protection**, as with Chrome's own debugging port: the `Host` header must be `127.0.0.1`, `localhost` or `[::1]` (against DNS rebinding), and requests carrying an `Origin` other than `devtools://devtools` are refused, so web pages in a browser cannot connect. Other local users on the same machine can reach it, as with Chrome's and Node's ports. At most 32 connections; slow or stuck clients are disconnected rather than allowed to block a page.

## What works

- **JavaScript** (V8's own domains): the console with real objects and stack traces, sources, breakpoints, `debugger` statements, stepping, pausing a long-running task, the CPU profiler and heap snapshots. While paused, the window keeps its last frame and the page is not reported as hung. Uncaught page errors appear in the console with their stack.
- **Elements**: a read-only element tree that updates live as the page changes; several DevTools windows each see their own expanded tree. Editing nodes or attributes answers "read only".
- **Styles**: the CSS domain, showing the styles applied to an element.
- **Network**: requests made by the page.
- **Overlay**: highlighting the element under the cursor in the page.
- **Source maps**: stack traces and console locations use the app's source maps (`src/main.js:8:9`), and the runtime's own scripts are on DevTools' ignore list, so stepping and stack traces stay in the app's code.

## Reloading without losing DevTools

When a page reloads (`location.reload()`, or `acdev dev` rebuilding), its DevTools stays connected: the runtime keeps the target for up to 10 seconds, queues messages, reconnects to the new page, re-enables the domains DevTools had open and tells it that the context, frame tree and document were replaced. An app reopened in a new window under a desktop is a new target.

## For developers

[dev/devtools.md](../../../agents/dev/devtools.md) (the endpoint, domains, pausing, reload, adding a domain), [dev/dev-ipc.md](../../../agents/dev/dev-ipc.md) (the `devtools` request, the F12 event, development mode in a guest).
