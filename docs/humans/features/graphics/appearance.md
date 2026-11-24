# Appearance: dark mode, accent colour, glass level, motion and language

Synced with docs/agents on 2026-11-24

The user's appearance settings reach every page through standard CSS and web interfaces wherever a standard exists, and through `acde:window` only where none does. In a Dopame OS session the settings come from the desktop portal (`org.freedesktop.appearance`, plus acdes's own `org.acde.appearance` for the glass settings and wallpaper tone); in a host window they come from the host system or from environment variables. Pages are told about changes as they happen.

## Dark and light

- `prefers-color-scheme` and `matchMedia('(prefers-color-scheme: dark)')` follow `ACDE_COLOR_SCHEME` (`light` or `dark`) when set, otherwise the portal's `color-scheme` (Linux), otherwise light.
- A change fires `change` on the media query list and restyles the page.
- During development `acdev appearance` (and the `emulate` tool of acdev's MCP server) can force a scheme for all pages; the system's own changes are held back until the emulation is removed.

## Accent colour

- CSS system colours `AccentColor` and `AccentColorText` follow the portal's `accent-color`; `AccentColorText` is white or black, whichever contrasts better. Without a setting they are the defaults (`#0075ff` with white; `#99c8ff` with black on dark pages).
- CSS cannot tell "not set" from "the user picked the default blue", so `acde:window.accent()` returns the raw `#rrggbb` or `null`, and `on('accent')` reports changes.

## Glass level and reduced transparency

- acdes's portal namespace carries the **glass level** (0 clear, 1 medium, 2 tinted) and **reduce transparency** (on / off). Pages redraw their glass when either changes (see [system glass](glass.md)).
- Reduce transparency is also the only source of `@media (prefers-reduced-transparency: reduce)` and the matching `matchMedia`, so CSS can react to it like any media feature.

## Reduced motion

The portal's `reduced-motion` switches shape animations (glass pieces appearing and disappearing, window animations when the desktop chose to follow it) to the short linear "reduced" curve. See [title bars and window looks](../desktop/frames-and-looks.md) for the two presets.

## Wallpaper tone

acdes measures whether each screen's wallpaper is mostly light or dark; `acde:window.wallTone()` returns `light`, `dark` or `null` for the screen under the window (falling back to the main screen), and `on('wallTone')` reports changes, including when the window moves to another screen. Outside a Dopame OS session there is no portal and it stays `null`.

## Language

- Pages get their language from `LANGUAGE`, `LC_ALL`, `LC_MESSAGES` or `LANG`; with none set, from the system's preferred languages (Windows and macOS user settings, `/etc/locale.conf` or `/etc/default/locale` on Linux); otherwise `en-US`.
- `acde:session.setLanguage(tag)` (desktop only) writes the system locale and fires `languagechange`.
- The runtime's own strings follow the page's language (Chinese or English, other languages getting English): the labels of file roots (Documents, Desktop, app data …) and the titles of folder, open and save dialogs.

## For developers

[dev/build-run.md](../../../agents/dev/build-run.md) (colour scheme, glass level, reduced motion, accent colour, wallpaper tone, language), [acde-api.md](../../../agents/acde-api.md) (`accent()`, `wallTone()`).
