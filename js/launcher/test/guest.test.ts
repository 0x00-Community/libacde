import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { GuestWindows, type GuestRow } from '../src/dev/guest.ts'
import { Inspect, type InspectHost } from '../src/dev/inspect.ts'

const row = (id: number, pid: number, extra: Partial<GuestRow> = {}): GuestRow => ({
  id, app_id: `app${id}`, pid, title: `T${id}`, x: 0, y: 0, width: 100, height: 80, activated: false, hidden: false, desktop: false, ...extra,
})

function base(local: number, focused: number[]): InspectHost {
  return {
    shell: { focus: (w) => void focused.push(w), windows: () => [], inject: () => {} },
    web: () => null,
    record: (w) => (w === local ? { kind: 'web', app: 'files', title: 'Files' } : null),
    panel: () => false,
    close: async () => undefined,
    hide: () => {},
    open: () => Promise.reject(new Error('no open')),
    accessibility: async () => null,
    records: () => [local],
  }
}

describe('the window table the session manager sends along', () => {
  test('without one the host table is used; with one the winman ids name the windows', () => {
    const focused: number[] = []
    const guest = new GuestWindows(42, false)
    const inspect = new Inspect(guest.host(base(1, focused), () => 1))
    assert.deepEqual(inspect.windows(), [])
    const desktop = { id: 0, desktop: true, app_id: null, pid: 10, title: 'desktop', activated: false, hidden: false }
    guest.place([row(7, 42, { activated: true }), { ...row(9, 50), desk: 2, desktop: undefined }, desktop, { bogus: true }])
    const windows = inspect.windows().map((w) => [w.ref, w.kind, w.app, w.title, w.focus])
    assert.deepEqual(windows, [
      ['w7', 'web', 'files', 'Files', true],
      ['w9', 'native', 'app9', 'T9', false],
      ['w0', 'desktop', null, null, false],
    ])
    assert.equal(guest.mine(), 7)
  })

  test('the own window maps back to the local one; the desktop process owns w0', () => {
    const focused: number[] = []
    const app = new GuestWindows(42, false)
    const host = app.host(base(1, focused), () => 1)
    app.place([row(7, 42), row(9, 50)])
    host.shell.focus(7)
    host.shell.focus(9)
    assert.deepEqual(focused, [1, 9])
    assert.deepEqual(host.records(), [7])
    const desktop = new GuestWindows(10, true)
    desktop.place([row(7, 42), { id: 0, desktop: true, pid: 10 }])
    assert.equal(desktop.mine(), 0)
  })
})

describe('pointer and keys in the guest', () => {
  test('go to the manager once a window table came, before that to the local shell', () => {
    const local: object[] = []
    const sent: object[] = []
    const guest = new GuestWindows(42, false)
    const plain = base(1, [])
    const host = guest.host({ ...plain, shell: { ...plain.shell, inject: (e) => void local.push(e) } }, () => 1, (e) => void sent.push(e))
    host.shell.inject({ type: 'move', x: 1, y: 2 })
    guest.place([row(7, 42)])
    host.shell.inject({ type: 'down', x: 1, y: 2, button: 0x110 })
    assert.deepEqual(local, [{ type: 'move', x: 1, y: 2 }])
    assert.deepEqual(sent, [{ type: 'down', x: 1, y: 2, button: 0x110 }])
  })
})
