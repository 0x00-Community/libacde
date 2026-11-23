import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { Inspect, type InspectHost, type ShellWindow } from '../../src/dev/inspect.ts'
import type { Rpc } from '../../../page/src/rpc.ts'
import type { PageMatch } from '../../../page/src/pageinspect.ts'

const row = (id: number, z: number, rect: [number, number, number, number], extra: Partial<ShellWindow> = {}): ShellWindow => ({
  id, window: id, canvas: 0, mode: 'window', x: rect[0], y: rect[1], width: rect[2], height: rect[3], z, focus: false, ...extra,
})

const match = (id: number, role: string, name: string, tag: string): PageMatch => ({ id, role, name, tag, rect: [0, 0, 10, 10], visible: true })

function host(rows: ShellWindow[], answer: (window: number, x: number, y: number) => unknown, panels: number[] = []): InspectHost {
  const rpc = (window: number): Rpc => ({ call: async (_s: string, _m: string, [x, y]: number[]) => answer(window, x!, y!) }) as unknown as Rpc
  return {
    shell: { focus: () => {}, windows: () => rows, inject: () => {}, play: async () => {} },
    web: (window) => (window === 3 || window === 5 ? rpc(window) : null),
    record: (window) => (window === 0 || panels.includes(window) ? null : { kind: window === 6 ? 'native' : 'web', app: `app${window}`, title: `T${window}` }),
    panel: (window) => panels.includes(window),
    close: async () => undefined,
    hide: () => {},
    open: () => Promise.reject(new Error('no open')),
    accessibility: async () => null,
    records: () => rows.map((r) => r.id),
    ref: (window) => `w${window}`,
  }
}

describe('what lies under a point of the screen', () => {
  test('the topmost window answers with the element, its chain and what the point passed through', async () => {
    const asked: number[][] = []
    const answer = (window: number, x: number, y: number) => {
      asked.push([window, x, y])
      return { target: match(12, 'button', 'Save', 'button'), chain: [match(4, 'main', '', 'main')], through: [match(20, 'generic', 'veil', 'div')] }
    }
    const inspect = new Inspect(host([row(3, 1, [0, 0, 400, 300]), row(5, 2, [100, 100, 200, 200])], answer))
    const hit = await inspect.hit(150, 140)
    assert.deepEqual(asked, [[5, 50, 40]])
    assert.deepEqual(hit.window, { ref: 'w5', app: 'app5', title: 'T5' })
    assert.equal(hit.surface, 'page')
    assert.deepEqual(hit.local, [50, 40])
    assert.equal(hit.element?.ref, 'w5.12')
    assert.equal(hit.element?.line, 'button "Save" [w5.12]')
    assert.deepEqual(hit.element?.chain.map((e) => e.ref), ['w5.4'])
    assert.deepEqual(hit.element?.through.map((e) => [e.ref, e.tag]), [['w5.20', 'div']])
    assert.equal((await inspect.hit(20, 20)).window?.ref, 'w3')
  })

  test('the title bar, a native window, a panel, nothing, and a page that went away', async () => {
    const rows = [
      row(0, -1, [0, 0, 800, 600]), row(3, 1, [0, 0, 400, 300]), row(5, 2, [500, 0, 100, 100]),
      row(6, 3, [0, 400, 100, 100]), row(9, 9, [700, 500, 100, 100]),
    ]
    const answer = (window: number) => {
      if (window === 5) throw new Error('the page went away')
      return { decoration: true }
    }
    const inspect = new Inspect(host(rows, answer, [9]))
    assert.equal((await inspect.hit(10, 10)).surface, 'decoration')
    assert.equal((await inspect.hit(550, 50)).gone, true)
    assert.equal((await inspect.hit(50, 450)).surface, 'native')
    const panel = await inspect.hit(750, 550)
    assert.deepEqual([panel.surface, panel.window?.ref, panel.local], ['panel', 'w0', [50, 50]])
    assert.deepEqual(await new Inspect(host([], answer)).hit(1, 1), { x: 1, y: 1, window: null, surface: 'none' })
  })
})
