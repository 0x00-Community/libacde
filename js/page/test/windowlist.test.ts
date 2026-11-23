import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { WindowEvent } from '../src/native.ts'
import { WindowList, WindowSelf } from '../src/remote.ts'
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

test('a window key from open stays on its row through later events', () => {
  const windows = list()
  windows.deliver(event('windowopen', 7, { appId: 'notes', title: 'Notes', key: 'w-3f2a' }))
  windows.deliver(event('windowtitle', 7, { appId: 'notes', title: 'Draft' }))
  windows.deliver(event('windowstate', 7, { state: 'filled', previous: 'normal' }))
  assert.equal(windows.api.list()[0]!.key, 'w-3f2a')
  windows.deliver(event('windowopen', 8, { appId: 'files', title: 'Files' }))
  assert.equal('key' in windows.api.list()[1]!, false, 'no key when the compositor gives none')
})

test('a window key from a winman snapshot is on the row and on the derived open', () => {
  const windows = list()
  const opened: unknown[] = []
  windows.api.on('open', (change) => void opened.push(change))
  const rect = { x: 0, y: 0, width: 100, height: 100 }
  const first = { appId: 'files', title: 'Files', state: 'normal', hidden: false, rect, active: false, desk: 0, first: true, key: 'w-91' }
  windows.deliver(event('window', 5, first))
  windows.deliver(event('window', 5, { ...first, title: 'Docs', first: false, changed: ['title'], key: undefined }))
  assert.equal(windows.api.list()[0]!.key, 'w-91')
  assert.equal((opened[0] as { key?: string }).key, 'w-91')
})

test('a window on the Agent stage says so in its row until the person takes it over', () => {
  const windows = list()
  const rect = { x: 0, y: 0, width: 80, height: 50 }
  windows.deliver(event('window', 9, { appId: 'Minecraft', title: 'Minecraft', state: 'normal', rect, desk: 0, stage: true, first: true }))
  assert.deepEqual(windows.api.list(), [{ id: 9, appId: 'Minecraft', title: 'Minecraft', state: 'normal', rect, desk: 0, stage: true }])
  windows.deliver(event('window', 9, { appId: 'Minecraft', title: 'Minecraft', state: 'normal', rect, desk: 0, stage: false, first: false, changed: ['stage'] }))
  assert.deepEqual(windows.api.list(), [{ id: 9, appId: 'Minecraft', title: 'Minecraft', state: 'normal', rect, desk: 0 }])
})

const window = (accent: string | null) => {
  const self = new WindowSelf(new Rpc(() => {}), {} as never, {} as never, null, WindowSelf.output(800, 600, 1))
  self.tint(accent)
  return self
}

test('accent() is the system accent the page started with, null when the system sets none', () => {
  assert.equal(window(null).api.accent(), null)
  assert.equal(window('#d0249f').api.accent(), '#d0249f')
})

test('a new accent fires the accent event once and accent() follows it', () => {
  const self = window(null)
  const seen: unknown[] = []
  const off = self.api.on('accent', (color) => void seen.push(color))
  self.tint('#d0249f')
  self.tint('#d0249f')
  assert.equal(self.api.accent(), '#d0249f')
  self.tint(null)
  assert.equal(self.api.accent(), null)
  assert.deepEqual(seen, ['#d0249f', null], 'an unchanged accent fires nothing')
  assert.equal(off(), true)
  self.tint('#0075ff')
  assert.deepEqual(seen, ['#d0249f', null], 'a removed handler hears nothing')
  assert.equal(self.api.accent(), '#0075ff', 'the default colour picked by the user is not the unset null')
})
