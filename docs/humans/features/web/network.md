# App backends and the network

Synced with docs/agents on 2026-11-24

A page can talk to three kinds of places: files of its own package, its own backend (a server that runs next to the app), and the rest of the network. `fetch`, `XMLHttpRequest`, `WebSocket` and `EventSource` reach all three with browser rules, from the page's own thread. The backend never listens on a TCP port: the runtime connects to it through a private socket, so other programs on the machine cannot reach it.

## The app backend

An app declares a backend in its manifest: `"backend": {"entry": "server.js", "port": 3000}` (the port defaults to 3000). The built package contains `backend/server.js` and a small preload, `acde-listen.js`, that acdev's build adds.

- **What runs it**: Bun if it is on `PATH` (`bun --preload acde-listen.js server.js`), otherwise Node 22.3 or newer (`node --import <preload> server.js`). With `ACDE_RUNTIME=node` (which `acdev dev` sets from the project) Node is tried first. The same build runs on both. If neither is installed, the backend does not start and a warning says so.
- **No ports**: the preload replaces every `listen(P)` (and `Bun.serve`) with a Unix socket named `P.sock` in a private per-process folder, so the server code keeps writing `listen(3000)`. A fixed-port UDP bind is refused. Ports opened by native extensions are not managed.
- **When it runs**: it starts with the app and stops when the app exits. A reload (`acdev dev` rebuilding, or `location.reload()`) stops and restarts it: SIGTERM to its process group, SIGKILL after 3 seconds.
- **When it crashes**: it is restarted after half a second with a warning `[<app>#backend] stopped (exit N); starting it again`. After the fourth crash within a minute it stays down until the next reload.
- **Its environment** is built from scratch: `ACDE_APP_ID`, `ACDE_APP_DATA`, `ACDE_APP_SOCKETS`, `PORT`, `NODE_ENV=production`, plus the session's `HOME`, `PATH`, `LANG` and the like. Its working folder is `$XDG_DATA_HOME/acde/apps/<id>/`. It runs in its own session and process group and dies with the runtime.
- **Its output** appears in the log prefixed `[<app>#backend]`, and `acdev dev` shows it.
- **Proxies**: if the session sets any of `http_proxy`, `https_proxy` or `all_proxy`, those variables and `no_proxy` are passed through unchanged. Otherwise the backend gets the system's manual proxy as variables: the user's Internet settings on Windows, the system configuration on macOS (Linux has no system source). `no_proxy` always includes `localhost,127.0.0.1,::1`. Proxy scripts and auto-discovery cannot be written as variables, so a backend does not get them.

### Windows

Windows has its own transport, chosen to keep the same promise (no TCP listener, no named pipe anyone could open):

- With **Bun**, the backend listens on real AF_UNIX sockets in a fresh short folder per start, under the user's temporary folder, readable only by the user and SYSTEM. A new folder each time avoids sockets that a killed Bun left behind.
- With **Node**, which cannot use file-system Unix sockets on Windows, the runtime and the preload talk over two anonymous pipes with a small framed protocol; every connection is a stream on it.
- Each backend runs in its own job object, so the backend and everything it spawned end together. On stop the runtime asks it to shut down gracefully (stop accepting, finish requests in flight) and ends the job after 3 seconds.
- A backend built by an older acdev, whose preload does not speak the pipe protocol, is not started under Node; the warning says to rebuild with acdev.

## Where a request goes

| Request | Goes to |
| --- | --- |
| Same origin (`http://acde.local/...`), `GET` / `HEAD`, a file that exists in the package or in a folder lent to the app | The file, read directly. Symbolic links that lead outside the folder are treated as missing. |
| Same origin, anything else; or `localhost:<port>` (also `127.0.0.1`, `[::1]`, `0.0.0.0`) when the app has a backend | The app's own backend socket. If it is not up yet, the request retries for 10 seconds. |
| Same origin without a backend | Fails, with a message that the app has neither this file nor a backend. |
| Any other host (`http`, `https`, `ws`, `wss`) | The network: system DNS, TCP, TLS with certificate and host name checks (HTTP/1.1 only). Responses are not decompressed automatically. |

### Proxies for pages

If `http_proxy`, `https_proxy` or `all_proxy` is set (either case, lowercase first), only the variables are used, on every platform. Otherwise the runtime asks the platform: Windows Internet settings (including WPAD and configuration scripts), macOS system configuration (including PAC scripts, up to 10 seconds), and nothing on Linux. `no_proxy` (names with subdomains, IPs, CIDR, ports, `*`) applies to system settings too. Localhost, loopback addresses and the app's backend are always direct. HTTP proxies are used through `CONNECT` tunnels, SOCKS4a and SOCKS5 are supported, and when a PAC answer lists several proxies they are tried in order with every failure reported. Proxy authentication, `https://` proxies and Linux desktop-environment proxy settings are not supported.

## Browser rules

The page's origin is `http://acde.local`, and requests follow the same rules as in a browser:

- Cross-origin requests carry `Origin`, `Referer`, `User-Agent` and `Accept-Language`. CORS is enforced: anything beyond a simple request sends a preflight `OPTIONS` first (not cached), and a response that does not allow this origin is a network error (`TypeError` whose `cause` starts with `CORS:`). Only safelisted and exposed response headers are visible.
- `mode: 'no-cors'` allows only simple methods and headers and returns an opaque response; `mode: 'same-origin'` refuses other origins. Redirects are followed with CORS checked at each step.
- Cross-origin requests neither send nor store cookies (see [storage](storage.md)). Cookies with the app's own backend work as usual.
- Forbidden headers (`Host`, `Cookie`, `Origin`, `Referer`, `Accept-Encoding`, `Sec-*`, `Proxy-*` …) are dropped if a page sets them. Synchronous XHR only reaches package files.
- Bodies are read as the Fetch standard says (a read body is locked; a null body is never "used"). A `Request` whose URL contains a user name or password throws.
- Classic `<script src>` from another host runs without CORS, as in a browser.
- Large files picked or dropped by the user can be sent as request bodies, multipart form parts and WebSocket messages without the page reading them first (see [files](../apps/files.md)).

## Seeing requests

During development every request is recorded with its method, URL, status, sizes, time and error, and `acdev network` lists them. A request that cannot be served says why: no such file and no backend, or not an address of this app. Tools can also answer requests with fake responses (the `route` tool of acdev's MCP server and test driver), see [the dev channel](../dev/dev-channel.md).

## For developers

[web/backend.md](../../../agents/web/backend.md) (starting backends, the Windows contract and frame format, routing, proxies, CORS), [runtime/layout.md](../../../agents/runtime/layout.md) ("应用后端").
