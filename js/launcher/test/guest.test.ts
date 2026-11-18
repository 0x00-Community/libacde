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
    guest.place([row(3, 10, { desktop: true }), row(9, 50), row(7, 42, { activated: true }), { bogus: true }])
    const windows = inspect.windows().map((w) => [w.ref, w.kind, w.app, w.title, w.focus])
    assert.deepEqual(windows, [
      ['w9', 'native', 'app9', 'T9', false],
      ['w7', 'web', 'files', 'Files', true],
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
    desktop.place([row(3, 10, { desktop: true }), row(7, 42)])
    assert.equal(desktop.mine(), 0)
  })
})
