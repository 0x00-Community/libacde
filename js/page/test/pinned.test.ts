import assert from 'node:assert/strict'
import { test } from 'node:test'
import { Pinned } from '../src/pinned.ts'
import type { Rpc } from '../src/rpc.ts'

type Call = readonly [string, ...unknown[]]

function harness(opacity = '1') {
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
  const pinned = new Pinned(rpc, surface as never, dom, page as never)
  return { calls, listeners, style, element, pinned, answer: () => answer! }
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
  assert.deepEqual(calls.find((c) => c[0] === 'openPanel'), ['openPanel', 7, 5n, 10, 20, 100, 50])
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
