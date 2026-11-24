# Title bars and window looks

Synced with docs/agents on 2026-11-24

App windows get a title bar that the runtime draws, hit-tests and drags. The desktop cannot draw it and never sees it in its DOM, but it can **declare** how every title bar looks and behaves, and how windows' corners, shadows and animations look. This keeps one consistent style across all apps, including ones that never heard of the desktop. Native programs draw their own decorations and are not affected.

## The title bar theme: `setFrameTheme`

`acde:desktop.setFrameTheme(theme | null)` declares the colours and layout of every runtime-drawn title bar:

- `active` and `inactive`, each `{background?, title?, line?, buttons?}`, with colours written as `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()` or `rgba()`;
- `buttonStyle: 'dots' | 'glyphs'` (traffic lights, or Windows-style minimise / maximise / close), `buttonSide: 'left' | 'right'`, `titleAlign: 'center' | 'start'`;
- `light: {active?, inactive?}`: the colours to use for pages that do not support dark mode.

Each call replaces the whole theme; `null` returns to the default. Open windows are redrawn at once, and windows switch between `active` and `inactive` as focus moves. A bad colour or value throws `TypeError`; unknown keys are ignored with a warning, so a desktop written for a newer runtime still works on an older one (runtimes from before this rule throw, so desktops probe first).

**Dark and light pages**: when the theme has `light` colours, windows whose page does not render dark get the `light` set (missing entries take the runtime's defaults, not the dark theme's). A page renders dark when its root's computed `color-scheme` contains `dark`, or the root says nothing and its `<meta name="color-scheme">` contains `dark`; if `light` is also allowed, only while the system (or an emulation) prefers dark. The runtime re-checks only when something that can affect this changes, so it costs nothing on ordinary DOM updates. With no theme at all (a single app under `acdev dev`), dark pages get the runtime's dark default and others the light one. A disabled minimise button is drawn in the title colour at 16 % opacity, so it stays visible on dark bars.

Who has these calls: the desktop page and system apps that declare `acde:desktop`. In an acdes session the desktop's theme reaches every app process, including ones started later; a system app's setting only affects its own windows.

## Double-click on the title bar

`acde:desktop.setDoubleClick('fill' | 'minimize' | 'none')` declares what double-clicking a title bar does. It can be changed at any time and applies to the next double-click; unknown values throw `TypeError`.

## Corners, shadows, motion and metrics: `setWindowLook`

`acde:desktop.setWindowLook(look | null)` declares the outer shape of windows, drawn by the compositor (window pages no longer draw outer shadows or rounded corners themselves):

| Key | Meaning |
| --- | --- |
| `corners: {radius, shape?}` | Outer corner radius 0–256, and the shape in CSS `corner-shape` terms: `round` (default), `squircle` or `superellipse(k)` with k from 1 to 3. |
| `shadows: {focused, unfocused}` | Up to 4 layers each, every layer `{x, y, blur, spread, color}` with the meaning of one CSS `box-shadow` outer shadow; earlier layers are drawn on top. |
| `motion` | The animation preset for windows: `'standard'` (default) or `'reduced'`. Only presets can be chosen, never numbers. |
| `titlebarHeight` | The title bar height in CSS pixels, an integer from 24 to 64 (default 32; Dopame uses 40). Content moves down accordingly. |
| `controls: {inset, size, gap, caption}` | Traffic-light distance from the edge (0–48), diameter (6–32), spacing (0–32), and the width of each Windows-style button (24–80). Defaults 12, 12, 8, 46. |
| `title: {size, weight}` | Title font size (9–24) and weight (100–900). Defaults 13 and 600. |

Every key may be left out: only the given keys change, the rest keep their last value (initially square corners, no shadow, `standard`, default metrics), so `setWindowLook({motion: 'reduced'})` alone is fine; `null` resets everything. Bad values throw `TypeError`, unknown keys are ignored with a warning. Metrics apply to title bars created after the call.

Only acdes's desktop process uses it (it needs a compositor recent enough); the acdev host window and the C interface ignore it. It is an optional method: call `desktop.setWindowLook?.(…)`.

In an acdes session, where the compositor can draw glass, the traffic-light buttons are drawn as small glass pieces by the compositor, and the runtime leaves holes for them in the title bar background.

## Following the compositor's animations: `motion()`

`acde:desktop.motion(preset?)` returns `{preset, duration, easing}` for a preset (or the one last chosen with `setWindowLook`): the duration in milliseconds and a CSS `linear(…)` easing sampled from the same curve the compositor uses. A desktop that slides its own layers in step with window animations (switching virtual desktops, for example) uses these values instead of keeping its own copy.

- `standard` is a critically damped spring: response 0.35 s, no overshoot, settled within 0.1 % after about 0.51 s.
- `reduced` is a 0.15 s linear fade with no scaling or movement.

When the system asks for reduced motion (the desktop portal's `reduced-motion`), shape animations such as glass appearing and disappearing use the reduced curve too.

## Apps that draw their own title bar

Apps with `frame: false` draw their own. If such apps should follow the desktop's theme, the desktop publishes it through one of its services (as Dopame does); the runtime does not do this.

## For developers

[runtime/desktop.md](../../../agents/runtime/desktop.md) ("窗框"), [runtime/webs.md](../../../agents/runtime/webs.md) ("窗框": the theme, the window look, dark pages), [graphics/render.md](../../../agents/graphics/render.md) (drawing the frame, animation curves).
