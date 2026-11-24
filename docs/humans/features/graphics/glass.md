# System glass, blur and window materials

Synced with docs/agents on 2026-11-24

Dopame's look relies on translucent surfaces: frosted menus, liquid-glass toolbars that bend what is behind them, windows whose background shows the desktop. libacde provides these as **system glass**, written in ordinary CSS, and as **window materials**, chosen per window. Every parameter lives in one table shared by the page renderer and the compositor, so a glass element looks the same whether the runtime composites it itself (a host window) or hands it to acdes's compositor (a Dopame OS session). Other browsers simply show the plain `backdrop-filter`.

## Blur

`backdrop-filter: blur(...)` works everywhere: on page elements, on pinned panels (where the compositor blurs the windows underneath) and on window regions (`acde:window.setBackdrop`). Blur is a chain of down- and up-sampling passes whose depth follows the blur radius, close to a true Gaussian; a blur of 0 copies the background unchanged.

## System glass in CSS

An element becomes system glass when it has a `backdrop-filter` **and** the custom property `--acde-glass`:

| `--acde-glass` | Look |
| --- | --- |
| `regular` | Liquid glass at the user's chosen level. |
| `clear` | One level more see-through than `regular`. |
| `blur` | Background blur with a tint, no lens, highlight, outline or refraction; for menus, banners and search fields. It is the same as the window material `blur`. |

Glass follows the system: its **level** (see [appearance](appearance.md)), dark or light pages, and "reduce transparency" (which turns all glass into a nearly opaque tint).

### Levels

| Level | Character |
| --- | --- |
| Clear (default) | Almost no blur, a refracting bevel at the edges, no tint; the middle stays sharp and still. |
| Medium | More blur and stronger inner refraction. |
| Tinted | Heavier blur and a tint. |
| Opaque | Used when the user asks to reduce transparency: heavy blur and an almost opaque tint, light or dark by page. |

### Fine-tuning

Further custom properties override single parameters of the preset; a value that cannot be parsed is ignored with a warning:

- `--acde-glass-refraction`, `-blur`, `-bleed`, `-white-point`, `-smoothing` and `-tint` (a colour as `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa` or `rgb(r g b / a)`);
- `--acde-glass-highlight: <angle> <strength> [<width>]`: where the light comes from (0deg is up, clockwise), how strong, and how wide the bright edge is. The side facing the light gets a thin bright line and the opposite side a fainter one; the preset is `330deg 0.45 4px`;
- `--acde-glass-outline: <width> <colour> [pieces | merged] | none`: a thin outline, on by default (1 px, half-white on light pages, quarter-white on dark ones). `pieces` outlines each piece of a merged group separately (for distinguishable buttons); `merged` follows the group's outer edge;
- `--acde-glass-lens: <bevel width> <thickness> [<index> [<dispersion>]]`: the edge lens. Only a ring at the edge bends the background, the way Apple's glass does: the profile is a rounded bevel, refraction follows Snell's law, it never flips the image, and red, green and blue bend slightly differently for a coloured fringe. Thickness 0 or an index of 1 turns it off;
- `--acde-glass-darken: <number>`: how much the glass darkens what is behind;
- `--acde-glass-film: <strength> | auto | none`: a thin-film interference shimmer, strongest near the edges and slowly drifting (weaker on dark pages, still when motion is reduced). Off by default; `auto` is 0.2.

### Merging pieces

Pieces of glass close to each other can melt together like drops, as with Apple's glass containers:

- `--acde-glass-merge: <length>` on a container makes the glass inside it merge when pieces come closer than that distance (10px is the recommended value). Shapes bulge towards each other below the distance and join below half of it, with highlights, refraction and outlines following the joined outline. Pieces at least that far apart look exactly as if alone.
- `--acde-glass-union: <name>` merges glass with the same name regardless of where it sits in the DOM.
- Glass inside glass (buttons on a glass toolbar) forms a separate layer: the buttons merge with each other but not with the toolbar. Page glass never merges with window materials or across windows.
- A group holds at most 16 pieces; extra pieces are drawn on their own (`acdev render` reports `group-full`).

### Appearing and disappearing

Give a piece `--acde-glass-id: <name>` and it animates: a new piece grows out of the nearest piece of its group, and a removed piece shrinks back into the nearest one, on the same spring curve as window animations (about half a second). Pieces present in the first frame, pieces without a name and pieces with nothing to grow from simply appear. With reduced motion, pieces appear and vanish at once.

## Window materials

A window can have a material painted behind its whole content (or only its title bar):

- `acde:window.setMaterial(name, options?)` and `material()`, or the default in the manifest's `window.material`.
- Built-in names: `glass` (liquid glass; `role` `regular` or `clear`; windows can merge with a shared `group` and `spacing`), `blur` (background blur with a tint) and `none`. Other valid names (`[a-z0-9._-]`, up to 64 bytes) are reserved for future custom effects: an unknown name draws as `none` with one warning, and `material()` reports `none`.
- `overrides` maps the same parameter names as the CSS properties (`blur`, `tint`, `outline` …) to CSS values. The level, dark / light and reduced transparency remain the system's.
- `region: 'titlebar'` limits the material to the title bar.
- The page background must be transparent for a material to show.
- **Cost**: use `blur` for long-lived main window backgrounds (moderate cost), `glass` for small toolbars and panels floating above content (the most expensive), and `blur` again for short-lived pop-ups.

In a Dopame OS session the compositor draws materials; in a host window the runtime's own compositor does. The C interface does not pass materials on yet.

## When glass cannot be drawn fully

Without a GPU the CPU cannot refract, so glass falls back to frosted blur with tint and darkening (`acdev render` reports `cpu-cannot-refract`). Transparent pixels under glass (the glass sits on a transparent panel, with the real background further down) are treated as "only tint", so a capsule over the wallpaper does not turn dark.

## For developers

[graphics/glass.md](../../../agents/graphics/glass.md) (the blur chain, presets, the material table, groups, the lens, film, animations, wire formats), [acde-api.md](../../../agents/acde-api.md) (`setMaterial`, `setBackdrop`).
