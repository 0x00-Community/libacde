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

test('hover: with no pointer or mouse listener a move does no hit test; a listener added later starts from where the pointer was', async () => {
  using web = await page('<!doctype html><html><body><div id="a"></div><div id="b"></div></body></html>')
  const id = (name: string) => web.internals.idOf(web.run(`document.getElementById("${name}")`))
  web.dom.boxes.set(id('a'), { x: 0, y: 0, width: 100, height: 100 })
  web.dom.boxes.set(id('b'), { x: 0, y: 200, width: 100, height: 100 })
  let hits = 0
  const hitTest = web.dom.hitTest.bind(web.dom)
  web.dom.hitTest = (x: number, y: number) => {
    hits++
    return hitTest(x, y)
  }
  const input = web.internals.input
  input.pointer('pointermove', 10, 10)
  input.pointer('pointermove', 20, 20)
  assert.equal(hits, 0)
  web.run(`
    globalThis.log = []
    for (const type of ['mouseover', 'mouseout']) document.addEventListener(type, (e) => log.push(type + ':' + (e.target.id || e.target.nodeName)), true)
  `)
  input.pointer('pointermove', 30, 30)
  assert.deepEqual(web.json('log'), [], 'still over a: no boundary events')
  input.pointer('pointermove', 30, 230)
  assert.deepEqual(web.json('log'), ['mouseout:a', 'mouseover:b'])
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

test('user-select: none on an ancestor stops a selection that starts inside it, text re-enables it', async () => {
  using web = await page('<!doctype html><html><body><div id="w" style="user-select: none"><p><span id="s">chrome</span></p><p id="t" style="user-select: text">content</p></div></body></html>')
  const chrome = web.internals.idOf(web.run('document.getElementById("s").firstChild'))
  const content = web.internals.idOf(web.run('document.getElementById("t").firstChild'))
  web.dom.boxes.set(web.internals.idOf(web.run('document.getElementById("s")')), { x: 0, y: 0, width: 60, height: 20 })
  web.dom.boxes.set(web.internals.idOf(web.run('document.getElementById("t")')), { x: 0, y: 50, width: 70, height: 20 })
  web.dom.caret = (x, y) => (y < 20 ? { node: chrome, offset: Math.min(6, Math.round(x / 10)) } : { node: content, offset: Math.min(7, Math.round(x / 10)) })
  const input = web.internals.input
  const selected = () => web.run('getSelection().toString()')

  input.pointer('pointerdown', 0, 5)
  input.pointer('pointermove', 60, 5)
  input.pointer('pointerup', 60, 5)
  assert.equal(selected(), '', 'a child of user-select: none does not select')

  input.pointer('pointerdown', 0, 55)
  input.pointer('pointermove', 70, 55)
  input.pointer('pointerup', 70, 55)
  assert.equal(selected(), 'content', 'user-select: text inside it selects')
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

test('pointermove: getCoalescedEvents has every sample since the last dispatch, each with its own position, time and movement', async () => {
  using web = await page('<!doctype html><html><body><div id="pad"></div></body></html>')
  web.dom.boxes.set(web.internals.idOf(web.run('document.getElementById("pad")')), { x: 10, y: 20, width: 300, height: 200 })
  web.run(`
    globalThis.seen = []
    const pad = document.getElementById('pad')
    const sample = (s, e) => ({ x: s.clientX, ox: s.offsetX, t: s.timeStamp, mx: s.movementX, b: s.buttons, p: s.pressure, type: s.pointerType, same: s.target === e.target, trusted: s.isTrusted, bubbles: s.bubbles, nested: s.getCoalescedEvents().length })
    pad.addEventListener('pointermove', (e) => seen.push({ x: e.clientX, t: e.timeStamp, mx: e.movementX, samples: e.getCoalescedEvents().map((s) => sample(s, e)) }))
    pad.addEventListener('pointerdown', (e) => seen.push({ down: e.getCoalescedEvents().length, p: e.pressure }))
  `)
  type Sample = { x: number; ox: number; t: number; mx: number; b: number; p: number; type: string; same: boolean; trusted: boolean; bubbles: boolean; nested: number }
  type Seen = { x: number; t: number; mx: number; samples: Sample[] }
  const input = web.internals.input
  input.pointer('pointermove', 30, 40)
  const t0 = web.run('performance.now()') as number
  input.pointer('pointermove', 60, 40, { time: t0 - 1, samples: [{ x: 40, y: 40, time: t0 - 10 }, { x: 50, y: 40, time: t0 - 5 }, { x: 60, y: 40, time: t0 - 1 }] })
  const [first, moved] = web.json<Seen[]>('seen')
  assert.deepEqual(first!.samples.map((s) => s.x), [30])
  assert.deepEqual(moved!.samples.map((s) => [s.x, s.ox, s.mx, s.b, s.p, s.type, s.same, s.trusted, s.bubbles, s.nested]), [
    [40, 30, 10, 0, 0, 'mouse', true, true, false, 0],
    [50, 40, 10, 0, 0, 'mouse', true, true, false, 0],
    [60, 50, 10, 0, 0, 'mouse', true, true, false, 0],
  ])
  assert.deepEqual([moved!.x, moved!.mx], [60, 30])
  for (const [i, ago] of [10, 5, 1].entries()) assert.ok(Math.abs(moved!.samples[i]!.t - (t0 - ago)) < 0.5, `sample ${i} keeps its time`)
  assert.ok(Math.abs(moved!.t - (t0 - 1)) < 0.5, 'the dispatched event has the time of the last sample')

  web.run('seen.length = 0')
  input.pointer('pointerdown', 60, 40)
  input.pointer('pointermove', 70, 40, { samples: [{ x: 65, y: 40 }, { x: 70, y: 40 }] })
  const [down, drag] = web.json<[{ down: number; p: number }, Seen]>('seen')
  assert.deepEqual([down.down, down.p], [0, 0.5])
  assert.deepEqual(drag.samples.map((s) => [s.x, s.b, s.p]), [[65, 1, 0.5], [70, 1, 0.5]])
  input.pointer('pointerup', 70, 40)

  assert.deepEqual(web.json(`(() => {
    const a = new PointerEvent('pointermove', { clientX: 1 }), b = new PointerEvent('pointermove', { clientX: 2 })
    const errors = [[{}], null, 3].map((list) => { try { new PointerEvent('pointermove', { coalescedEvents: list }); return 'accepted' } catch (e) { return e.name } })
    return [new PointerEvent('pointermove').getCoalescedEvents().length, new PointerEvent('pointermove', { coalescedEvents: [a, b] }).getCoalescedEvents().map((e) => e.clientX), new PointerEvent('pointermove').getPredictedEvents().length, errors]
  })()`), [0, [1, 2], 0, ['TypeError', 'TypeError', 'TypeError']])
})

test('a pen stroke carries pointerType pen, its pressure, tilt and twist on every sample, and the eraser is button 5', async () => {
  using web = await page('<!doctype html><html><body><div id="pad"></div></body></html>')
  web.dom.boxes.set(web.internals.idOf(web.run('document.getElementById("pad")')), { x: 0, y: 0, width: 300, height: 200 })
  web.run(`
    globalThis.seen = []
    const pad = document.getElementById('pad')
    const fields = (e) => [e.type, e.pointerType, e.pointerId, e.button, e.buttons, e.pressure, e.tangentialPressure, e.tiltX, e.tiltY, e.twist]
    for (const type of ['pointerdown', 'pointerup']) pad.addEventListener(type, (e) => seen.push(fields(e)))
    pad.addEventListener('mousedown', (e) => seen.push([e.type, e.button, e.buttons]))
    pad.addEventListener('pointermove', (e) => seen.push([...fields(e), e.getCoalescedEvents().map((s) => [s.clientX, s.pointerType, s.pressure, s.tiltX, s.tiltY, s.twist])]))
  `)
  const input = web.internals.input
  const pen = (pressure: number, tiltX = 0, eraser = false) => ({ pressure, tangentialPressure: 0, tiltX, tiltY: -10, twist: 90, eraser })
  input.pointer('pointermove', 10, 10, { pen: pen(0) })
  input.pointer('pointerdown', 10, 10, { pen: pen(0.2) })
  input.pointer('pointermove', 30, 10, { pen: pen(0.6, 30), samples: [{ x: 20, y: 10, pen: pen(0.4, 20) }, { x: 30, y: 10, pen: pen(0.6, 30) }] })
  input.pointer('pointerup', 30, 10, { pen: pen(0) })
  input.pointer('pointerdown', 40, 10, { button: 5, pen: pen(0.5, 0, true) })
  input.pointer('pointerup', 40, 10, { button: 5, pen: pen(0, 0, true) })
  input.pointer('pointermove', 50, 10)
  input.pointer('pointerdown', 50, 10)
  input.pointer('pointerup', 50, 10)
  assert.deepEqual(web.json('seen'), [
    ['pointermove', 'pen', 2, 0, 0, 0, 0, 0, -10, 90, [[10, 'pen', 0, 0, -10, 90]]],
    ['pointerdown', 'pen', 2, 0, 1, 0.2, 0, 0, -10, 90],
    ['mousedown', 0, 1],
    ['pointermove', 'pen', 2, 0, 1, 0.6, 0, 30, -10, 90, [[20, 'pen', 0.4, 20, -10, 90], [30, 'pen', 0.6, 30, -10, 90]]],
    ['pointerup', 'pen', 2, 0, 0, 0, 0, 0, -10, 90],
    ['pointerdown', 'pen', 2, 5, 32, 0.5, 0, 0, -10, 90],
    ['mousedown', 5, 32],
    ['pointerup', 'pen', 2, 5, 0, 0, 0, 0, -10, 90],
    ['pointermove', 'mouse', 1, 0, 0, 0, 0, 0, 0, 0, [[50, 'mouse', 0, 0, 0, 0]]],
    ['pointerdown', 'mouse', 1, 0, 1, 0.5, 0, 0, 0, 0],
    ['mousedown', 0, 1],
    ['pointerup', 'mouse', 1, 0, 0, 0, 0, 0, 0, 0],
  ])
})

test('altitudeAngle and azimuthAngle follow tiltX / tiltY and back, default to π/2 and 0, and come with every pen sample', async () => {
  using web = await page('<!doctype html><html><body><div id="pad"></div></body></html>')
  web.dom.boxes.set(web.internals.idOf(web.run('document.getElementById("pad")')), { x: 0, y: 0, width: 300, height: 200 })
  const failures = web.json<string[]>(`(() => {
    const failures = []
    const P = Math.PI
    const check = (init, want) => {
      const e = new PointerEvent('pointerdown', { pointerId: 'pointer1', ...init })
      for (const [key, value] of Object.entries(want)) if (e[key] !== value) failures.push(JSON.stringify(init) + ' ' + key + ' ' + e[key] + ' != ' + value)
    }
    for (const [tiltX, tiltY, azimuthAngle, altitudeAngle] of [[0, 0, 0, P / 2], [0, 90, P / 2, 0], [0, -90, 3 * P / 2, 0], [90, 0, 0, 0], [90, 90, 0, 0],
      [90, -90, 0, 0], [-90, 0, P, 0], [-90, 90, 0, 0], [-90, -90, 0, 0], [0, 45, P / 2, P / 4], [0, -45, 3 * P / 2, P / 4], [45, 0, 0, P / 4], [-45, 0, P, P / 4]])
      check({ tiltX, tiltY }, { azimuthAngle, altitudeAngle })
    check({}, { azimuthAngle: 0, altitudeAngle: P / 2, tiltX: 0, tiltY: 0 })
    for (const [azimuthAngle, altitudeAngle, tiltX, tiltY] of [[0, 0, 90, 0], [0, P / 4, 45, 0], [0, P / 2, 0, 0], [P / 2, 0, 0, 90], [P / 2, P / 4, 0, 45],
      [P, 0, -90, 0], [P, P / 4, -45, 0], [3 * P / 2, 0, 0, -90], [3 * P / 2, P / 4, 0, -45]])
      check({ azimuthAngle, altitudeAngle }, { tiltX, tiltY })
    check({ tiltX: 45 }, { tiltX: 45, tiltY: 0, azimuthAngle: 0, altitudeAngle: P / 4 })
    check({ tiltY: 45 }, { tiltX: 0, tiltY: 45, azimuthAngle: P / 2, altitudeAngle: P / 4 })
    check({ azimuthAngle: P / 4 }, { azimuthAngle: P / 4, altitudeAngle: P / 2, tiltX: 0, tiltY: 0 })
    check({ altitudeAngle: P / 4 }, { azimuthAngle: 0, altitudeAngle: P / 4, tiltX: 45, tiltY: 0 })
    check({ tiltX: 45, azimuthAngle: P / 4 }, { tiltX: 45, tiltY: 0, azimuthAngle: P / 4, altitudeAngle: P / 2 })
    check({ tiltY: 45, altitudeAngle: P / 4 }, { tiltX: 0, tiltY: 45, azimuthAngle: 0, altitudeAngle: P / 4 })
    check({ tiltX: 30.9, tiltY: -10 }, { tiltX: 30, tiltY: -10 })
    check({ azimuthAngle: P }, { tiltX: 0, tiltY: 0 })
    try { new PointerEvent('pointerdown', { altitudeAngle: NaN }); failures.push('NaN accepted') } catch (e) { if (e.name !== 'TypeError') failures.push(e.name) }
    const keys = Object.keys(PointerEvent.prototype)
    if (!keys.includes('altitudeAngle') || !keys.includes('tiltX')) failures.push('not enumerable: ' + keys.join())
    return failures
  })()`)
  assert.deepEqual(failures, [])
  web.run(`
    globalThis.seen = []
    const pad = document.getElementById('pad')
    pad.addEventListener('pointermove', (e) => seen.push([e.pointerType, e.tiltX, e.tiltY, e.altitudeAngle, e.azimuthAngle, e.getCoalescedEvents().map((s) => [s.altitudeAngle, s.azimuthAngle])]))
  `)
  const input = web.internals.input
  const pen = (tiltX: number, tiltY: number) => ({ pressure: 0.5, tangentialPressure: 0, tiltX, tiltY, twist: 0, eraser: false })
  input.pointer('pointermove', 10, 10, { pen: pen(30, -20), samples: [{ x: 5, y: 10, pen: pen(0, 40) }, { x: 10, y: 10, pen: pen(30, -20) }] })
  input.pointer('pointermove', 20, 10)
  const rad = (degrees: number) => (degrees * Math.PI) / 180
  const altitude = Math.atan(1 / Math.sqrt(Math.tan(rad(30)) ** 2 + Math.tan(rad(-20)) ** 2))
  const azimuth = Math.atan2(Math.tan(rad(-20)), Math.tan(rad(30))) + 2 * Math.PI
  assert.deepEqual(web.json('seen'), [
    ['pen', 30, -20, altitude, azimuth, [[Math.PI / 2 - rad(40), Math.PI / 2], [altitude, azimuth]]],
    ['mouse', 0, 0, Math.PI / 2, 0, [[Math.PI / 2, 0]]],
  ])
})

test('switching between the mouse and a pen: the old pointer leaves, the new one enters, compat mouse events follow only the element', async () => {
  using web = await page('<!doctype html><html><body><div id="pad"></div><div id="other"></div></body></html>')
  const id = (name: string) => web.internals.idOf(web.run(`document.getElementById("${name}")`))
  web.dom.boxes.set(id('pad'), { x: 0, y: 0, width: 300, height: 200 })
  web.dom.boxes.set(id('other'), { x: 0, y: 200, width: 300, height: 200 })
  web.run(`
    globalThis.log = []
    const name = (t) => t ? t.id || t.nodeName : 'null'
    for (const type of ['pointerover', 'pointerout', 'pointerenter', 'pointerleave', 'mouseover', 'mouseout', 'mouseenter', 'mouseleave'])
      document.addEventListener(type, (e) => log.push([type, name(e.target), type.startsWith('pointer') ? e.pointerType + e.pointerId : '', name(e.relatedTarget)].join(':')), true)
    document.addEventListener('pointermove', (e) => log.push(['pointermove', e.pointerType, e.clientX, e.movementX, e.pressure, e.tiltX].join(':')), true)
  `)
  const input = web.internals.input
  const pen = (pressure: number, tiltX = 0) => ({ pressure, tangentialPressure: 0, tiltX, tiltY: 0, twist: 0, eraser: false })
  const take = () => web.json<string[]>('log.splice(0)')
  input.pointer('pointermove', 10, 10)
  take()
  input.pointer('pointermove', 12, 10, { pen: pen(0) })
  assert.deepEqual(take(), [
    'pointerout:pad:mouse1:null', 'pointerleave:pad:mouse1:null', 'pointerleave:BODY:mouse1:null', 'pointerleave:HTML:mouse1:null',
    'pointerover:pad:pen2:null', 'pointerenter:HTML:pen2:null', 'pointerenter:BODY:pen2:null', 'pointerenter:pad:pen2:null',
    'pointermove:pen:12:2:0:0',
  ])
  input.pointer('pointerdown', 12, 10, { pen: pen(0.2) })
  input.pointer('pointermove', 12, 10, { pen: pen(0.6) })
  input.pointer('pointermove', 12, 10, { pen: pen(0.6, 20) })
  input.pointer('pointerup', 12, 10, { pen: pen(0) })
  assert.deepEqual(take(), ['pointermove:pen:12:0:0.6:0', 'pointermove:pen:12:0:0.6:20'], 'pressure and tilt changes without motion are moves')
  input.pointer('pointermove', 14, 10)
  assert.deepEqual(take(), [
    'pointerout:pad:pen2:null', 'pointerleave:pad:pen2:null', 'pointerleave:BODY:pen2:null', 'pointerleave:HTML:pen2:null',
    'pointerover:pad:mouse1:null', 'pointerenter:HTML:mouse1:null', 'pointerenter:BODY:mouse1:null', 'pointerenter:pad:mouse1:null',
    'pointermove:mouse:14:2:0:0',
  ])
  input.pointer('pointermove', 14, 250, { pen: pen(0) })
  assert.deepEqual(take(), [
    'pointerout:pad:mouse1:null', 'pointerleave:pad:mouse1:null', 'pointerleave:BODY:mouse1:null', 'pointerleave:HTML:mouse1:null',
    'mouseout:pad::other', 'mouseleave:pad::other',
    'pointerover:other:pen2:null', 'pointerenter:HTML:pen2:null', 'pointerenter:BODY:pen2:null', 'pointerenter:other:pen2:null',
    'mouseover:other::pad', 'mouseenter:other::pad',
    'pointermove:pen:14:0:0:0',
  ])
  input.leave()
  assert.deepEqual(take(), [
    'pointerout:other:pen2:null', 'mouseout:other::null',
    'pointerleave:other:pen2:null', 'mouseleave:other::null', 'pointerleave:BODY:pen2:null', 'mouseleave:BODY::null', 'pointerleave:HTML:pen2:null', 'mouseleave:HTML::null',
  ])
  input.pointer('pointermove', 20, 20)
  assert.deepEqual(take().slice(0, 2), ['pointerover:pad:mouse1:null', 'mouseover:pad::null'], 'after leaving, the next pointer enters as before')
})

test('dragging an overlay scrollbar thumb scrolls its box and fires no pointer events at the page', async () => {
  using web = await page('<!doctype html><html><body><div id="box" data-scroll-height="1000"></div></body></html>')
  const box = web.internals.idOf(web.run('document.getElementById("box")'))
  web.dom.boxes.set(box, { x: 0, y: 0, width: 100, height: 200 })
  web.run('globalThis.downs = 0; document.addEventListener("pointerdown", () => downs++)')
  const input = web.internals.input
  web.dom.scrollbar = [box, 1, 5]
  input.pointer('pointerdown', 95, 10)
  web.dom.scrollbar = null
  input.pointer('pointermove', 95, 30)
  assert.equal(web.run('document.getElementById("box").scrollTop'), 100)
  input.pointer('pointermove', 95, 1000)
  input.pointer('pointerup', 95, 1000)
  assert.equal(web.run('document.getElementById("box").scrollTop'), 800, 'clamped to the end')
  assert.equal(web.run('downs'), 0)
  input.pointer('pointerdown', 50, 50)
  input.pointer('pointerup', 50, 50)
  assert.equal(web.run('downs'), 1, 'a press off the thumb reaches the page')
})
