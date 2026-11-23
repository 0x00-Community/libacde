import assert from 'node:assert/strict'
import { test } from 'node:test'
import { Pinned } from '../src/pinned.ts'
import type { Rpc } from '../src/rpc.ts'

type Call = readonly [string, ...unknown[]]

function harness(opacity = '1', anchors = false) {
  const calls: Call[] = []
  const listeners = new Map<string, () => void>()
  let answer: { resolve: (value: unknown) => void; reject: (error: Error) => void } | null = null
  const style = { pointerEvents: 'auto', visibility: 'visible', opacity }
  const element = {
    isConnected: true,
    parentElement: null,
    getBoundingClientRect: () => ({ left: 10, top: 20, right: 110, bottom: 70, width: 100, height: 50 }),
    addEventListener: (type: string, fn: () => void) => listeners.set(type, fn),
    removeEventListener: (type: string) => listeners.delete(type),
  }
  class Observer {
    observe(): void {}
    disconnect(): void {}
  }
  const page = {
    window: {
      innerWidth: 800,
      innerHeight: 600,
      devicePixelRatio: 1,
      ResizeObserver: Observer,
      MutationObserver: Observer,
      addEventListener: () => {},
      requestAnimationFrame: () => 0,
      getComputedStyle: () => style,
    },
    idOf: () => 7,
  }
  const rpc = {
    call: (service: string, method: string, args: unknown[]) => {
      calls.push([`rpc ${service}.${method}`, ...args])
      return service === 'panel' && method === 'open'
        ? new Promise((resolve, reject) => {
          answer = { resolve, reject }
        })
        : Promise.resolve(undefined)
    },
  } as unknown as Rpc
  const surface = {
    openPanel: (...args: unknown[]) => calls.push(['openPanel', ...args]),
    placePanel: (...args: unknown[]) => calls.push(['placePanel', ...args]),
    closePanel: (...args: unknown[]) => calls.push(['closePanel', ...args]),
    reservePanel: (...args: unknown[]) => calls.push(['reservePanel', ...args]),
  }
  const dom = { setPanel: (...args: number[]) => calls.push(['setPanel', ...args]) }
  const pinned = new Pinned(rpc, surface as never, dom, page as never, anchors)
  return { calls, listeners, style, element, pinned, page, answer: () => answer! }
}

test('a pinned element leaves the page plane before the panel opens, so it is never shown where no input reaches it', () => {
  const { calls, element, pinned } = harness()
  pinned.pin(element as unknown as Element)
  assert.deepEqual(calls, [['setPanel', 7, 10, 20, 100, 50], ['rpc panel.open', [10, 20, 100, 50]]])
})

test('once the panel opens it is placed with the input box of the element', async () => {
  const { calls, element, pinned, answer } = harness()
  pinned.pin(element as unknown as Element)
  answer().resolve({ window: 1048577, token: 5n })
  await Promise.resolve()
  assert.deepEqual(calls.find((c) => c[0] === 'openPanel'), ['openPanel', 7, 5n, 10, 20, 100, 50, false, false])
  assert.deepEqual(calls.filter((c) => c[0] === 'placePanel').at(-1), ['placePanel', 7, 10, 20, 100, 50, 0, 0, 100, 50])
})

test('a panel that could not open goes back into the page plane', async () => {
  const { calls, element, pinned, answer } = harness()
  pinned.pin(element as unknown as Element)
  answer().reject(new Error('refused'))
  await Promise.resolve()
  await Promise.resolve()
  assert.deepEqual(calls.at(-1), ['setPanel', 7, 0, 0, 0, 0])
})

test('the end of a fade-in places the panel again, so its input box is not left empty', async () => {
  const { calls, listeners, style, element, pinned, answer } = harness('0')
  pinned.pin(element as unknown as Element)
  answer().resolve({ window: 1048577, token: 5n })
  await Promise.resolve()
  assert.deepEqual(calls.filter((c) => c[0] === 'placePanel').at(-1), ['placePanel', 7, 10, 20, 100, 50, 0, 0, 0, 0])
  style.opacity = '1'
  listeners.get('animationend')!()
  assert.deepEqual(calls.filter((c) => c[0] === 'placePanel').at(-1), ['placePanel', 7, 10, 20, 100, 50, 0, 0, 100, 50])
})

test('keyboard: exclusive opens the panel with the keyboard all to itself; on-demand is the default', async () => {
  const exclusive = harness()
  exclusive.pinned.pin(exclusive.element as unknown as Element, { keyboard: 'exclusive' })
  exclusive.answer().resolve({ window: 1048577, token: 5n })
  await Promise.resolve()
  assert.deepEqual(exclusive.calls.find((c) => c[0] === 'openPanel'), ['openPanel', 7, 5n, 10, 20, 100, 50, true, false])
  const onDemand = harness()
  onDemand.pinned.pin(onDemand.element as unknown as Element, { keyboard: 'on-demand' })
  onDemand.answer().resolve({ window: 1048577, token: 5n })
  await Promise.resolve()
  assert.deepEqual(onDemand.calls.find((c) => c[0] === 'openPanel'), ['openPanel', 7, 5n, 10, 20, 100, 50, false, false])
})

test('userOnly: the panel opens marked as a card only the user may answer; ordinary panels are not', async () => {
  const card = harness()
  card.pinned.pin(card.element as unknown as Element, { keyboard: 'exclusive', userOnly: true })
  card.answer().resolve({ window: 1048577, token: 5n })
  await Promise.resolve()
  assert.deepEqual(card.calls.find((c) => c[0] === 'openPanel'), ['openPanel', 7, 5n, 10, 20, 100, 50, true, true])
  const overlay = harness()
  overlay.pinned.pin(overlay.element as unknown as Element, { keyboard: 'exclusive', userOnly: false })
  overlay.answer().resolve({ window: 1048577, token: 5n })
  await Promise.resolve()
  assert.deepEqual(overlay.calls.find((c) => c[0] === 'openPanel'), ['openPanel', 7, 5n, 10, 20, 100, 50, true, false])
})

test('userOnly: anything but a boolean is a TypeError and pins nothing', () => {
  const { calls, element, pinned } = harness()
  assert.throws(() => pinned.pin(element as unknown as Element, { userOnly: 'yes' as never }), TypeError)
  assert.deepEqual(calls, [])
})

test('keyboard: anything else is a TypeError and pins nothing', () => {
  const { calls, element, pinned } = harness()
  assert.throws(() => pinned.pin(element as unknown as Element, { keyboard: 'grab' as never }), TypeError)
  assert.deepEqual(calls, [])
})

test('after the viewport shrinks, an extent drawn before it is dropped and the panel is the element again', async () => {
  const { calls, element, pinned, answer, page } = harness()
  const handle = pinned.pin(element as unknown as Element)
  answer().resolve({ window: 1048577, token: 5n })
  await Promise.resolve()
  pinned.extend(7, [0, 0, 700, 90])
  assert.deepEqual(calls.filter((c) => c[0] === 'placePanel').at(-1)?.slice(0, 6), ['placePanel', 7, 0, 0, 700, 90])
  page.window.innerWidth = 533
  page.window.devicePixelRatio = 1.5
  element.getBoundingClientRect = () => ({ left: 200, top: 20, right: 300, bottom: 70, width: 100, height: 50 })
  handle.update()
  assert.deepEqual(calls.filter((c) => c[0] === 'placePanel').at(-1), ['placePanel', 7, 200, 20, 100, 50, 0, 0, 100, 50])
})

const ANCHOR = { window: 12, x: 40, y: 8 }
const WIRE = { window: 12, x: 40, y: 8, width: 0, height: 0, edge: 'bottom', align: 'start', adjust: ['flip', 'slide'], visible: [0, 0, 100, 50] }

test('anchor: without anchored panels in the window system pin throws NotSupportedError, with reserve or an exclusive keyboard TypeError', () => {
  const plain = harness()
  assert.throws(() => plain.pinned.pin(plain.element as unknown as Element, { anchor: ANCHOR }), { name: 'NotSupportedError' })
  const { element, pinned, calls } = harness('1', true)
  assert.throws(() => pinned.pin(element as unknown as Element, { anchor: ANCHOR, reserve: { edge: 'top' } }), TypeError)
  assert.throws(() => pinned.pin(element as unknown as Element, { anchor: ANCHOR, keyboard: 'exclusive' }), TypeError)
  assert.throws(() => pinned.pin(element as unknown as Element, { anchor: { ...ANCHOR, edge: 'middle' as never } }), /anchor.edge is one of/)
  assert.throws(() => pinned.pin(element as unknown as Element, { anchor: { ...ANCHOR, window: 1.5 } }), /anchor.window/)
  assert.deepEqual(calls, [])
})

test('anchor: the panel opens with the anchor filled in and the element box as what is visible; a new anchor goes to the window system', async () => {
  const { calls, element, pinned, answer } = harness('1', true)
  const handle = pinned.pin(element as unknown as Element, { anchor: ANCHOR })
  assert.deepEqual(calls.find((c) => c[0] === 'rpc panel.open'), ['rpc panel.open', [10, 20, 100, 50], WIRE])
  answer().resolve({ window: 1048577, token: 5n })
  await Promise.resolve()
  assert.equal(calls.filter((c) => c[0] === 'rpc panel.anchor').length, 0)
  handle.anchor!({ ...ANCHOR, edge: 'top', align: 'end', adjust: [] })
  assert.deepEqual(calls.filter((c) => c[0] === 'rpc panel.anchor').at(-1), [
    'rpc panel.anchor', 1048577, { ...WIRE, edge: 'top', align: 'end', adjust: [] },
  ])
})

test('closed: unpinned after unpin, parent-closed or refused when the window system ends the panel, refused when it does not open', async () => {
  const plain = harness()
  const pinnedPlain = plain.pinned.pin(plain.element as unknown as Element)
  pinnedPlain.unpin()
  assert.equal(await pinnedPlain.closed, 'unpinned')
  const anchored = harness('1', true)
  const handle = anchored.pinned.pin(anchored.element as unknown as Element, { anchor: ANCHOR })
  anchored.answer().resolve({ window: 1048577, token: 5n })
  await Promise.resolve()
  anchored.pinned.ended(7, 'parent-closed')
  assert.equal(await handle.closed, 'parent-closed')
  assert.ok(anchored.calls.some((c) => c[0] === 'closePanel'))
  assert.deepEqual(anchored.calls.filter((c) => c[0] === 'rpc panel.close').at(-1), ['rpc panel.close', 1048577])
  const refused = harness('1', true)
  const never = refused.pinned.pin(refused.element as unknown as Element, { anchor: ANCHOR })
  refused.answer().reject(new Error('no'))
  assert.equal(await never.closed, 'refused')
})
