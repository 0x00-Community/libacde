# System modules: terminals, Office, settings and native programs

Synced with docs/agents on 2026-11-24

Some things an operating system's own apps need have no web interface: a terminal, Wi-Fi, Bluetooth, sound devices, power settings, displays. libacde offers them as `acde:` modules. Each one needs a capability in the manifest (see [the manifest](manifest.md)), is injected straight into the page's isolate, and is mostly meant for the system's own apps, such as a terminal or a settings app. Most of them exist only on Linux; the [platform table](manifest.md#what-each-platform-has) lists what is missing where.

All of them follow the same patterns: a `state()` that returns everything at once, setters that return promises, an `on('changed', handler)` that delivers the whole new state and returns an unsubscribe function, and failures as an `AcdeError` with a `code` (see [errors from system modules](manifest.md#errors-from-system-modules)).

## `acde:terminal` (capability `terminal`)

- `open(cols, rows, options?)` starts a terminal session and returns its id; `subscribe(id, callback)` delivers batches of events (`output`, `cwd`, `exit`, the foreground process); `input(id, data)`, `resize(id, cols, rows)` (answers the size actually used) and `close(id)` control it; `shells()` lists the available shells and `limits()` says how many sessions may be open.
- The front end may provide the terminal implementation (acdes does, as part of its session); its rules decide which `command`, `cwd` and `env` options are accepted, and a refusal arrives with a `code` naming the field.
- The built-in fallback starts the user's login shell (`-l`) in the home folder: on Unix the shell from the account database, with every inherited file descriptor closed, and the working folder reported every second on Linux. On Windows it uses ConPTY and picks Windows Terminal's default profile, then `pwsh.exe`, then Windows PowerShell, then `%ComSpec%`; the whole process tree lives in a job object, so closing the terminal (and leaving the runtime) ends everything the shell left running. The fallback accepts no options and allows 8 sessions. It is meant to be removed once acdes's implementation has replaced it, after which host windows will have no terminal.

## `acde:office` (capability `office`, Linux)

Edits Word, Excel and PowerPoint documents in an ONLYOFFICE Document Server editor.

- `server()` and `state()` report whether the document server is usable; `server.problem` names what is wrong (`unreachable`, `missing` (no local server and no docker / podman to start one), `unhealthy`, `secret` (no shared JWT secret), `rejected`, `bridge`, `runtime`).
- `state(root, dir, path)` or `state(handle)` opens a document from one of the app's file roots or from a file the user lent; `editorPage(...)` builds the editor page; `save({key})` asks the editor to save; `new({root, kind, dir?, name?})` creates an empty `docx`, `xlsx` or `pptx`.
- The roots are the same as the app's `acde:fs` roots, so an app without `fs` has none. There is no way to open a document by absolute path. Documents from a read-only lend can only be viewed.
- Saving checks the modification time: in a root, a file changed by someone else is saved as a conflict copy; a lent file is not overwritten and the save fails. If the lend is taken back, open sessions can no longer read or write it (the save fails with 410). After a runtime restart, documents in roots reopen from the editor's signed ticket; lent files cannot, and unsaved edits must be saved elsewhere.
- One Office bridge runs per session; a second process that declares `office` at the same time cannot start its own.

## `acde:network` (capability `network`, Linux)

NetworkManager, connected on first use: `state()` (devices, connections, whether Wi-Fi is on), `networks(scan?)` (visible Wi-Fi networks, optionally scanning first), `connect(ssid, password?)`, `activate(iface)`, `disconnect(iface)`, `setWifiEnabled(on)`, and `on('changed')`.

## `acde:system` (capability `system`, Linux)

- `info()`: host name, operating system, CPU, GPU, NPU, memory, uptime and serial number. `storage()`: the volumes and their capacity. `time()` and `timezones()`: time zone and NTP, through timedated. `user()`: the current user.
- `setTimezone(zone)`, `setNtp(on)` and `setHostname(name)` change them through timedated and hostnamed, which ask polkit for authorisation.
- `on('changed', handler)` delivers `{storage, hostname, timezone, ntp}`. While someone listens, the page checks every 30 seconds and right after its own setters succeed, and only reports real changes.

## `acde:bluetooth` (capability `bluetooth`, Linux)

BlueZ adapters and devices: power, discoverability (with a timeout), the adapter's name, scanning, pairing (the module registers its own pairing agent; `on('pairing')` asks the page, and `answer(id, accept, pin?)` replies), connecting, trusting, blocking, renaming and forgetting devices. `setOnLogin('remember' | 'on' | 'off')` decides whether Bluetooth is switched on at login; only the user's own session applies it.

## `acde:audio` (capability `audio`, Linux)

PipeWire's output and input devices, their ports and the cards' profiles, and each app's streams: `setDefault`, `setVolume`, `setMuted`, `setBalance`, `setPort`, `setProfile`, and `moveStream(id, device)` to send one app's sound to another device. Playing sound itself needs no capability (see [sound and video](../web/media-and-devices.md)).

## `acde:devices` (capability `devices`, Linux)

- **Drives** through UDisks2: `mount`, `unmount`, `eject`, and `on('added')` when a drive is plugged in. Automatic mounting is a setting: mount on login, on attach, and whether unknown devices are mounted (by default drives mount when attached, unknown ones too, and not at login), with per-device overrides and `forgetDevice`. The settings live in `~/.config/acde/automount.json`, and only the user's own session runs the automounter.
- **Printers** through CUPS (IPP): set the default printer, pause and resume, list jobs and cancel one.

## `acde:power` (capability `power`, Linux)

- `state()` reports the battery (UPower), the power profile (power-profiles-daemon), the policy settings grouped by mains / battery / low battery (idle action, screen off, lid, power button, brightness), the backlight and the charge limit.
- `setProfile`, `configure(patch)`, `setScreenOff`, `setSleep`, `setBrightness`, `setChargeLimit`, `suspend()` and `hibernate()` (through logind).
- The **policy** itself (what the power button, the lid, idleness and a low battery do) is not decided here: acdes's session runs it, using these settings. Settings are saved in `$XDG_CONFIG_HOME/acde/power.json` and every process watches that file, so a change in the settings app takes effect at once. `state().policy` says whether a policy process is running.
- When the policy needs the user (`prompt`, or `critical` with the action and seconds left), the event goes to the desktop page through this module.

## `acde:input` (capability `input`)

- The keyboard layout, model and options (xkb), key repeat, NumLock, and mouse and touchpad settings (libinput), through `state()`, `set(patch)` and `on('changed')`. These need the window system's help: an acdes session provides them; a host window does not, and calls fail there.
- `catalog()` lists the layouts, models and options known to xkeyboard-config.
- **Input methods** (fcitx5): `state().ime` has `{running, current, group}`, `set({ime: {current?, group?}})` switches the current input method or changes the group, and `catalog().imes` lists the available ones. Changes made elsewhere are reported too. When fcitx5 is not running (the login screen, no session bus) the input method part is empty and writing to it fails; the rest works.

## `acde:display` (capability `display`, Linux)

- `state()` lists the outputs as the window system reports them, each with its `bounds` in desktop coordinates and its `workArea`; the built-in screen also has its backlight `brightness`.
- `configure(id, {scale})` changes the scale (in an acdes session all screens share one scale); `setBrightness(id, value)` works on the built-in screen only. `setNightLight` is not implemented yet.

## `acde:launch` (capabilities `launch` and `slots`)

- `start(argv)` starts a native program; its windows appear in the window list like any other.
- `embed(...)` puts a native program's window into a slot inside the page. This only works in a Linux session; host windows have no slots, and acdes does not support it yet either (501).
- `close(...)` only closes windows the app itself started or embedded.

## For developers

[runtime/architecture.md](../../../agents/runtime/architecture.md) (the injected capabilities, the terminal seam, power, input, display, Office), [acde-api.md](../../../agents/acde-api.md) (errors and events of these modules), the SDK types in `acde-api/src/types/`.
