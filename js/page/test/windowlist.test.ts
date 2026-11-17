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
  for (const type of ['windowactivate', 'windowmenus', 'windowready', 'windowanimated', 'windowattention'] as const) windows.deliver(event(type, 11))
  assert.deepEqual(windows.api.list(), [{ id: 7, appId: 'notes', title: 'Notes', web: true, state: 'normal' }])
  windows.deliver(event('windowclose', 7))
  assert.deepEqual(windows.api.list(), [])
})
