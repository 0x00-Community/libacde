import assert from 'node:assert/strict'
import { test } from 'node:test'
import { page } from './harness.ts'

const PAGE = '<!doctype html><html><body><div id="outer"><div id="inner"><span id="leaf">x</span></div></div></body></html>'

test('dispatch: capture down from the window, target, bubble up; stopPropagation; once; passive', async () => {
  using web = await page(PAGE)
  const order = web.json<string[]>(`(() => {
    const log = []
    const leaf = document.getElementById('leaf'), inner = document.getElementById('inner'), outer = document.getElementById('outer')
    const at = (name) => (e) => log.push(name + ':' + e.eventPhase + ':' + (e.currentTarget === window ? 'window' : e.currentTarget.id || e.currentTarget.nodeName))
    window.addEventListener('ping', at('w-capture'), true)
    window.addEventListener('ping', at('w-bubble'))
    document.addEventListener('ping', at('d-capture'), { capture: true })
    outer.addEventListener('ping', at('o-capture'), true)
    outer.addEventListener('ping', at('o-bubble'))
    leaf.addEventListener('ping', at('t-bubble'))
    leaf.addEventListener('ping', at('t-capture'), true)
    inner.addEventListener('ping', at('i-once'), { once: true })
    leaf.dispatchEvent(new Event('ping', { bubbles: true }))
    leaf.dispatchEvent(new Event('ping', { bubbles: true }))
    return log
  })()`)
  assert.deepEqual(order.slice(0, 9), ['w-capture:1:window', 'd-capture:1:#document', 'o-capture:1:outer', 't-capture:2:leaf', 't-bubble:2:leaf', 'i-once:3:inner', 'o-bubble:3:outer', 'w-bubble:3:window', 'w-capture:1:window'])
  assert.equal(order.filter((e) => e.startsWith('i-once')).length, 1)

  const stopped = web.json(`(() => {
    const log = []
    const leaf = document.getElementById('leaf'), inner = document.getElementById('inner')
    inner.addEventListener('pong', (e) => { log.push('inner'); e.stopPropagation() })
    document.body.addEventListener('pong', () => log.push('body'))
    leaf.addEventListener('pong', (e) => { log.push('a'); e.stopImmediatePropagation() })
    leaf.addEventListener('pong', () => log.push('b'))
    const event = new CustomEvent('pong', { bubbles: true, cancelable: true, detail: 7 })
    leaf.addEventListener('pong', (e) => e.preventDefault(), { passive: true })
    const result = leaf.dispatchEvent(event)
    return { log, result, phase: event.eventPhase, current: event.currentTarget, detail: event.detail, path: event.composedPath().length }
  })()`)
  assert.deepEqual(stopped, { log: ['a'], result: true, phase: 0, current: null, detail: 7, path: 0 })
})

test('composedPath, window.event, inline handlers from markup and properties, errors reported', async () => {
  using web = await page('<!doctype html><html><body><div id="d" onclick="window.clicked = (window.clicked || 0) + 1; return false"><b id="b">x</b></div></body></html>')
  const result = web.json(`(() => {
    const b = document.getElementById('b')
    let path, current
    document.body.addEventListener('click', (e) => { path = e.composedPath().map((t) => t === window ? 'window' : t.nodeName); current = window.event === e })
    const ev = new MouseEvent('click', { bubbles: true, cancelable: true })
    const notCancelled = b.dispatchEvent(ev)
    const d = document.getElementById('d')
    const fromAttr = typeof d.onclick
    d.onclick = null
    b.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    return { path, current, clicked: window.clicked, notCancelled, fromAttr, after: window.event === undefined }
  })()`)
  assert.deepEqual(result, { path: ['B', 'DIV', 'BODY', 'HTML', '#document', 'window'], current: true, clicked: 1, notCancelled: false, fromAttr: 'function', after: true })

  const seen = web.json(`(() => {
    const log = []
    window.addEventListener('error', (e) => log.push('error:' + e.message))
    document.body.addEventListener('boom', () => { throw new Error('bad') })
    document.body.addEventListener('boom', () => log.push('second'))
    document.body.dispatchEvent(new Event('boom'))
    return log
  })()`)
  assert.deepEqual(seen, ['error:bad', 'second'])
  assert.equal(web.host.errors.length, 1)
})

test('body event handler attributes and properties are the window\'s', async () => {
  using web = await page('<!doctype html><html><body onload="window.loaded = (window.loaded || 0) + 1" onfocus="window.bodyFocus = true"><p>x</p></body></html>')
  const parsed = web.json(`(() => {
    const before = typeof window.onload
    const onPageLoad = window.loaded
    window.dispatchEvent(new Event('load'))
    document.body.dispatchEvent(new Event('focus'))
    return { before, onPageLoad, mirrored: document.body.onload === window.onload, loaded: window.loaded, bodyFocus: window.bodyFocus === true, own: document.body.getAttribute('onload') !== null }
  })()`)
  assert.deepEqual(parsed, { before: 'function', onPageLoad: 1, mirrored: true, loaded: 2, bodyFocus: false, own: true })

  const changed = web.json(`(() => {
    const log = []
    document.body.setAttribute('onhashchange', 'window.hashed = true')
    window.dispatchEvent(new Event('hashchange'))
    const f = () => log.push('prop')
    document.body.onresize = f
    window.dispatchEvent(new Event('resize'))
    const created = document.createElement('body')
    created.onpopstate = () => log.push('created')
    window.dispatchEvent(new Event('popstate'))
    const other = document.implementation.createHTMLDocument('x').body
    other.onstorage = () => log.push('other')
    window.dispatchEvent(new Event('storage'))
    window.onload = null
    return { hashed: window.hashed === true, log, resize: window.onresize === f, otherOwn: other.onstorage, storage: window.onstorage, cleared: document.body.onload }
  })()`)
  assert.deepEqual(changed, { hashed: true, log: ['prop', 'created'], resize: true, otherOwn: null, storage: null, cleared: null })
})

test('click activation: checkbox toggles (undone when cancelled), label clicks its control, submit fires submit', async () => {
  using web = await page('<!doctype html><html><body><form id="f"><label id="l">x <input id="c" type="checkbox"></label><input id="t" name="t" value="v"><button id="s">go</button></form></body></html>')
  const result = web.json(`(() => {
    const log = []
    const c = document.getElementById('c'), f = document.getElementById('f')
    c.addEventListener('change', () => log.push('change ' + c.checked))
    c.addEventListener('input', () => log.push('input'))
    c.click()
    const afterClick = c.checked
    c.addEventListener('click', (e) => e.preventDefault(), { once: true })
    c.click()
    const afterCancelled = c.checked
    document.getElementById('l').click()
    const afterLabel = c.checked
    f.addEventListener('submit', (e) => { e.preventDefault(); log.push('submit ' + (e.submitter && e.submitter.id) + ' ' + new FormData(f).get('t')) })
    document.getElementById('s').click()
    return { log, afterClick, afterCancelled, afterLabel }
  })()`)
  assert.deepEqual(result, { log: ['input', 'change true', 'input', 'change false', 'submit s v'], afterClick: true, afterCancelled: true, afterLabel: false })
})

test('hover: a removed target hands its boundary to the nearest connected ancestor', async () => {
  using web = await page('<!doctype html><html><body><nav id="dock"><span id="ico"><b id="art">x</b></span></nav><div id="away"></div></body></html>')
  const id = (name: string) => web.internals.idOf(web.run(`document.getElementById("${name}")`))
  web.dom.boxes.set(id('dock'), { x: 0, y: 0, width: 200, height: 100 })
  web.dom.boxes.set(id('ico'), { x: 10, y: 10, width: 50, height: 50 })
  web.dom.boxes.set(id('art'), { x: 10, y: 10, width: 50, height: 50 })
  web.dom.boxes.set(id('away'), { x: 0, y: 300, width: 200, height: 100 })
  web.run(`
    globalThis.log = []
    for (const type of ['mouseover', 'mouseout', 'mouseenter', 'mouseleave'])
      document.addEventListener(type, (e) => log.push(type + ':' + (e.target.id || e.target.nodeName)), true)
  `)
  const input = web.internals.input
  input.pointer('pointermove', 20, 20)
  web.run('log.length = 0; document.getElementById("ico").innerHTML = "<b id=art2>y</b>"')
  web.dom.boxes.set(id('art2'), { x: 10, y: 10, width: 50, height: 50 })
  input.pointer('pointermove', 21, 20)
  assert.deepEqual(web.json('log'), ['mouseout:ico', 'mouseover:art2', 'mouseenter:art2'])
  web.run('log.length = 0')
  input.leave()
  assert.deepEqual(web.json('log'), ['mouseout:art2', 'mouseleave:art2', 'mouseleave:ico', 'mouseleave:dock', 'mouseleave:BODY', 'mouseleave:HTML'])
})

test('input routing: hover, compat mouse events, click count, focus on press, keys to the focused element', async () => {
  using web = await page('<!doctype html><html><body><div id="a"><button id="btn">b</button></div><input id="field"></body></html>')
  const a = web.internals.idOf(web.run('document.getElementById("a")'))
  const btn = web.internals.idOf(web.run('document.getElementById("btn")'))
  const field = web.internals.idOf(web.run('document.getElementById("field")'))
  web.dom.boxes.set(a, { x: 0, y: 0, width: 200, height: 100 })
  web.dom.boxes.set(btn, { x: 10, y: 10, width: 50, height: 20 })
  web.dom.boxes.set(field, { x: 0, y: 200, width: 100, height: 20 })
  web.run(`
    globalThis.log = []
    for (const type of ['pointerover', 'pointerenter', 'mouseover', 'mouseenter', 'pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click', 'dblclick', 'pointerleave', 'focus', 'keydown', 'keypress', 'keyup'])
      document.addEventListener(type, (e) => log.push(type + ':' + (e.target.id || e.target.nodeName) + (type === 'click' || type === 'mousedown' ? ':' + e.detail : '')), true)
  `)
  const input = web.internals.input
  input.pointer('pointermove', 20, 15)
  input.pointer('pointerdown', 20, 15)
  input.pointer('pointerup', 20, 15)
  input.pointer('pointerdown', 20, 15)
  input.pointer('pointerup', 20, 15)
  const log = web.json<string[]>('log')
  assert.deepEqual(log.slice(0, 2), ['pointerover:btn', 'mouseover:btn'])
  assert.deepEqual(log.filter((e) => e.startsWith('pointerenter')), ['pointerenter:HTML', 'pointerenter:BODY', 'pointerenter:a', 'pointerenter:btn'])
  assert.ok(log.includes('focus:btn'))
  assert.ok(log.includes('click:btn:1') && log.includes('click:btn:2') && log.includes('dblclick:btn'))
  assert.ok(log.indexOf('pointerdown:btn') < log.indexOf('mousedown:btn:1'))
  assert.equal(web.run('document.activeElement.id'), 'btn')
  assert.equal(web.dom.focus.id, btn)
  input.pointer('pointerdown', 10, 205)
  input.pointer('pointerup', 10, 205)
  assert.equal(web.run('document.activeElement.id'), 'field')
  web.run('log.length = 0')
  input.key('keydown', 'a', 'KeyA')
  input.key('keyup', 'a', 'KeyA')
  assert.deepEqual(web.json('log'), ['keydown:field', 'keypress:field', 'keyup:field'])
  web.run('document.addEventListener("keydown", (e) => globalThis.lastKey = e.keyCode + e.key, { once: true })')
  input.key('keydown', 'Enter', 'Enter')
  assert.equal(web.run('lastKey'), '13Enter')
  input.key('keydown', 'Tab', 'Tab', { modifiers: { shift: true } })
  assert.equal(web.run('document.activeElement.id'), 'btn')
  assert.equal(web.dom.focus.visible, true)
})

test('focused HTML buttons activate with Enter and Space', async () => {
  using web = await page('<form><button id="btn">Submit</button><input id="other"></form>')
  for (const name of ['btn', 'other']) {
    const id = web.internals.idOf(web.run(`document.getElementById('${name}')`))
    web.dom.boxes.set(id, { x: 0, y: 0, width: 80, height: 30 })
  }
  web.run(`
    const btn = document.getElementById('btn'), other = document.getElementById('other')
    const activations = []
    btn.addEventListener('click', (e) => activations.push([e.type, e.detail, e.isTrusted, e.pointerId]))
    btn.form.addEventListener('submit', (e) => { e.preventDefault(); activations.push([e.type, e.submitter.id]) })
    btn.focus()
  `)
  const input = web.internals.input
  const log = () => web.json('activations')
  const clickAndSubmit = [['click', 0, true, -1], ['submit', 'btn']]
  input.key('keydown', 'Enter', 'Enter')
  assert.deepEqual(log(), clickAndSubmit)
  input.key('keyup', 'Enter', 'Enter')
  assert.deepEqual(log(), clickAndSubmit)
  web.run('activations.length = 0')

  input.key('keydown', ' ', 'Space')
  input.key('keydown', ' ', 'Space', { repeat: true })
  assert.deepEqual(log(), [])
  input.key('keyup', ' ', 'Space')
  input.key('keyup', ' ', 'Space')
  assert.deepEqual(log(), clickAndSubmit)
  web.run('activations.length = 0')

  for (const [key, code] of [['Enter', 'Enter'], [' ', 'Space']] as const) {
    web.run('btn.addEventListener("keydown", (e) => e.preventDefault(), { once: true })')
    input.key('keydown', key, code)
    input.key('keyup', key, code)
  }
  input.key('keydown', ' ', 'Space')
  web.run('btn.addEventListener("keyup", (e) => e.preventDefault(), { once: true })')
  input.key('keyup', ' ', 'Space')
  input.key('keydown', ' ', 'Space')
  web.run('other.focus(); btn.focus()')
  input.key('keyup', ' ', 'Space')
  input.key('keydown', ' ', 'Space')
  web.run('btn.disabled = true')
  input.key('keyup', ' ', 'Space')
  input.key('keydown', 'Enter', 'Enter')
  assert.deepEqual(log(), [])
})

test('selecting text with the pointer: drag, double and triple press, Shift, and what stops it', async () => {
  using web = await page('<!doctype html><html><body><p id="p">Hello brave new world</p><p id="q" style="user-select: none">fixed</p><button id="b">press</button></body></html>')
  const text = web.internals.idOf(web.run('document.getElementById("p").firstChild'))
  const p = web.internals.idOf(web.run('document.getElementById("p")'))
  web.dom.boxes.set(p, { x: 0, y: 0, width: 300, height: 20 })
  web.dom.boxes.set(web.internals.idOf(web.run('document.getElementById("b")')), { x: 0, y: 100, width: 50, height: 20 })
  web.dom.boxes.set(web.internals.idOf(web.run('document.getElementById("q")')), { x: 0, y: 50, width: 300, height: 20 })
  web.dom.caret = (x, y) => (y < 20 ? { node: text, offset: Math.min(21, Math.round(x / 10)) } : null)
  const input = web.internals.input
  const selected = () => web.run('getSelection().toString()')
  web.run('globalThis.starts = 0; document.addEventListener("selectstart", () => starts++)')

  input.pointer('pointerdown', 0, 5)
  input.pointer('pointermove', 30, 5)
  input.pointer('pointermove', 50, 5)
  input.pointer('pointerup', 50, 5)
  assert.equal(selected(), 'Hello')
  assert.equal(web.run('starts'), 1)

  input.pointer('pointerdown', 140, 5, { modifiers: { shift: true } })
  input.pointer('pointerup', 140, 5, { modifiers: { shift: true } })
  assert.equal(selected(), 'Hello brave ne')

  input.pointer('pointerdown', 80, 5)
  input.pointer('pointerup', 80, 5)
  assert.equal(web.run('getSelection().isCollapsed'), true)
  input.pointer('pointerdown', 80, 5)
  input.pointer('pointerup', 80, 5)
  assert.equal(selected(), 'brave')
  input.pointer('pointerdown', 80, 5)
  input.pointer('pointerup', 80, 5)
  assert.equal(selected(), 'Hello brave new world')

  input.pointer('pointerdown', 10, 105)
  input.pointer('pointerup', 10, 105)
  assert.equal(selected(), 'Hello brave new world', 'pressing a button keeps the selection')
  input.pointer('pointerdown', 10, 55)
  input.pointer('pointerup', 10, 55)
  assert.equal(selected(), 'Hello brave new world', 'user-select: none does not select')

  web.run('document.addEventListener("mousedown", (e) => e.preventDefault(), { once: true })')
  input.pointer('pointerdown', 200, 5)
  input.pointer('pointermove', 210, 5)
  input.pointer('pointerup', 210, 5)
  assert.equal(selected(), 'Hello brave new world', 'a cancelled mousedown does not select')

  web.run('document.addEventListener("selectstart", (e) => e.preventDefault(), { once: true })')
  input.pointer('pointerdown', 1000, 5)
  input.pointer('pointerup', 1000, 5)
  assert.equal(selected(), 'Hello brave new world', 'a cancelled selectstart does not select')
})

test('range inputs: pressing and dragging sets the value, keys step it; input while it moves, change when done', async () => {
  using web = await page('<!doctype html><html><body><input type="range" id="r" min="0" max="10"><input type="range" id="off" disabled></body></html>')
  const r = web.internals.idOf(web.run('document.getElementById("r")'))
  web.dom.boxes.set(r, { x: 0, y: 0, width: 116, height: 16 })
  web.run('globalThis.log = []; const r = document.getElementById("r"); for (const t of ["input", "change"]) r.addEventListener(t, () => log.push(t + ":" + r.value))')
  const input = web.internals.input
  input.pointer('pointerdown', 38, 8)
  input.pointer('pointermove', 68, 8)
  input.pointer('pointermove', 500, 8)
  input.pointer('pointerup', 500, 8)
  assert.deepEqual(web.json('log'), ['input:3', 'input:6', 'input:10', 'change:10'])
  assert.equal(web.run('document.activeElement.id'), 'r')
  web.run('log.length = 0')
  input.key('keydown', 'ArrowLeft', 'ArrowLeft')
  input.key('keydown', 'Home', 'Home')
  input.key('keydown', 'Home', 'Home')
  assert.deepEqual(web.json('log'), ['input:9', 'change:9', 'input:0', 'change:0'])
})

test('range inputs change their value the way the user does, past a page that tracks the value property (React)', async () => {
  using web = await page('<!doctype html><html><body><input type="range" id="r" min="0" max="10" value="5"></body></html>')
  const r = web.internals.idOf(web.run('document.getElementById("r")'))
  web.dom.boxes.set(r, { x: 0, y: 0, width: 116, height: 16 })
  web.run(`globalThis.log = []; const r = document.getElementById("r")
    const own = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')
    let tracked = r.value
    Object.defineProperty(r, 'value', { configurable: true, get() { return own.get.call(this) }, set(v) { tracked = String(v); own.set.call(this, v) } })
    r.addEventListener('input', () => log.push(r.value === tracked ? 'unchanged' : 'changed:' + r.value))`)
  web.internals.input.pointer('pointerdown', 88, 8)
  web.internals.input.pointer('pointerup', 88, 8)
  assert.deepEqual(web.json('log'), ['changed:8'])
})
