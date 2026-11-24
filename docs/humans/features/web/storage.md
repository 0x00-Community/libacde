# Web storage

Synced with docs/agents on 2026-11-24

Pages keep data with the standard web storage interfaces: `localStorage`, `sessionStorage`, IndexedDB, cookies, the Cache API and the origin private file system. There is no browser profile behind them; libacde stores everything itself, in SQLite files per app, and shares it between all windows of the same app.

## Partitions: whose data is where

All pages run as threads of one process, and storage is kept by **partition**:

- one per app (`app-<id>`), shared by every window of that app;
- one per desktop (`desktop-<id>`);
- one for the login screen (`greeter`), kept in memory only.

A partition lives in `$ACDE_STORAGE/<partition>/`, by default `$XDG_DATA_HOME/acde/storage` (on Linux usually `~/.local/share/acde/storage`). Setting `ACDE_STORAGE` to an empty string keeps everything in memory: `acdev test` uses this so tests never see each other's data. The files are SQLite databases in WAL mode. A corrupt file is moved aside as `.bad-<milliseconds>` and storage starts empty; a file that cannot be opened at all falls back to memory. Both cases log a warning.

Data is written when the last page of a partition goes away and before the process exits, so nothing is lost on a normal close.

## `localStorage` and `sessionStorage`

- `localStorage` is one store per app, shared by all its windows. Keys and values are stored as UTF-16 (lone surrogates survive). The quota is 5 MiB counted in UTF-16 code units; going over makes `setItem` throw `QuotaExceededError` and change nothing.
- When another window of the same app changes a value, this window receives a `storage` event (the window that made the change does not). Named properties behave as WebIDL says: `localStorage.foo = 'x'` stores, and names that exist on the prototype (`key`, `length`) are never treated as items.
- `sessionStorage` belongs to one page, lives in memory and has the same quota. `location.reload()` restarts the page and does not keep it, which differs from the standard; this is a known gap.
- Older versions kept `localStorage` in JSON files under `$XDG_CONFIG_HOME/acde/storage/`; these are imported once when the new database is created, and renamed `.json.imported`.

## IndexedDB

IndexedDB is complete enough to run promise wrappers such as `idb` exactly as in a browser:

- Each database is one SQLite file. Transactions are scheduled as the standard requires: overlapping scopes with a writer run in creation order, read-only transactions never see uncommitted writes, and each write is its own savepoint, so one failing request only undoes itself.
- Opening with a newer version sends `versionchange` to the other connections, reports `blocked` while they stay open, and upgrades once they close. Deleting a database works the same way.
- Keys sort as the standard says (numbers < dates < strings < binary < arrays). Key paths, generated keys, multi-entry indexes, unique indexes (a violated unique index aborts the upgrade), cursors and `IDBKeyRange` behave as in Chromium, with the same exception order and messages.
- Transactions become inactive and auto-commit at the same moments as in a browser (after each event listener's microtasks, and at the end of the task), so code that `await`s between requests behaves the same as in Chrome.
- Values are structured clones. `Blob`, `File`, `FileList`, `ImageData`, `DOMPoint`, `DOMRect`, `DOMQuad` and `DOMMatrix` can be stored; DOM nodes, `window` and events cannot (`DataCloneError`).
- The Web Platform Tests for IndexedDB pass 1332 of 1384 subtests on macOS; the failures come from missing workers in tests, iframes, bfcache, `SharedArrayBuffer` and tests that need the WPT server.

## Cookies

`document.cookie` and the `Cookie` / `Set-Cookie` headers of same-origin requests (the app's own backend) share one cookie jar per app. Cookies with `Expires` or `Max-Age` are saved; session cookies last until the runtime exits. Scripts never see `HttpOnly` cookies. Every other host is a third party to `acde.local`, so cross-origin requests neither send nor store cookies, as Safari's third-party cookie blocking does.

## The Cache API

`caches`, `CacheStorage` and `Cache` store requests and responses (with headers and the full body) in the partition. Matching follows the standard: the URL fragment is ignored, `ignoreSearch` and `ignoreMethod` work, and `Vary` compares the named request headers (`Vary: *` never matches). `put`, `add`, `addAll` and `delete` are atomic; duplicate requests inside one `addAll` are an `InvalidStateError`, and 206 responses are refused. `caches.delete(name)` hides the cache at once; `Cache` objects already open keep working until the partition is next opened. Calls from one page run in the order they were made.

## `navigator.storage` and the private file system

- `estimate()` reports usage as the total size of the partition's files, and quota as 60 % of the disk, as Chromium does.
- `persist()` and `persisted()` are always `true`: Dopame OS never evicts an app's storage.
- `getDirectory()` returns the origin private file system, a read-write `FileSystemDirectoryHandle` named `''` stored under `<partition>/opfs/`. For in-memory partitions it is a fresh private folder (mode 0700) under the runtime directory, deleted when the process exits. `createSyncAccessHandle` is not available (it only exists in workers, which do not have it here).
- Quotas are not enforced on writes: IndexedDB and the private file system never throw `QuotaExceededError`. This is a difference from browsers, not a security boundary.

## For developers

[web/storage.md](../../../agents/web/storage.md) (partitions, the IndexedDB scheduler, key encoding, tests).
