# Panels: menu bars, Docks, pop-ups and edges

Synced with docs/agents on 2026-11-24

Everything a desktop shows above app windows (a menu bar, a Dock, a pop-up menu, a notification, a confirmation card) is a **panel**: a piece of the desktop page that the runtime lifts into a window of its own. The element keeps living in the desktop page's single document, with the same component tree and the same events, so a React or Vue desktop does not have to split its state across windows. This page explains pinning, reserving screen edges, keyboard behaviour, anchored pop-ups and the other panel options.

## Pinning an element

`acde:desktop.pin(element, options?)` turns an element into a panel and returns a `PinnedPanel` with `update()`, `unpin()`, `reserve(...)`, `anchor(...)`, `popup` and `closed`.

- The panel floats above all app windows. Its input region is exactly the element's box; an element with `pointer-events: none` or that is invisible receives nothing, and pointer events go back into the same document.
- The runtime re-measures the panel when the element changes size, content or animation (it watches resizes and mutations and re-measures again after transitions end), and hides the panel when it cannot be measured. `update()` re-measures at once.
- The element must be a stacking context; ancestors' clipping and opacity do not apply to it. `margin` leaves room around it for shadows.
- From the moment of pinning the element is no longer drawn in the main page, so there is never a moment when it is visible but clicks fall through to a window behind it.
- `backdrop-filter` on a panel is handed to the compositor, which blurs the windows underneath (acdes uses the largest blurred area per panel). System glass works too (see [system glass](../graphics/glass.md)).
- Call `unpin()` when the component unmounts.

## Reserving an edge

`reserve: {edge, thickness?}` makes the panel occupy a strip along an output edge, like a layer-shell exclusive zone:

- The **work area** is the output minus the reserved strips (the largest per edge). Filled windows resize with it and new windows avoid it. Full-screen windows ignore it.
- Without `thickness`, the strip reaches from the edge to the panel's far side and follows the measurement. Give a thickness explicitly when the panel animates or grows on hover, so windows are not pushed around.
- `panel.reserve(r | null)` changes or drops the reservation in place; unpinning and pinning again would change the work area twice.
- Apps learn the new work area through `acde:window.output.workArea`; the desktop reads it from `acde:display`.

## Keyboard

- `keyboard: 'on-demand'` (the default) gives the panel the keyboard only when clicked: menu bars and Docks. The desktop page itself behaves the same way.
- `keyboard: 'exclusive'` takes the keyboard while the panel is shown and gives it back to the previous window when it closes: pop-up menus and confirmation cards opened from the keyboard that must receive keys at once.
- Keys go to the desktop page's focused element, so when a panel opens, move the page's focus into it.
- `acde:desktop.requestGrab('pointer' | 'keyboard')` / `releaseGrab(...)` ask the window system for a grab through the desktop page's surface, for example so a confirmation card can hold the keyboard.

## Panels only the user may answer

`userOnly: true` marks a card that only the person may answer (a confirmation, a permission prompt), usually together with `keyboard: 'exclusive'`. The runtime passes the flag down to the window system; acdes's session manager then refuses keys injected by Agents and pointer events on the panel while it is shown. Ordinary exclusive panels (pop-up menus, launchers, chat overlays) must not set it, so Agents can still operate and dismiss them. Single elements that only the user may click are protected separately by the session manager's per-step check (Dopame marks them `data-user-only`).

## Anchored pop-ups

Menus, tooltips and drop-downs that belong to a spot inside some window use `anchor: {window, x, y, width?, height?, edge?, align?, adjust?}` instead of screen coordinates:

- The rectangle is in that window's page CSS pixels (width and height default to 0, a point). The compositor places the panel on its `edge` (default `bottom`), aligned `start` / `center` / `end` (default `start`), and if it does not fit, `flip`s to the other side and / or `slide`s onto the screen (both by default).
- The panel follows its parent window when it moves or hides and closes when the parent closes. Alignment uses the element itself, so `margin` for shadows does not offset it. `panel.anchor(next)` moves the anchor (a drop-down following a scrolling field).
- It cannot be combined with `reserve` or `keyboard: 'exclusive'` (`TypeError`; a menu that needs the keyboard uses `requestGrab`).
- Where the window system cannot anchor (older acdes, the acdev host window) `pin` throws `NotSupportedError`, and the desktop computes coordinates itself.

## Pop-up windows

`popup: true` (with an optional `parent` panel) makes a real pop-up window of the xdg_popup kind: the compositor closes it when the user clicks outside, so the page no longer has to work out where clicks land.

- It can be combined with `anchor`, but not with `keyboard: 'exclusive'` or `reserve` (a pop-up takes the keyboard itself and reserves nothing).
- When the compositor dismisses it, `closed` resolves with `'dismissed'`; when a panel ends, the pop-ups under it end with `'parent-closed'`.
- Where pop-ups are not available (host windows, older acdes) the panel opens as an ordinary on-demand panel; `panel.popup` says what actually happened, and the page must then close it on outside clicks itself. `popup` is false until the element is visible and positioned.

## Kinds of panel and stacking

`kind` places the panel in a band of the shell layer, later bands covering earlier ones: `'dock'` (menu bars, Docks), `'floating'` (the default), `'notice'` (notifications) and `'confirm'` (cards that need an answer). Pop-ups always sit in the pop-up band regardless of `kind`. Unknown values throw `TypeError`.

## When a panel ends

Every panel has a `closed` promise that resolves with the reason: `'unpinned'` (the page called `unpin()`), `'parent-closed'` (the anchoring window closed; the runtime already removed the panel), `'refused'` (the window system would not anchor it: the window is gone, the session is locked, or it never opened), or `'dismissed'` (a pop-up the compositor closed).

## Full-screen windows

On a screen whose front window is full screen, `dock`, `floating` and `notice` panels drop below that window and are covered by it; the desktop does not need to hide them or make them transparent. `confirm` cards, the input method's candidate panel and the lock screen stay on top. Panels on other screens are not affected.

## Screen edges

`acde:desktop.on('edge', handler)` reports the pointer entering and leaving a screen edge (`{edge: 'top' | 'right' | 'bottom' | 'left', entered, output}`; a corner counts as two edges), so a desktop can reveal an auto-hidden menu bar or Dock. An edge only counts where no other output continues beyond it. Host windows send it (relative to the host window's content area); acdes sessions do not.

## For developers

[runtime/desktop.md](../../../agents/runtime/desktop.md) ("面板与占边"), [runtime/webs.md](../../../agents/runtime/webs.md) ("钉住的面板": measuring, work area, grabs, edges), [runtime/layout.md](../../../agents/runtime/layout.md) (panel calls through the session manager).
