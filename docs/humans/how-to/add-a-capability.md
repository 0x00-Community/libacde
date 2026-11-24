# How to give apps a new system capability

Synced with docs/agents on 2026-11-24

A system capability is an `acde:` module that an app may import only when its manifest asks for it (for example `"capabilities": {"terminal": true}` lets a page `import { terminal } from 'acde:terminal'`). Adding one touches the SDK, the manifest rules, the native binding and the page glue. Use `acde:terminal` as the worked example throughout.

1. **Declare the types.** Add the module's declarations to the SDK in `acde-api/src/` (the module in `acde-api/src/modules/app-modules.d.ts`, its type in `acde-api/src/types/page.d.ts`, the manifest flag in `acde-api/src/types/manifest.d.ts`). The SDK holds declarations only. Rules for changing it, including deprecations: [acde-api.md](../../agents/acde-api.md).
2. **Teach the manifest rules the name.** Add it to `SYSTEM_CAPABILITIES` (and `CAPABILITY_MODULES` if it is a module of its own) in `manifest/index.ts`. The rules are shared by the runtime and acdev's `acdev check`, so both refuse a misspelled capability the same way.
3. **Write the native side.** Implement the module in Rust under `src/host/` (the terminal binding is `src/host/terminal.rs`). It is injected into the page's own isolate, not called over RPC. When the work needs the operating system's help, define a seam here and implement it in the front end (acdes implements the terminal sessions behind `acde:terminal`).
4. **Expose it to pages.** In the page glue, bind the native object only when the capability was granted (see how `natives.terminal` is attached in `js/page/src/worker.ts`). Granting and approval rules: [runtime/webs.md](../../agents/runtime/webs.md) "必需与可选能力" and "批准".
5. **Test it.** Runtime regression tests and WPT: [dev/testing.md](../../agents/dev/testing.md).
6. **Tell acdev.** A new capability is a visible change: acdev's CLI and its skill `npm/skills/acdev/` follow it in the same wave (rule in [AGENTS.md](../../../AGENTS.md)).
7. **Version it.** Once released, the module's interface is frozen in AC/acde-RFC by version; the SDK in this repository stays the source.
