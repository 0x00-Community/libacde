import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { WindowEvent } from '../src/native.ts'
import { WindowList } from '../src/remote.ts'
import { Rpc } from '../src/rpc.ts'

const list = () => new WindowList(new Rpc(() => {}))
const event = (type: WindowEvent['type'], window: number, fields: Record<string, unknown> = {}): WindowEvent => ({ type, window, ...fields })

test('every row carries the state of its window, from the first event on', () => {
  const windows = list()
  windows.deliver(event('windowopen', 7, { appId: 'notes', title: 'Notes', web: true }))
  windows.deliver(event('windowopen', 8, { appId: 'org.gnome.Calculator', title: 'Calculator' }))
  assert.deepEqual(windows.api.list(), [
    { id: 7, appId: 'notes', title: 'Notes', web: true, state: 'normal' },
    { id: 8, appId: 'org.gnome.Calculator', title: 'Calculator', state: 'normal' },
  ])
  windows.deliver(event('windowstate', 7, { state: 'filled', previous: 'normal' }))
  windows.deliver(event('windowstate', 8, { state: 'minimized', previous: 'normal' }))
  assert.deepEqual(windows.api.list().map((row) => row.state), ['filled', 'minimized'])
  windows.deliver(event('windowtitle', 7, { appId: 'notes', title: 'Draft' }))
  assert.deepEqual(windows.api.list()[0], { id: 7, appId: 'notes', title: 'Draft', web: true, state: 'filled' }, 'a new title keeps the state')
  const seen: unknown[] = []
  windows.api.on('state', (change) => void seen.push(change))
  windows.deliver(event('windowstate', 7, { state: 'normal', previous: 'filled' }))
  assert.equal(windows.api.list()[0]!.state, 'normal')
  assert.deepEqual(seen, [{ type: 'windowstate', window: 7, state: 'normal', previous: 'filled' }], 'and the handlers still get the event')
})

test('only open, title, state and close change the rows', () => {
  const windows = list()
  windows.deliver(event('windowopen', 7, { appId: 'notes', title: 'Notes', web: true }))
  windows.deliver(event('windowstate', 9, { state: 'filled', previous: 'normal' }))
  windows.deliver(event('windowstate', 7, { state: 'sideways', previous: 'normal' }))
  for (const type of ['windowactivate', 'windowmenus', 'windowready', 'windowattention'] as const) windows.deliver(event(type, 11))
  assert.deepEqual(windows.api.list(), [{ id: 7, appId: 'notes', title: 'Notes', web: true, state: 'normal' }])
  windows.deliver(event('windowclose', 7))
  assert.deepEqual(windows.api.list(), [])
})

test('a window whose process crashed leaves the list, and the crash is still said', () => {
  const windows = list()
  windows.deliver(event('windowopen', 10, { appId: 'files', title: 'Files', web: true }))
  const crashes: unknown[] = []
  windows.api.on('crash', (crash) => void crashes.push(crash))
  windows.deliver(event('windowcrash', 10, { reason: 'killed', signal: 9 }))
  assert.deepEqual(windows.api.list(), [])
  assert.deepEqual(crashes, [{ type: 'windowcrash', window: 10, reason: 'killed', signal: 9 }])
})

test('a winman snapshot updates the row, is said as window, and still says the old events', () => {
  const windows = list()
  const said: unknown[] = []
  for (const name of ['window', 'open', 'title', 'state', 'activate', 'geometry', 'desk', 'parent'] as const) {
    windows.api.on(name, (change) => void said.push(change))
  }
  const rect = { x: 10, y: 20, width: 300, height: 200 }
  const first = { appId: 'files', title: 'Files', pid: 40, instance: 1, state: 'normal', hidden: false, rect, active: true, desk: 0, first: true }
  windows.deliver(event('window', 4, first))
  assert.deepEqual(windows.api.list(), [{ id: 4, appId: 'files', title: 'Files', state: 'normal', rect, instance: 1, desk: 0 }])
  assert.deepEqual(said.map((e) => (e as { type: string }).type), ['window', 'windowopen', 'windowactivate', 'windowgeometry'])
  said.length = 0
  const filled = { ...first, state: 'filled', previous: 'normal', first: false, changed: ['state', 'desk'], desk: 2, active: false }
  windows.deliver(event('window', 4, filled))
  assert.equal(windows.api.list()[0]!.state, 'filled')
  assert.deepEqual(said.slice(1), [{ type: 'windowstate', window: 4, state: 'filled', previous: 'normal' }, { type: 'windowdesk', window: 4, desk: 2 }])
})
