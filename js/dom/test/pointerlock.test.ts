import assert from 'node:assert/strict'
import { test } from 'node:test'
import { page, type Web } from './harness.ts'

const PAGE = '<!doctype html><html><body><canvas id="view"></canvas><div id="other"></div><div id="host"></div></body></html>'

function gesture(web: Web): void {
  web.internals.input.key('keydown', 'a', 'KeyA')
  web.internals.input.key('keyup', 'a', 'KeyA')
}

function setup(web: Web, supported = true, gestured = true) {
  const calls: string[] = []
  if (gestured) gesture(web)
  if (supported) web.internals.env.pointerLock = { lock: () => calls.push('lock'), unlock: () => calls.push('unlock') }
  web.run(`
    globalThis.log = []
    document.addEventListener('pointerlockchange', () => log.push('change:' + (document.pointerLockElement?.id ?? null)))
    document.addEventListener('pointerlockerror', () => log.push('error'))
    globalThis.settle = (p) => p.then(() => log.push('resolved'), (e) => log.push('rejected:' + e.name))
  `)
  return calls
}

test('pointer lock: a granted request locks the element, fires pointerlockchange and resolves', async () => {
  using web = await page(PAGE)
  const calls = setup(web)
  web.run('settle(document.getElementById("view").requestPointerLock())')
  assert.deepEqual(calls, ['lock'])
  assert.equal(web.run('document.pointerLockElement'), null)
  web.internals.pointerLock('granted')
  await web.wait()
  assert.deepEqual(web.json('log'), ['change:view', 'resolved'])
  assert.equal(web.run('document.pointerLockElement === document.getElementById("view")'), true)

  web.run('log.length = 0; document.exitPointerLock()')
  await web.wait()
  assert.deepEqual(calls, ['lock', 'unlock'])
  assert.deepEqual(web.json('log'), ['change:null'])
  assert.equal(web.run('document.pointerLockElement'), null)
})

test('pointer lock: refusals reject with the standard errors and fire pointerlockerror', async () => {
  using web = await page(PAGE)
  const calls = setup(web)
  web.run('settle(document.createElement("div").requestPointerLock())')
  web.run('settle(document.getElementById("view").requestPointerLock({ unadjustedMovement: true }))')
  await web.wait()
  assert.deepEqual(web.json('log'), ['error', 'rejected:WrongDocumentError', 'error', 'rejected:NotSupportedError'])

  web.run('log.length = 0; settle(document.getElementById("view").requestPointerLock())')
  web.run('settle(document.getElementById("view").requestPointerLock())')
  web.internals.pointerLock('denied')
  await web.wait()
  assert.deepEqual(calls, ['lock'])
  assert.deepEqual(web.json('log'), ['error', 'rejected:InvalidStateError', 'error', 'rejected:NotAllowedError'])

  web.internals.setActive(false)
  web.run('log.length = 0; settle(document.getElementById("view").requestPointerLock())')
  await web.wait()
  assert.deepEqual(web.json('log'), ['error', 'rejected:WrongDocumentError'])
})

test('pointer lock: a window without it says NotSupportedError', async () => {
  using web = await page(PAGE)
  setup(web, false)
  web.run('settle(document.getElementById("view").requestPointerLock())')
  await web.wait()
  assert.deepEqual(web.json('log'), ['error', 'rejected:NotSupportedError'])
})

test('pointer lock: while locked every pointer event goes to the element with movement and a still position', async () => {
  using web = await page(PAGE)
  setup(web)
  const id = (name: string) => web.internals.idOf(web.run(`document.getElementById("${name}")`))
  web.dom.boxes.set(id('view'), { x: 0, y: 0, width: 100, height: 100 })
  web.dom.boxes.set(id('other'), { x: 0, y: 200, width: 100, height: 100 })
  const input = web.internals.input
  input.pointer('pointermove', 50, 50)
  web.run('document.getElementById("view").requestPointerLock()')
  web.internals.pointerLock('granted')
  await web.wait()
  web.run(`
    log.length = 0
    for (const type of ['pointermove', 'mousemove', 'mousedown', 'mouseup', 'click', 'mouseover', 'mouseout'])
      document.addEventListener(type, (e) => log.push([type, e.target.id, e.clientX, e.clientY, e.movementX, e.movementY].join(' ')), true)
  `)
  input.pointer('pointermove', 50, 50, { target: id('other'), movement: [7, -3] })
  input.pointer('pointerdown', 50, 50, { target: id('other') })
  input.pointer('pointerup', 50, 50, { target: id('other') })
  assert.deepEqual(web.json('log'), [
    'pointermove view 50 50 7 -3',
    'mousemove view 50 50 7 -3',
    'mousedown view 50 50 0 0',
    'mouseup view 50 50 0 0',
    'click view 50 50 0 0',
  ])
})

test('pointer lock: the host taking it back or the element leaving the document unlocks', async () => {
  using web = await page(PAGE)
  const calls = setup(web)
  web.run('document.getElementById("view").requestPointerLock()')
  web.internals.pointerLock('granted')
  await web.wait()
  web.run('log.length = 0')
  web.internals.pointerLock('lost')
  await web.wait()
  assert.deepEqual(web.json('log'), ['change:null'])
  assert.deepEqual(calls, ['lock'])

  web.run('log.length = 0; document.getElementById("view").requestPointerLock()')
  web.internals.pointerLock('granted')
  web.run('document.getElementById("view").remove()')
  await web.wait()
  assert.deepEqual(calls, ['lock', 'lock', 'unlock'])
  assert.deepEqual(web.json('log'), ['change:null', 'change:null'])
  assert.equal(web.run('document.pointerLockElement'), null)
})

test('pointer lock: a shadow root reports the locked element inside it', async () => {
  using web = await page(PAGE)
  setup(web)
  web.run(`
    const root = document.getElementById('host').attachShadow({ mode: 'open' })
    root.innerHTML = '<canvas id="inside"></canvas>'
    root.getElementById('inside').requestPointerLock()
  `)
  web.internals.pointerLock('granted')
  await web.wait()
  assert.equal(web.run('document.getElementById("host").shadowRoot.pointerLockElement.id'), 'inside')
  web.run('document.exitPointerLock()')
  assert.equal(web.run('document.getElementById("host").shadowRoot.pointerLockElement'), null)
})

test('pointer lock: a request needs a user gesture, unless the page itself let the last lock go', async () => {
  using web = await page(PAGE)
  const calls = setup(web, true, false)
  assert.deepEqual(web.json('[navigator.userActivation.hasBeenActive, navigator.userActivation.isActive]'), [false, false])
  web.run('settle(document.getElementById("view").requestPointerLock())')
  await web.wait()
  assert.deepEqual(web.json('log'), ['error', 'rejected:NotAllowedError'])
  assert.deepEqual(calls, [])

  web.internals.input.key('keydown', 'Escape', 'Escape')
  web.run('log.length = 0; settle(document.getElementById("view").requestPointerLock())')
  await web.wait()
  assert.deepEqual(web.json('log'), ['error', 'rejected:NotAllowedError'])

  gesture(web)
  assert.deepEqual(web.json('[navigator.userActivation.hasBeenActive, navigator.userActivation.isActive, navigator.userActivation === navigator.userActivation]'), [true, true, true])
  assert.equal(web.run('Object.prototype.toString.call(navigator.userActivation)'), '[object UserActivation]')
  web.run('document.getElementById("view").requestPointerLock()')
  web.internals.pointerLock('granted')
  web.run('document.exitPointerLock()')
  await web.wait()
  web.run('log.length = 0; settle(document.getElementById("view").requestPointerLock())')
  web.internals.pointerLock('granted')
  await web.wait()
  assert.deepEqual(web.json('log'), ['change:view', 'resolved'])
  assert.deepEqual(calls, ['lock', 'unlock', 'lock'])
})

