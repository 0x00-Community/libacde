import assert from 'node:assert/strict'
import { test } from 'node:test'
import { DevToolsPage } from '../../src/devtools/index.ts'

function page() {
  const sent: Record<string, unknown>[] = []
  const devtools = new DevToolsPage('frame', (_, text) => void sent.push(JSON.parse(text) as Record<string, unknown>))
  devtools.use([
    {
      name: 'Test',
      methods: {
        now: () => ({ value: 1 }),
        later: async () => ({ value: 2 }),
        broken: () => {
          throw new Error('no')
        },
      },
    },
  ])
  return { devtools, sent }
}

test('a method that answers at once is answered before receive returns, so it works while the page is paused', () => {
  const { devtools, sent } = page()
  devtools.receive(1, JSON.stringify({ id: 1, method: 'Test.now', params: {} }))
  devtools.receive(1, JSON.stringify({ id: 2, method: 'Test.broken', params: {} }))
  devtools.receive(1, JSON.stringify({ id: 3, method: 'Test.missing', params: {} }))
  assert.deepEqual(sent, [
    { id: 1, result: { value: 1 } },
    { id: 2, error: { code: -32603, message: 'no' } },
    { id: 3, error: { code: -32601, message: "'Test.missing' wasn't found" } },
  ])
})

test('a method that answers with a promise is answered when it settles', async () => {
  const { devtools, sent } = page()
  devtools.receive(1, JSON.stringify({ id: 4, method: 'Test.later', params: {} }))
  assert.deepEqual(sent, [])
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.deepEqual(sent, [{ id: 4, result: { value: 2 } }])
})
