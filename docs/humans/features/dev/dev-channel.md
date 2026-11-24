# The dev channel: hot reload, logs and debugging requests

Synced with docs/agents on 2026-11-24

While `acdev dev` runs, the runtime opens a private channel that acdev's commands, its MCP server and its test driver use to reload pages, read their console and network activity, look at and operate windows, and control time, dialogs, pickers and responses. This page describes what the runtime offers on that channel; the commands themselves are documented in acdev. The same requests reach pages in an acdev guest system through acdes's session manager.

## The channel

- On Linux and macOS it is a Unix socket in a private folder (only the user can open it); on Windows a named pipe that refuses remote clients and other users. It can evaluate code in pages, so it is for the developer only.
- Messages are one JSON object per line. The connection that introduces itself as `acdev dev` owns the session: when it closes, the runtime exits. Other tools connect alongside.
- Built projects reach the runtime through `ACDE_PATH`, never by copying into installed packages; the log is the runtime's standard error.

## Reloading

- **Hot reload**: when a rebuild changed only `.css` and `.js` files and the HTML is otherwise the same, the page swaps in place: style sheets are replaced, and changed function bodies in module scripts are replaced through V8's live edit. Anything that cannot be swapped (a module's top level, a function currently on the stack, classic scripts) falls back to a full reload.
- **Full reload** keeps what the user was doing: before closing, the old page is asked for form values, scroll positions, focus and selection (elements matched by id or position), and the new page gets them back a few times during its first 1.5 seconds. Component state in JavaScript is not kept.
- A reload of an app reopens its windows where they were (re-centred if the manifest's size changed); a reload of the desktop restarts the desktop page but not its services.
- Quitting closes pages, stops backends and removes sockets, with a hard exit after 6 seconds.

## The development log

In development mode each page reports, at no cost when nobody listens:

- **console output and uncaught errors** (`console.*`, errors, unhandled rejections, and `alert` / `confirm` / `prompt` as `dialog` entries), with stack positions translated through the app's source maps;
- **requests** from `fetch`, XHR, `EventSource` and WebSockets: start, response, bytes received, errors (including why an address could not be served).

The runtime keeps the last 5000 of each kind (texts up to 16 KB), so the record survives a page crash or reload. Readers page through it with a sequence number and are told how many entries were dropped, so a follower never misses or repeats an entry. Entries carry times that can be aligned across several page processes.

## Requests

Each request names a page by its name (`notes#1048576`), an app id, or `desktop`:

| Request | What it does |
| --- | --- |
| `webs` | Lists pages (name, app, kind, window, title). A page that does not answer in time is still listed, marked as stalled. |
| `eval` | Evaluates code in a page (or calls a function on one element) and returns the result as JSON. |
| `shot`, `screen` | A page's pixels, or the whole screen with every window. |
| `snapshot`, `act`, `run`, `query`, `open` | The text view and actions of [acde:inspect](../desktop/inspect.md); `open` also opens a file or link with an app. |
| `console`, `network` | The development log above, filtered by page, level or failure. |
| `emulate`, `appearance` | Force the colour scheme, glass level or reduced transparency for all pages. |
| `choose` | Answer the next folder, open or save picker in advance (or cancel it), without a dialog. |
| `route`, `unroute` | Give matching `fetch` / XHR requests a fake response or a failure, by URL glob or regular expression, optionally a limited number of times. WebSockets and EventSource are not routed. |
| `drop` | Simulate dropping files from the system onto a window at a point, exactly like a real drop. |
| `pointer`, `input` | Move the pointer, or inject raw mouse, wheel, key and text events (with waits that keep keys held). |
| `hit` | What is at a screen point: which window, which part (page, title bar, panel, native), which element, and which `pointer-events: none` elements the pointer passes through. |
| `render` | How a page is really drawn: GPU or CPU, the adapter, and every blurred or glass layer with how it was drawn and why it was degraded. |
| `clipboard` | Write and read the clipboard (the session's own when headless). |
| `clock` | Take over a page's time like Playwright's clock: install, pause at a time, fast-forward, run for a duration, fix or move `Date`. CSS animations keep real time. |
| `dialog` | Answer the page's next `alert` / `confirm` / `prompt`. |
| `init`, `uninit` | Run a script in pages before their HTML is parsed. |
| `reload`, `resize` | Reload a page; resize its content area. |
| `devtools` | The DevTools port (see [DevTools](devtools.md)). |
| `permissions`, `answer`, `revoke` | List, answer and revoke camera / microphone / idle prompts and decisions. |

Some requests need a host window and answer an error in a guest session (`choose`, `drop`, `clipboard`, `resize`).

## In a guest system and in production

- In an acdev guest system the runtime ignores `ACDE_DEV`; acdes's session manager opens the channel only on development guests (recognised by the virtual machine's firmware configuration) and forwards each request to the right process. Page names there are `<app>#<instance>`, the lock page is `lock` and the login screen `greeter`. Pointer and key actions are injected by the compositor as real input, and window operations go through it too.
- In a regular Dopame OS session only `webs`, `snapshot`, `query`, `act` and `run` are answered, and only with a guard that protects elements only the person may operate (see [acde:inspect](../desktop/inspect.md#in-a-production-session)). Everything else answers `not available in a production session`.

## For developers

[dev/dev-ipc.md](../../../agents/dev/dev-ipc.md) (every request and its answer, hot reload, the dev log, guest systems and production sessions).
