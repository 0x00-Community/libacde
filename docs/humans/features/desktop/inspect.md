# Seeing and operating the desktop as text: `acde:inspect`

Synced with docs/agents on 2026-11-24

`acde:inspect` lets an Agent work with the screen without screenshots: it turns every window into a tree of roles, names and states, gives each element a stable reference, and performs clicks, typing and key presses on those references with real input. Only the desktop page and desktop services that declare it can use it. The same machinery answers `acdev inspect`, `acdev act` and the acdev MCP tools during development, so what a developer sees there is what an Agent sees.

## The tree

Each web page is described as nodes `{id, role, name?, value?, states?, level?, rect, children?}`:

- **Roles** come from the `role` attribute or the HTML element (HTML-AAM: a named `<section>` is `region`, `<output>` is `status`); an element with no role but `cursor: pointer` is `clickable`. A `<select>` drop-down is a `combobox` (its value is the selected text, and when open its children are the visible options); a list box is a `listbox`.
- **Names** follow ARIA and labels, up to 120 characters, joining adjacent text the way it was written (`{n}%` in JSX reads `100%`).
- **Invisible things are pruned**: `display: none`, `aria-hidden`, `inert`, `opacity: 0`, `visibility: hidden` and content scrolled out of view. Nodes with no role, name or state are flattened. Each parent lists at most 60 children and a whole tree at most 600 nodes by default. Options can keep scrolled-out content (`offscreen`) or transparent and hidden elements (`invisible`, marked `transparent` / `invisible`; transparent elements can still be clicked, invisible ones cannot).
- **References follow DOM nodes**: an element keeps its reference while it lives, through moves, text changes, attribute changes and list reordering. Only when the page replaces it (new `innerHTML`, a list rebuilt without keys) does it get a new one, and the old reference answers `element N is gone`.

## References and the text snapshot

- Windows are `w0` (the desktop), `w1`, `w2` …; elements are `w3.12`; a window's title bar menus are `w3.m1` (first menu) and `w3.m1.2` (its second item; separators count).
- `snapshot({window?, ref?, all?, diff?, limit?, find?, offscreen?})` returns a fixed English text format, independent of the system language:

```text
- window "Notes" [app=notes] [focused] [w1]
  - button "Save" [pressed] [w1.52]
  - textbox "Title": "Shopping list" [w1.53]
  - heading "Today" [level=2] [w1.54]
- desktop
```

  Window lines carry markers in this order: `app=`, `native`, `focused`, `minimized`, the mode when not `free`, and the state `filled` / `fullscreen`. The focused window (or the one asked for) is expanded; `all` expands every window that is not minimised. Notes in parentheses explain what was left out (`(empty)`, `(N more outside the visible area; …)`, `(too many elements, …)`).
- `find` keeps only elements whose name or value contains a text, with their ancestors. `diff` lists only changed lines (`+`, `~`, `-`), relisting everything when more than 60 % changed, and starts with `(windows unchanged)` when the window list is the same.
- `query({selector, window?, ref?, limit?, attributes?})` finds elements by CSS selector, including ones not in the semantic tree, and returns references that `act` accepts, with `tag`, `visible` and up to 20 requested attributes.

## Acting

`act(ref, action, arg)` performs one action and answers `{ok, text}` plus, when the action completed, the windows and a snapshot of what changed. `run(steps)` performs several and stops at the first failure. Actions wait for the page to settle (no DOM changes for 200 ms, at most 3 seconds) and, if the focused window changed, for the new one.

- **Pointer actions** (`click`, `double`, `right`, `hover`, `scroll`) find a clickable point of the element (scrolling it into view and trying its centre and four quarter points), then send a whole sequence of real pointer events through the window system. The compositor raises and focuses windows as it would for a person. If something covers the element, the answer names it. An element that is gone or not displayed is reported as such instead of clicking whatever is at its old place.
- **`type` / `settext`** enter text; **`key`** presses keys (`"ctrl+a backspace"`, `"7 . 5 * 2"`; characters that need Shift on a US keyboard get it automatically). **`select`** picks options by value or text. **`drag`** moves an element onto another reference, also in another window, with realistic timing (a pause after pressing, one frame between moves). **`focus`** focuses an element.
- **Window references** accept `focus`, `restore`, `minimize`, `close`, `key`, `fill`, `fullscreen`, `move` (`"100,80"`), `resize` (`"800x600"`), `raise` and `topmost` (`off` to undo). The desktop, a window filling a host window, pinned panels, minimised windows and windows not in `normal` state refuse moves and resizes with a reason (for example "act restore on it first").
- **Menus**: acting on a menu item opens its menu first if needed and clicks the row once it is drawn; disabled items are not clicked.
- **Minimised windows** are marked `[minimized]`, skipped by `all` and `find`, and pointer actions on them answer `w1 is minimized; act restore on it first` without injecting anything. Restoring or focusing brings them back.
- **`open(name)`** starts an app or native program by id or name.
- In a Dopame OS session the **lock page** and the **system confirmation page** appear as windows on top (`[lock]`, `[confirm]`); they cannot be moved, resized, minimised or closed, and their buttons are operated like any other element.

## Windows without elements

- **Pixels** (acdes sessions): games, OpenGL / Vulkan programs and native programs without accessibility information are handled by pixels: `shot(window, {max?})` returns that window's own image as PNG (long side scaled to `max`, default 1280) with its rectangle, and `input(window, events)` sends moves, presses, wheel, relative motion (to turn a game camera), key down / up, text and waits in window coordinates.
- **Native windows with accessibility** (Linux sessions) are read through AT-SPI2: GTK 3 apps always report, Qt apps when enabled. Roles are normalised to ARIA names, and actions use AT-SPI's own actions, editable text and focus before falling back to coordinates.
- **The Agent's stage** (acdes): native programs an Agent opens appear with `[stage]`; they are not on screen and do not take the person's keyboard, and the Agent's input reaches them without waiting for the person. When the person activates such a window, they take over.

## In a production session

`acde:inspect` is not only a development tool: in a regular Dopame OS session the system Agent uses it through a desktop service to read other apps' windows and click elements. There, only `webs`, `snapshot`, `query`, `act` and `run` are answered; raw coordinate input, evaluation, fake responses, clocks and logs are refused. Every `act` must carry a guard selector, and a step whose target (or the element under the pointer, or the focused element for keys) matches it is refused with "only the user may act on this element". Dopame marks permission prompts and confirmation cards with `data-user-only` for this purpose.

## For developers

[web/inspect.md](../../../agents/web/inspect.md) (the tree, references, snapshot format, actions, pixels, native windows), [dev/dev-ipc.md](../../../agents/dev/dev-ipc.md) (the same requests on the dev channel, production sessions, guards).
