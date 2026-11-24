# Running the runtime: commands, settings, logs and embedding

Synced with docs/agents on 2026-11-24

libacde is a library; the programs that run it are its two front ends, acdev's runtime (host windows on Linux, Windows and macOS) and acdes (Dopame OS sessions on Linux). Both share the same command-line subcommands, environment variables, logging and timing, which this page describes. `<runtime>` stands for either program.

## Ways to run

| Way | Command | Platforms | Output | Compositor |
| --- | --- | --- | --- | --- |
| Session (Dopame OS) | `acdes`, started by the login screen | Linux | the screen, or a remote desktop | acdes's compositor |
| Desktop in a window (`acdev dev`) | `<runtime> desktop <id|folder>` | Linux, Windows, macOS | one host window holding the desktop and all its windows | acdev's host window |
| One app in a window | `<runtime> app <id|folder>` | same | one host window filled by the app; closing it ends the run | same |

In host windows, features the host does not have are left out rather than faked: no input method daemon, no accessibility bus, no autostart, no session bus names, and capabilities from the [platform table](../apps/manifest.md#what-each-platform-has) are missing. A single app (`app`) still loads the desktop package's services and assets (`--desktop` picks which), but features that need a desktop page to publish something have no publisher. When it declares `fs`, it only gets its private folder, plus the public places of an isolated test folder that `acdev dev` prepares.

## Commands

```sh
<runtime>                                    # session: desktop chosen as described in "the desktop package"
<runtime> --desktop dopame                   # that desktop (id or folder)
<runtime> --greeter                          # the login screen (greetd)
<runtime> desktop <id|folder>                # a desktop in a host window
<runtime> app <id|folder>                    # one app in a host window
<runtime> lock <id|folder>                   # a desktop's lock page alone, with a stand-in acde:lock
<runtime> desktops                           # installed desktops and other sessions
<runtime> --version                          # program version and contract level
<runtime> doctor [--json]                    # self-check: files, GPU (draws a real frame), Linux display
<runtime> sessions DIR --package PKG         # write acdert-<id>.desktop entries for display managers
<runtime> applications DIR --package PKG     # desktop entries and mimeapps for apps with file types
<runtime> open [--app ID] PATH|URI...        # open in the running session
<runtime> run FILE [ARGS...]                 # run one module in an isolate
```

`--size 1920x1080 --scale 1` sets the screen before an output is known, and `--launch <program> --seconds N --stats` starts a program, stops after N seconds and prints statistics. An unknown option prints the reason and usage and exits with 2; `doctor` exits with 1 when it finds errors.

## Environment variables

| Variable | Effect |
| --- | --- |
| `ACDE_PATH` | Extra package folders searched first (`;`-separated on Windows). |
| `ACDE_DESKTOP` | The desktop to start. |
| `ACDE_STORAGE` | Root of web storage; empty keeps storage in memory. |
| `ACDE_COLOR_SCHEME` | `light` or `dark`, overriding the system. |
| `ACDE_SIZE` | Screen size of headless and remote outputs (default 1440x900). |
| `ACDE_DEV` | Development mode in host windows: the dev channel, DevTools, live edit. |
| `ACDE_GPU_ALLOW_CPU=1` | Allow CPU-type GPU devices (development and measurement only). |
| `ACDE_HUD=1` | Start a host window with the frame-rate overlay. |
| `ACDE_WEB_FPS` | Fix the page frame rate. |
| `ACDE_AUDIO=null` | Render audio without a device. |
| `ACDE_CAPTURE=fake` | Fake camera and microphone. |
| `ACDE_PROFILE_DIR`, `ACDE_PROFILE_SECONDS` | Write V8 CPU profiles of every isolate (sampled every 250 µs) to a folder every N seconds (default 30) and at exit. |
| `ACDE_AUTOSTART`, `ACDE_BUS_NAMES` | Force XDG autostart and session bus names on or off. |
| `ACDE_BACKEND=rdp` and `ACDE_RDP_*` | Serve the session over a remote desktop (acdes). |

## Logs

- Every line is `<milliseconds since start> <level> <thread> <target>: <text>`, on standard error, written by a background thread so logging never blocks. `RUST_LOG` adds filters to the defaults. Pages' `console.*` appear under the target `js`.
- Page errors start with the page name (`[files#1048576] page error: …`), including where something was thrown or rejected.
- **Start-up marks**: each milestone logs `startup: <mark> ms=<since start>` (shell created, services started, first commit, first vblank, page load started, first frame …), so a slow start can be located.
- **Timing**: `RUST_LOG=perf=debug` turns on timing lines by area: `perf::latency` (input to screen, split into queue, JS, render, raster and present), `perf::task` (event loop tasks over 8 ms), `perf::frame`, `perf::webgl`, `perf::audio`, `perf::raster`, `perf::layers`, `perf::compose` and `perf::rpc`.

## Embedding libacde

- **Rust**: the crate `libacde` (library name `acde`) is the one package front ends depend on, as a path dependency of a side-by-side checkout. A front end implements the platform interface (claiming window surfaces, file pickers, content types, input and display settings) and hands it to the runtime; it can also provide the terminal implementation and a CPU rasteriser.
- **C**: `libacde.so` with `include/acde.h` offers a C interface for other front ends. It is narrower than the Rust one: for example it passes only text through the clipboard, and does not pass on window looks, materials or input method panels.
- **Contract**: the contract level is defined once in the SDK (`acde-api/api-level`) and compiled into the runtime, so a front end and the packages it runs always agree on what "level 1" means. The versioned standard of the `acde:` interfaces is kept in AC/acde-RFC, which can check a runtime against a frozen version.

## For developers

[dev/build-run.md](../../../agents/dev/build-run.md) (building, commands, variables, logs and timing), [runtime/architecture.md](../../../agents/runtime/architecture.md) (the two ways to run, processes and threads), [runtime/ffi.md](../../../agents/runtime/ffi.md) (the C interface), [dev/testing.md](../../../agents/dev/testing.md) (regression tests and the Web Platform Tests).
