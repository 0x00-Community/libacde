# The app manifest, capabilities and permissions

Synced with docs/agents on 2026-11-24

Every app is described by its package manifest: the `acde` field of `package.json`. The runtime reads nothing else to decide what an app is, how its window looks, which system modules it may import and whether it may run here at all. This page explains each field, how capabilities are granted, which of them ask the user, and what an app sees when something is missing.

## A package and its apps

A package is a folder with a `package.json`. Its `acde` field can hold apps, desktops, services and assets:

```json
"acde": {
  "apiLevel": 1,
  "apps": [{ "id": "term", "label": "Terminal", "icon": "utilities-terminal",
             "entry": "/ui/terminal/index.html", "capabilities": { "terminal": true } }],
  "assets": { "/ui/": ["dist/ui"] }
}
```

- **`assets`** maps a same-origin URL prefix to folders inside the package. The page is served at `http://acde.local/`, and a request for `/ui/x.js` is answered from the folder the prefix points to.
- **`apiLevel`** is the contract level the package was built for. The runtime accepts a range of levels (today both the lowest and the current level are 1). A package outside the range, or without `apiLevel` at all (built before levels existed), is refused: it does not appear in the app list, the desktop list or "open with", and starting it reports `needs level N, runtime is level M` (or `… supports levels L–M`). A package without a level gets a hint to rebuild it with the current acdev.
- **Where packages are found**: the runtime's own resources, then every folder in `ACDE_PATH`, then `$XDG_DATA_HOME/acde/packages/*`, then `$XDG_DATA_DIRS/acde/packages/*`. When two packages declare the same id, the first one found wins.

Desktops and services have their own fields; see [the desktop package](../desktop/desktop-package.md).

## Fields of an app

| Field | What it does |
| --- | --- |
| `id` | The app's identity: its storage partition, its private folder, its window positions and its permission decisions are all keyed by it. |
| `label` | The name people see (Dock, window title when the page sets none, prompts). |
| `icon` | A freedesktop icon name such as `accessories-calculator`, drawn from the current icon theme, or a same-origin `.svg` / `.png` URL under the assets. For one transition release, Dopame's 29 old private names (`files`, `calc` …) are still accepted with a deprecation warning that names the standard replacement. |
| `entry` | An HTML file (loaded as the document), a module whose default export is the app, or a path ending in `/` when the page is rendered by the app's backend. |
| `size` | The page size of a new window, in logical pixels, `[width, height]`; default 800 × 600. The title bar is added on top, and the whole window never exceeds the screen. |
| `frame` | `false` means the app draws its own title bar (see [windows and lifecycle](windows.md)). |
| `multiple` | The app may have more than one window; otherwise it is a single instance and a second open hands the request to the running one. |
| `heapMb` | The JavaScript heap limit of each page in MB (at least 32; smaller values are ignored with a warning). Apps that give none run with 256 MB. |
| `capabilities` | System capabilities the app wants (next section). |
| `optional` | Capabilities from that list the app can live without. |
| `services` | Names of desktop services the app calls. |
| `mimeTypes` | File types the app opens, such as `text/markdown` or `image/*`. |
| `schemes` | URL schemes whose links it opens (lowercase, no colon, never `file`). |
| `requests` | Private request kinds other apps may send it (see [opening files and links](opening.md)). |
| `backend` | `{entry, port?}`: a server that runs next to the app (see [backends and the network](../web/network.md)). |
| `window` | `{material}`: the window's default background material (see [system glass](../graphics/glass.md)). |
| `lifecycle` | `{suspendable?, endsAnytime?}`: how the app may be suspended and ended (see [windows and lifecycle](windows.md)). |
| `actions` | Declared operations that Agents and the system can call (below). |
| `sensitive` | Marks a system app as sensitive. Only a system package with a desktop may set it; how such apps are treated is decided by acdes, not by libacde. |

The rules that check a manifest live in one place, shared by the runtime and by `acdev check` / `acdev build`, so a typo is refused the same way at build time and at start-up.

## Capabilities

The `capabilities` object lists what the app needs beyond the web platform. Each name opens one door:

| Capability | Gives |
| --- | --- |
| `fs` | `acde:fs`: the app's private folder and folders lent to it; `{"roots": [...]}` also names public places (`desktop`, `documents`, `downloads`, `music`, `pictures`, `videos`) and `removable` (USB drives). See [files](files.md). |
| `terminal` | `acde:terminal`: terminal sessions. |
| `network`, `system`, `bluetooth`, `audio`, `devices`, `power`, `input`, `display` | The settings modules of the same names. See [system modules](system-modules.md). |
| `office` | `acde:office`: editing Office documents with a document server. |
| `launch`, `slots` | `acde:launch`: start native programs (`launch`) and embed them into a slot of the page (`slots`). |
| `open` | `true`, or a list of app ids: which apps this app may open by id. |
| `openFiles` | `acde:openPath` and `acde:handlers`: open a file with its default app and list the apps that can open it. |
| `camera`, `microphone` | `getUserMedia`. These ask the user first. |
| `idleDetection` | The standard `IdleDetector`. Asks the user first. |
| shell module names | Only for system apps (below). |

A few modules need no declaration at all: `acde:apps` (list and open), `acde:open`, `acde:window`, `acde:services`, `acde:actions`, `acde:capabilities`, `acde:icons` and `acde:sounds` are given to every app. Web notifications are a web capability that is never declared either.

### Who may declare what

Pages are injected by tier:

- **Third-party apps** get the app modules, plus the capabilities and services they declare. An app that declares a shell module (`windows`, `packages`, `session`, `desktops`, `inspect`, `desktop`, `setDefault`, `permissions`, `voice`, `software`) is refused at start-up (`shell_module`).
- **System apps** are apps in the same package as an installed desktop (and the community store). They may declare shell modules in addition.
- **The desktop page** gets everything; see [the desktop package](../desktop/desktop-package.md).

A module that was not given is simply not defined. Importing it fails with an `AcdeError` whose `code` is `forbidden` (status 403) and whose message names the module and the missing declaration, for example `acde:terminal needs "terminal" in the capabilities of Notes`. When the service that provides a module has stopped, the code is `service_not_running` (503).

### Services are namespaces

A desktop service exposes one or more namespaces; a page imports each as `acde:<namespace>`. Apps list the **service's name** in `services`, not the namespace. If an app imports a name that is neither a system module nor a namespace of a listed service, the error tells it to list the service that provides it. `acdev check` prints a `note:` when it cannot verify statically that a listed service really provides a namespace.

## Required and optional

Every declared capability and service is required unless it is also listed in `optional`. Before an app starts, the runtime checks each one and gives it a state:

| State | Meaning |
| --- | --- |
| `available` | Usable now. |
| `not-declared` | The manifest does not ask for it. |
| `unsupported` | This platform or running mode does not have it, or it could not be started (for example the Office bridge). |
| `service-not-running` | The desktop service is not up. |
| `shell-only` | A shell module asked for by a third-party app. |
| `not-asked`, `asking`, `denied` | Approval states (next section). |

A required capability that is neither available nor in an approval state keeps the app out of the app list and "open with", and starting it fails with a sentence that names the reason and the fix, for example `Notes needs "network" (not available on Windows host windows); list it in "optional" to run without it`. An optional capability that is missing lets the app run; importing its module then fails with the reason.

During `acdev dev`, a reload whose new manifest needs something unavailable logs that error, leaves the open windows on the previous build and keeps the session running; fixing the manifest and saving reloads as usual.

### What each platform has

The runtime has one table of what exists where, by operating system and running mode (`session` is a Dopame OS session on Linux, `desktop` a full desktop in a host window, `standalone` one app in a host window). The runtime, `acdev check --platform` and these docs all read it:

| Platform | Missing |
| --- | --- |
| Linux session | nothing |
| Linux host window | `slots` |
| Windows host window | `network`, `system`, `bluetooth`, `audio`, `devices`, `power`, `input`, `display`, `idleDetection`, `office`, `camera`, `microphone`, `slots`; a single app also lacks web notifications |
| macOS host window | `network`, `system`, `bluetooth`, `audio`, `devices`, `power`, `input`, `display`, `idleDetection`, `office`, `slots` |

Window background effects are fully supported everywhere. The Linux session and macOS rows have not been measured on real hardware.

## Approval: what asks the user

Only `camera`, `microphone` and `idleDetection` ask the user, the way a browser does. Everything else is granted by declaring it.

- The question comes when the app first needs it: the first `getUserMedia`, `IdleDetector.requestPermission()`, or `request(name)` from `acde:capabilities`. Starting the app never asks.
- System apps are never asked.
- "No", "not answered yet" and "revoked" only make those calls fail (`NotAllowedError`, or an `AcdePermissionError` with status 403 and `code` `denied`, `not_asked` or `asking`). The app still starts and stays in the list.
- Decisions are remembered per app. Revoking deletes the decision: for the rest of this run it counts as refused, and the next run asks again.
- Who answers: on a desktop, the desktop page through `acde:permissions` (it must answer, or the app waits). A single app in a host window (`acdev dev` on one app) shows a native dialog instead: a message box on Windows, a sheet on macOS, a persistent notification with Allow / Deny on Linux. Headless sessions and tools answer with `acdev allow` / `acdev deny`, and every prompt writes a log line saying so. In a single-app session an earlier "no" is asked again once per run, because the developer named this app and has no settings page to undo it.

## Asking at run time: `acde:capabilities`

Every page has `acde:capabilities` without declaring it:

- `level`, `platform`, `mode` and `window` (the window features) describe where the app runs.
- `get(name)` and `list()` return `{name, kind, state, reason, optional}` for every system capability, web capability and declared service, and stay current as approvals change.
- `request(name)` asks the user when the state is `not-asked` or `asking` (joining a prompt already on screen) and returns the state after the answer; for any other state it returns the current state without asking.

The intended pattern: `not-asked` → call `request`; `asking` → wait; `denied` → tell the user; `available` → use it.

## Errors from system modules

Failures from the settings modules (`acde:network`, `system`, `bluetooth`, `audio`, `devices`, `power`, `input`, `display`) are all an `Error` with `name === 'AcdeError'`, carrying `status`, `code` and `module`; the original error is in `cause` and its own code, if any, in `reason`. The codes are `bad_request` (400), `forbidden` (403), `unsupported` (501), `service_not_running` (503, the system service such as NetworkManager or BlueZ is not there) and `failed` (500). `acde:fs` has its own node-style codes (see [files](files.md)).

## Declared actions

`actions` lists operations the app offers to the system and to Agents, in the same shape as a plugin tool: `{name, description, inputSchema, outputSchema?, annotations?: {readOnly?, destructive?, toolRisk?}, longRunning?, target?}`. Names start with a lowercase letter and use lowercase letters, digits, `_` and `-` (at most 64), and are unique within the app; both schemas are JSON schemas of `type: "object"`; `toolRisk` is `readOnly`, `risky` or `destructive`. Because the declaration is static, the system can list an app's actions without running it. `readOnly` only counts for system packages, and `destructive` / `toolRisk` can only raise the risk level.

At run time the page registers handlers with `acde:actions`: `handle(name, (args, call) => result)`. `call` carries the caller, an optional target, an `AbortSignal` and `progress(done, text?)`. A handler returns `{ok: true, output?}` or `{error: {code, message}}`; throwing is reported as `failed`, a wrong shape as `bad_result`. Long-running actions report `pending`, then progress, then the result. Arguments are untrusted input: check them in the handler. The frames that carry calls between the session manager and the app are still being defined, so this is **in development** on the system side.

## For developers

[runtime/webs.md](../../../agents/runtime/webs.md) ("应用清单", "必需与可选能力", "平台支持表", "批准", "声明式操作"), [acde-api.md](../../../agents/acde-api.md) (the SDK and its error types), [runtime/packages.md](../../../agents/runtime/packages.md) (contract levels, where packages are found).
