# Files: `acde:fs`, pickers and drag and drop

Synced with docs/agents on 2026-11-24

Apps reach the user's files in two ways. The standard way is the one browsers use: file pickers, drag and drop, and `FileSystemHandle`s, where the page never learns a path. The system way is `acde:fs`, which works on **roots**: named places the app was given, addressed by an opaque id plus a relative path. No app other than the desktop ever handles absolute paths, and every operation is confined to its root even if someone swaps folders or links underneath.

## Roots

`acde:fs` needs the `fs` capability. Every call names a root and a relative path inside it, separated by `/`. `roots()` lists what the app has:

| Root (`source`) | What it is | How an app gets it |
| --- | --- | --- |
| `private` | The app's own data folder, `$XDG_DATA_HOME/acde/files/<app id>`, created on first use. | `"fs": true` or any `fs` declaration. |
| `place` | A public place: `desktop`, `documents`, `downloads`, `music`, `pictures`, `videos`, following the XDG user folders (`user-dirs.dirs`) and the known folders on Windows. | Named in `"fs": {"roots": [...]}`. |
| `lent` | A folder the user chose (named `work`, `work-2` …), or a folder the system lent for one launch. | The user picks it, or the desktop opens the app with it. |
| `removable` | A mounted USB stick or external drive, id `removable-<volume UUID>` (stable across re-plugging). | `removable` in `fs.roots`. Linux only. |

Each entry of `roots()` is `{id, label, source, mode, reason, exists, place?}`; `mode` is `writable`, `read_only` or `unavailable`. Only system apps that named public places also see a `path`. Labels follow the page's language ("Documents", "文档" …). When roots change (a drive is plugged in or ejected, a folder is lent or taken back) the module fires a `roots` event with `{added?, removed}`, and the page should call `roots()` again.

A root that was not given, or a place the user took back, is not listed, and calls on it are refused with 403 (`EACCES`, or `denied`). An app can give back a lent folder itself with `fs.revoke(name)`.

### Which drives count as removable

A mount counts when it sits directly under `/run/media/$USER/` or `/media/$USER/` (where UDisks2 mounts removable drives for the user) and comes from a block device in `/dev/`. Manual mounts elsewhere, deeper mounts, network and FUSE mounts do not. The root's `drive` field matches the drive's id in `acde:devices`, so an app can eject the drive it is showing. A read-only mount gives a `read_only` root.

## Operations

| Call | What it does |
| --- | --- |
| `list(root, path, hidden?)` | Lists a folder, folders first. Hidden items (dot names, and on Windows hidden or system attributes) are left out unless `hidden` is true. Names that are not valid text are skipped and counted in `skipped`. |
| `stat(root, path)` | One entry; regular files carry a `version`. A dangling link is reported as a link, not as missing. |
| `readText` / `readBytes` | Whole file up to 64 MiB, with `modifiedMs`, `size` and `version`. `readText` refuses non-UTF-8 with `EILSEQ`. |
| `writeFile(root, path, data, {create, ifModifiedMs, ifVersion, mkdirs})` | Writes a whole file up to 64 MiB. `create` is `always`, `new` or `never`. `ifVersion` / `ifModifiedMs` make the write fail with `ECONFLICT` if someone else changed the file since it was read. |
| `mkdir(root, path, {recursive})` | Creates a folder. |
| `copy(fromRoot, fromPath, toRoot, toDir)` | Copies a file or folder between two of the app's roots, streaming, any size. A taken name is numbered (`a 2.txt`, `project 2`). |
| `rename`, `delete` | Rename never overwrites an existing name; deleting a non-empty folder is `ENOTEMPTY`. |
| `preview` | The start of a text file, up to 256 KiB. |
| `download(root, path)` | `{name, contentType, data}`, typed by name the same way as `File.type`. |
| `watch(root, path, handler)` | Watches one folder level (Linux only; elsewhere it rejects with 501 and the app keeps polling). |
| `locate`, `openPath` | Paths and opening; only the desktop and system apps with public places get paths. |

**Safe saving**: replacing a file writes a temporary file in the same folder, flushes it, and renames it over the original, so a crash leaves either the old or the new file. Permissions and owner are kept on Unix; hidden / read-only attributes and creation time are kept on Windows. If the folder does not allow new files, the write falls back to writing in place and the result says `inPlace: true` (without crash safety).

**Versions**: `version` is an opaque string built from size, exact modification time and file identity. Compare it for equality only. It tells apart two writes in the same millisecond, FAT's 2-second clocks and a changed system clock.

**Watching**: `watch` resolves to an unsubscribe function. The handler receives batches `{root, path, changes}`, where each change is `appeared`, `disappeared`, `modified` (a write finished and the file was closed), `moved` (a rename inside the folder, with `from`) or `unknown` (the kernel's queue overflowed; list again). If the watched folder itself is deleted or moved, one `disappeared` change names it. The type names follow WICG's `FileSystemObserver`, but the module does not use that interface because roots are not handles.

### Errors

Failures reject with an `Error` carrying `message`, `status` and a node-style `code`:

| `code` | `status` | When |
| --- | --- | --- |
| `EINVAL` | 400 | Bad arguments, leaving the root, wrong type, a symbolic link |
| `EACCES` | 403 | The root was not given, or the system refuses |
| `ENOENT` | 404 | Does not exist |
| `EEXIST` | 409 | Already exists |
| `ECONFLICT` | 409 | Changed or gone since it was read (`ifVersion`, `ifModifiedMs`) |
| `ENOTEMPTY` | 409 | Deleting a folder that is not empty |
| `EFBIG` | 413 | Over a size limit |
| `EILSEQ` | 400 | `readText` on bytes that are not UTF-8 |
| `ENOSPC` | 507 | The disk is full |
| `EIO` | 500 | Anything else (details in the log) |

### Staying inside the root

Every call first opens the root's folder and then works relative to that handle. On Linux intermediate folders are resolved with `openat2(RESOLVE_BENEATH)`; elsewhere the same rule is emulated. Relative links inside the root work; absolute links and `..` that leave it are `EINVAL`, and swapping a folder between the check and the use cannot escape. The last component is never followed if it is a link. On Windows, backslashes, stream separators, reserved device names and names ending in a space or dot are refused.

## Large files

`readText`, `readBytes` and `writeFile` stop at 64 MiB. Larger files go through the standard interfaces:

- `getFile()` on a handle returns a `File` that reads on demand: `slice()` reads nothing, `stream()` reads 4 MiB at a time, `text()` and `arrayBuffer()` read it all. If the file changes after it was opened, reading fails with `NotReadableError`, as in Chromium. Such a `File` can be sent as a request body, a form part, a `blob:` URL or a WebSocket message without being read into memory first. Only operations that need the bytes synchronously (`structuredClone`, `postMessage`, storing in IndexedDB, synchronous XHR) still fail on it.
- `createWritable()` writes into a temporary file next to the target and replaces the original atomically on `close()`; `abort()` throws the temporary file away. Writing past the end fills with zeros, as the standard says.
- `acde:fs` has the same building blocks: `readRange` (up to 16 MiB per call), and `openWrite` / `writeChunk` / `truncateWrite` / `commitWrite` / `abortWrite`. A page may have 16 writes open; a write idle for 10 minutes is abandoned; a write that would leave less than 256 MiB free fails with `ENOSPC`; leftovers of a killed process are cleaned up the next time someone writes in that folder.

## Pickers

Standard pickers work, and the page never sees a path: the chosen file or folder is opened by the system and handed over as a handle.

- **`showDirectoryPicker()`** lends the chosen folder to the app. It also appears in `roots()` as a `lent` root, read-only unless the page asked for `mode: 'readwrite'`. In a Dopame OS session the session manager opens the picker and remembers the folder for next time; in a host window the platform dialog is used (the desktop portal on Linux, an `NSOpenPanel` sheet on macOS, the folder browser on Windows) and the grant lasts for this run. Cancelling rejects with `AbortError`.
- **`showOpenFilePicker()` and `<input type=file>`** hand over read-only files. **`showSaveFilePicker()`** hands over one writable file; the original is untouched until the page commits its write. The dialog titles read `<app name>: Open` / `Save` in the page's language. Filters come from the page's `types` / `accept`: on macOS they become file types (a type that cannot be mapped lifts the filter rather than hide files the page accepts); on Windows extensions are used and MIME-only filters fall back to "All files".
- Files handed over by the system are written in place through their descriptor (no atomic replace), so their owner, permissions and ACLs are kept, at the price that a crash during a write can damage the file.
- `File.type` comes from the host system's file type registry, the same one used to pick default apps (see [opening files and links](opening.md)); an unknown type is `''`, as in browsers.

During development, `acdev choose` answers the next picker in advance, so tests and headless sessions never need a dialog.

## Drag and drop

Dropping files or folders from another window (a file manager, another app) onto a page delivers:

- `dataTransfer.files` with every regular file up to 64 MiB;
- one `kind: 'file'` item per file and folder, whose `getAsFileSystemHandle()` returns a **read-only** handle (writing, creating and deleting are `NotAllowedError`). Folders and larger files are only reachable through the handle.

The drop is lent to the app first, so `drop` arrives slightly after the pointer is released. Paths never reach the page: `text/uri-list` and `text/plain` entries with local paths are removed, and a drop grants nothing beyond the handles. Pages can start drags of their own with standard HTML drag and drop, also into other windows.

## Handing files to other apps

An app can pass a handle it holds to another app: `acde:apps.open({kind: 'open-file' | 'open-folder', handle})`. The session manager checks that the handle really was lent to the sender, reopens the file itself and lends it to the receiver, which gets a handle of its own; a read-only handle stays read-only. Files in the app's private folder cannot be passed on. This needs the session manager, so a single app in a host window cannot hand files over.

## For developers

[web/fs.md](../../../agents/web/fs.md) (roots, lent folders, removable drives, handles, watching, errors, large files, pickers, platform dialogs).
