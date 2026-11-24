# Opening files and links, default apps and requests between apps

Synced with docs/agents on 2026-11-24

When the user double-clicks a document, follows a link, or one app asks another to show something, the runtime decides which app opens it and hands it over. All of these go through one opener: the same rules choose the app whether the request comes from the desktop, from an app, from a native program's `xdg-open`, or from the command line.

## Ways in

- **From the desktop or an app**: `acde:openPath(path)` (desktop and apps with `openFiles`) and `acde:open(request)` for typed requests.
- **From the command line**: `<runtime> open [--app <id>] <path|URI>...` opens in the running session (exit code 3 when no session runs).
- **From native programs**: `xdg-open` reaches acde apps through desktop entries the runtime writes (`<runtime> applications <dir>` writes `acde-<app>.desktop` files and an `acde-mimeapps.list`), and file managers' "show folder" calls arrive through `org.freedesktop.FileManager1` on the session bus.

## Choosing the app

1. **The type**: folders are `inode/directory`, links are `x-scheme-handler/<scheme>`, files get their type from the host system (below).
2. **The user's choice**, if one was remembered for that type (XDG `mimeapps.list` files, with the runtime's own `acde-mimeapps.list` read first).
3. **The first app whose manifest lists the type** in `mimeTypes` (exact types before `type/*`) or the scheme in `schemes`.
4. **A native program** registered for the type.
5. On macOS and Windows host windows, **the system's default program** (below).
6. Otherwise the answer is `no_app` (404). The runtime shows nothing itself: the desktop draws its own "open with" dialog using `acde:handlers` and `acde:setDefault`.

`acde:handlers(path)` lists the apps that can open a file, with `{mime, association, handlers}`; `association` is the key to pass to `acde:setDefault(association, app)` to remember the user's choice. On Linux the choice is written to `$XDG_CONFIG_HOME/acde-mimeapps.list`.

Apps that cannot run on this platform are never chosen. In a host window there are no slots for embedded native windows, so an app that needs `slots` (such as Dopame's browser) is left out of the defaults, the handlers, the app list and the Dock, and opening it by id answers `{status: 404, code: 'unsupported_app'}`.

### File types

The runtime ships no extension table of its own and never guesses from browser-oriented databases. Unknown is unknown (`''`), and the user picks the app.

- **Linux and macOS**: ask `gio info` (Linux); if that gives no MIME type, match the name against shared-mime-info's globs (from the XDG data folders, and Homebrew or MacPorts on macOS), using `file --mime-type` to break ties; finally `file` alone, else `application/octet-stream`. The result is the same on a macOS host window as on Dopame OS (`.md` is `text/markdown`). Without the shared-mime-info database a warning suggests installing it (`brew install shared-mime-info` on macOS).
- **Windows**: the `Content Type` the system registered for the last extension (`.csv` may be `application/vnd.ms-excel`, `.zip` is `application/x-zip-compressed`); extensions without one (often `.md`, `.py`, `.yaml`) are unknown.

### Windows: "open with"

On Windows the native programs for a file are the handlers the system recommends for its extension, the same list as Explorer's "Open with", each with its name and icon; the system's own default is marked `system: true` and is never changed. When a type is unknown, the list shows every acde app that declares any file type and the extension's native programs. The user's choices are remembered by extension in `$XDG_CONFIG_HOME/acde-extensions.list`. Opening with a chosen native program checks the path (absolute, a regular file, no `..`, no device paths or alternate streams) and that the program is still registered for the extension. Only the desktop, the `open` command and system apps that named public places may name a native program; other apps get 403 `refused`.

### macOS and Windows: handing over to the system

When nothing in the session can open a target, a host window on macOS or Windows hands it to the system's default program, and only two kinds of target qualify:

- **http and https links**, under 8 KiB, visible ASCII only, no quotes, backslashes, angle brackets, back-ticks or spaces, with a host. Other schemes (`file:`, `javascript:`, `mailto:` …) are never handed over.
- **Files** that resolve inside one of the caller's roots after following links, are regular files, and have an extension on a short list of document types: PDF, images, audio and video, plain text, Markdown, CSV, JSON, modern Office (`.docx`, `.xlsx`, `.pptx`) and OpenDocument. Executables, scripts, shortcuts and macro-enabled documents are never handed over, and neither are legacy Office files, HTML, SVG or RTF.

A refusal or failure answers `no_app` with the reason. Headless sessions run all the checks and only log what they would have opened. Linux does not hand over at all.

There is no user-gesture requirement yet: apps with `openFiles` can open things without a click, because the runtime has no user activation model for this path. Once it has, app-initiated handovers will need a recent activation.

## Requests to apps

Every app window can receive requests through `acde:window`'s `request` event; the first handler gets the request that opened the window, once.

- **System requests**: `{kind: 'open-link', url}` and `{kind: 'open-file', ...}`. An app without `fs` gets `{kind: 'open-file', url}`; an app with `fs` gets the file's root, path and a handle; a third-party app with only `fs` gets just the handle. The receiving window is raised.
- **`acde:open(request)` without a target**: the runtime picks the app: links go to an app whose `schemes` lists the scheme, then the default for that scheme, then native programs, and http / https finally to the system's browser; files go by `mimeTypes` and defaults. The caller needs `openFiles` (the desktop does not).
- **`acde:open(appId, request)` with a target**: the caller's `open` capability must allow that app (`true`, or a list containing it), and the target must accept the request: a link's scheme in its `schemes`, a file's type in its `mimeTypes`; otherwise 409 `not_accepted`. A request without `kind` is 400 `untyped_request`.
- **Private requests**: any other `kind` (lowercase words, not `open-file` / `open-link`) is a private request. The target lists the kinds it accepts in `requests`; the runtime only checks that the kind is listed and delivers the content untouched, so the receiver validates it. The SDK's `AppRequest` type lets a receiver declare its kinds and fields, and handlers narrow by `kind`.
- **Window options**: `{parent, modal}` opens the new window centred above a parent (apps can only name their own windows; the desktop any window); `{newWindow: true}` asks for another window of a running app (only apps with `multiple` really get one), and `running: true` in the answer says the app was already running. A single-instance app that is already running only receives the request.

## For developers

[web/default-apps.md](../../../agents/web/default-apps.md) (the opener, file types, Windows associations, handing over to the system), [runtime/webs.md](../../../agents/runtime/webs.md) (system and private requests), [web/fs.md](../../../agents/web/fs.md) (handles in requests).
