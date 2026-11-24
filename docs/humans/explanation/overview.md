# libacde overview

Synced with docs/agents on 2026-11-24

libacde is the runtime library behind the Dopame desktop. A desktop is a web page: page JavaScript runs in an embedded V8, the DOM is laid out by a Blitz fork and painted with vello on the GPU. There is no browser and no Node inside.

## Two front ends, one runtime

- **acdev** runs the runtime in a native window on Linux, Windows and macOS, for development (`acdev dev`).
- **acdes** runs it as the Dopame OS session: compositor, DRM/KMS, login, lock screen.

Both link libacde as a path dependency (`../libacde`). Anything both need lives here; anything only one needs lives in that front end. See [runtime/architecture.md](../../agents/runtime/architecture.md).

## The first rule

Web APIs behave exactly as in a browser. System features come only through `acde:` modules (`import { fs } from 'acde:fs'`), granted by the app's manifest. The full rule is at the top of [AGENTS.md](../../../AGENTS.md).

## Where to read next

- Writing a desktop: [runtime/desktop.md](../../agents/runtime/desktop.md).
- The SDK `@acde/api`: [acde-api.md](../../agents/acde-api.md).
- Which directory owns what: [runtime/layout.md](../../agents/runtime/layout.md).
