import assert from 'node:assert/strict'
import { test } from 'node:test'
import { page, type Web } from './harness.ts'

const NONE = -1

async function form(html: string): Promise<Web> {
  const web = await page(`<!doctype html><html><body>${html}</body></html>`)
  web.run(`globalThis.__log = []
    for (const s of document.querySelectorAll('select'))
      for (const type of ['input', 'change', 'mousedown', 'click']) s.addEventListener(type, (e) => __log.push(s.id + ':' + type))`)
  return web
}

function id(web: Web, name: string): number {
  return web.internals.idOf(web.run(`document.getElementById('${name}')`))
}

function box(web: Web, name: string): void {
  web.dom.boxes.set(id(web, name), { x: 0, y: 0, width: 120, height: 24 })
}

function pointer(web: Web, type: 'pointerdown' | 'pointermove' | 'pointerup', target: string | null, modifiers = {}): void {
  web.internals.input.pointer(type, 10, 10, { target: target === null ? undefined : id(web, target), modifiers })
}

function click(web: Web, target: string, modifiers = {}): void {
  pointer(web, 'pointerdown', target, modifiers)
  pointer(web, 'pointerup', target, modifiers)
}

function key(web: Web, k: string, modifiers = {}): boolean {
  const down = web.internals.input.key('keydown', k, k, { modifiers })
  web.internals.input.key('keyup', k, k, { modifiers })
  return down
}

const log = (web: Web) => web.json<string[]>('__log.splice(0)')
const value = (web: Web, name: string) => web.run<string>(`document.getElementById('${name}').value`)

test('select: a press opens the picker, dragging onto an option and releasing picks it', async () => {
  using web = await form(`<select id="s"><option>a</option><option>b</option><option disabled>c</option><option>d</option></select>`)
  box(web, 's')
  pointer(web, 'pointerdown', 's')
  assert.equal(web.dom.picker.select, id(web, 's'), 'open')
  assert.equal(web.dom.picker.active, 0, 'the selected option is highlighted')
  web.dom.pickerHit = () => 2
  pointer(web, 'pointermove', null)
  assert.equal(web.dom.picker.active, 0, 'a disabled option is not highlighted')
  web.dom.pickerHit = () => 3
  pointer(web, 'pointermove', null)
  assert.equal(web.dom.picker.active, 3)
  pointer(web, 'pointerup', null)
  assert.equal(web.dom.picker.select, NONE, 'closed')
  assert.equal(value(web, 's'), 'd')
  assert.deepEqual(log(web), ['s:mousedown', 's:input', 's:change'])
})

test('select: a click keeps the picker open, a click on an option picks it, a click outside closes it', async () => {
  using web = await form(`<select id="s"><option>a</option><option>b</option></select><button id="b">x</button>`)
  box(web, 's')
  box(web, 'b')
  click(web, 's')
  assert.equal(web.dom.picker.select, id(web, 's'))
  assert.deepEqual(log(web), ['s:mousedown', 's:click'], 'the opening click reaches the page')
  web.dom.pickerHit = () => 1
  pointer(web, 'pointerdown', null)
  pointer(web, 'pointerup', null)
  assert.equal(value(web, 's'), 'b')
  assert.deepEqual(log(web), ['s:input', 's:change'], 'the picker swallows its own presses')
  click(web, 's')
  log(web)
  web.dom.pickerHit = () => 0
  pointer(web, 'pointerdown', null)
  pointer(web, 'pointerup', null)
  assert.equal(value(web, 's'), 'a')
  click(web, 's')
  web.dom.pickerHit = () => -2
  pointer(web, 'pointerdown', 'b')
  pointer(web, 'pointerup', 'b')
  assert.equal(web.dom.picker.select, NONE)
  assert.equal(value(web, 's'), 'a', 'outside: no change')
})

test('select: preventDefault on mousedown, disabled selects and showPicker()', async () => {
  using web = await form(`<select id="s"><option>a</option></select><select id="d" disabled><option>a</option></select>`)
  box(web, 's')
  box(web, 'd')
  web.run(`document.getElementById('s').addEventListener('mousedown', (e) => e.preventDefault(), { once: true })`)
  click(web, 's')
  assert.equal(web.dom.picker.select, NONE, 'cancelled')
  click(web, 'd')
  assert.equal(web.dom.picker.select, NONE, 'disabled')
  assert.equal(web.run(`(() => { try { document.getElementById('d').showPicker() } catch (e) { return e.name } })()`), 'InvalidStateError')
  web.run(`document.getElementById('s').showPicker()`)
  assert.equal(web.dom.picker.select, id(web, 's'))
})

test('select: arrows, Home / End and typing change a closed drop-down box; Alt+ArrowDown opens it', async () => {
  using web = await form(`<select id="s"><option>apple</option><option disabled>banana</option><option>cherry</option><option>avocado</option></select>`)
  box(web, 's')
  web.run(`document.getElementById('s').focus()`)
  key(web, 'ArrowDown')
  assert.equal(value(web, 's'), 'cherry', 'skips the disabled option')
  assert.deepEqual(log(web), ['s:input', 's:change'])
  key(web, 'End')
  assert.equal(value(web, 's'), 'avocado')
  key(web, 'Home')
  assert.equal(value(web, 's'), 'apple')
  key(web, 'ArrowUp')
  assert.equal(value(web, 's'), 'apple', 'nothing above the first')
  log(web)
  key(web, 'a')
  assert.equal(value(web, 's'), 'avocado', 'the next one starting with a')
  key(web, 'Alt', { alt: true })
  key(web, 'ArrowDown', { alt: true })
  assert.equal(web.dom.picker.select, id(web, 's'))
})

test('select: in the picker, keys move the highlight; Enter picks, Escape cancels, focus loss closes', async () => {
  using web = await form(`<select id="s"><option>a</option><option>b</option><option>c</option></select><input id="t">`)
  box(web, 's')
  box(web, 't')
  web.run(`document.getElementById('s').focus()`)
  key(web, ' ')
  assert.equal(web.dom.picker.select, id(web, 's'), 'space opens it')
  log(web)
  key(web, 'ArrowDown')
  key(web, 'ArrowDown')
  assert.equal(web.dom.picker.active, 2)
  assert.equal(value(web, 's'), 'a', 'only highlighted')
  assert.deepEqual(log(web), [], 'keys go to the picker, not the page')
  key(web, 'Enter')
  assert.equal(value(web, 's'), 'c')
  assert.equal(web.dom.picker.select, NONE)
  assert.deepEqual(log(web), ['s:input', 's:change'])
  key(web, ' ')
  key(web, 'Home')
  key(web, 'Escape')
  assert.equal(web.dom.picker.select, NONE)
  assert.equal(value(web, 's'), 'c', 'Escape keeps the value')
  key(web, ' ')
  web.run(`document.getElementById('t').focus()`)
  assert.equal(web.dom.picker.select, NONE, 'focus moved away')
})

test('select: list boxes pick on press; Ctrl toggles and Shift extends in a multiple one', async () => {
  using web = await form(`<select id="m" multiple><option>a</option><option>b</option><option>c</option><option>d</option></select>`)
  box(web, 'm')
  const picked = () => web.json<string[]>(`[...document.getElementById('m').selectedOptions].map((o) => o.value)`)
  web.dom.listBox = () => 1
  click(web, 'm')
  assert.deepEqual(picked(), ['b'])
  assert.equal(web.dom.picker.select, NONE, 'no picker for a list box')
  assert.deepEqual(log(web), ['m:mousedown', 'm:input', 'm:change', 'm:click'])
  web.dom.listBox = () => 3
  click(web, 'm', { shift: true })
  assert.deepEqual(picked(), ['b', 'c', 'd'])
  web.dom.listBox = () => 2
  click(web, 'm', { ctrl: true })
  assert.deepEqual(picked(), ['b', 'd'])
  web.dom.listBox = () => 0
  click(web, 'm')
  assert.deepEqual(picked(), ['a'])
  key(web, 'ArrowDown', { shift: true })
  key(web, 'ArrowDown', { shift: true })
  assert.deepEqual(picked(), ['a', 'b', 'c'])
  key(web, 'ArrowDown')
  assert.deepEqual(picked(), ['d'])
})
