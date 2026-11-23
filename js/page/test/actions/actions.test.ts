import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { AppAction } from '@acde/api'
import { Actions, type ActionEvent } from '../../src/actions/index.ts'

const caller = { id: 'agent-1', session: 's1', run: 'r1' }
const declared: AppAction[] = [
  { name: 'add_note', description: 'Add a note', inputSchema: { type: 'object' } },
  { name: 'export_all', description: 'Export every note', inputSchema: { type: 'object' }, longRunning: true },
]

function registry() {
  const sent: ActionEvent[] = []
  return { sent, actions: new Actions(declared, (event) => void sent.push(event)) }
}

test('a declared action runs its handler with the caller and answers {ok, output}', async () => {
  const { sent, actions } = registry()
  let seen: unknown = null
  actions.api.handle<{ text: string }>('add_note', (args, call) => {
    seen = [args.text, call.caller.id]
    return { ok: true, output: { id: 7 } }
  })
  await actions.operate({ id: 'op1', name: 'add_note', args: { text: 'milk' }, caller })
  assert.deepEqual(seen, ['milk', 'agent-1'])
  assert.deepEqual(sent, [{ type: 'result', id: 'op1', result: { ok: true, output: { id: 7 } } }])
})

test('handle refuses an undeclared name; unknown, unhandled, throwing and malformed answers become errors', async () => {
  const { sent, actions } = registry()
  assert.throws(() => actions.api.handle('nope', () => ({ ok: true })), TypeError)
  await actions.operate({ id: 'a', name: 'nope', args: {}, caller })
  await actions.operate({ id: 'b', name: 'add_note', args: {}, caller })
  const off = actions.api.handle('add_note', () => {
    throw new Error('disk full')
  })
  await actions.operate({ id: 'c', name: 'add_note', args: {}, caller })
  assert.equal(off(), true)
  actions.api.handle('add_note', () => 'fine' as never)
  await actions.operate({ id: 'd', name: 'add_note', args: {}, caller })
  assert.deepEqual(sent.map((e) => e.type === 'result' && 'error' in e.result && e.result.error.code), ['unknown_action', 'no_handler', 'failed', 'bad_result'])
})

test('a long-running action answers pending first, then progress and the result, and can be cancelled', async () => {
  const { sent, actions } = registry()
  let release: () => void = () => {}
  actions.api.handle('export_all', async (_args, call) => {
    call.progress(0.5, 'half')
    await new Promise<void>((done) => {
      release = done
      call.signal.addEventListener('abort', () => done())
    })
    return { ok: true, output: 'done' }
  })
  const first = actions.operate({ id: 'x', name: 'export_all', args: {}, caller })
  await Promise.resolve()
  release()
  await first
  assert.deepEqual(sent.splice(0), [
    { type: 'pending', id: 'x' }, { type: 'progress', id: 'x', done: 0.5, text: 'half' }, { type: 'result', id: 'x', result: { ok: true, output: 'done' } },
  ])
  const second = actions.operate({ id: 'y', name: 'export_all', args: {}, caller })
  await Promise.resolve()
  assert.equal(actions.cancel('y'), true)
  await second
  assert.deepEqual(sent.at(-1), { type: 'result', id: 'y', result: { error: { code: 'cancelled', message: 'export_all was cancelled' } } })
  assert.equal(actions.cancel('y'), false)
})
