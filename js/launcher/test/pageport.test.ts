import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { LINE_LIMIT, Lines, PagePort } from '../src/manager/pageport.ts'

const bytes = (text: string) => new TextEncoder().encode(text)
const later = () => new Promise((resolve) => setTimeout(resolve, 0))

function link() {
  const written: string[] = []
  const box: { data: ((chunk: Uint8Array) => void) | null; close: (() => void) | null; closed: number } = { data: null, close: null, closed: 0 }
  return {
    written,
    box,
    value: {
      role: 'plugin',
      write: (data: string | Uint8Array) => void written.push(String(data)),
      onData: (handler: (chunk: Uint8Array) => void) => {
        box.data = handler
      },
      onClose: (handler: () => void) => {
        box.close = handler
      },
      close: () => void box.closed++,
    },
  }
}

describe('the lines of a page port connection', () => {
  test('a line split across chunks and characters is put back together; an endless one is reported once and skipped', () => {
    const got: (string | null)[] = []
    const lines = new Lines((line) => got.push(line))
    const all = bytes('{"a":"é"}\n[1]\n[2')
    lines.push(all.slice(0, 7))
    lines.push(all.slice(7))
    lines.push(bytes(']\n'))
    assert.deepEqual(got, ['{"a":"é"}', '[1]', '[2]'])
    lines.push(bytes('x'.repeat(LINE_LIMIT + 1)))
    lines.push(bytes('more'))
    lines.push(bytes('tail\n[3]\n'))
    assert.deepEqual(got.slice(3), [null, '[3]'])
  })
})

describe('the port a page gets at launch', () => {
  test('the init carries the data; lines the page posts wait for the connection, then go out in order', async () => {
    const posted: Record<string, unknown>[] = []
    const made = link()
    let calls = 0
    let give: (value: unknown) => void = () => {}
    const port = new PagePort({ type: 'x:init' }, () => {
      calls++
      return new Promise((resolve) => {
        give = resolve
      }) as never
    }, () => {})
    assert.deepEqual(port.init(), { port: { data: { type: 'x:init' } } })
    port.open((message) => void posted.push(message))
    assert.equal(port.take({ portline: '{"n":1}' }), true)
    port.take({ portline: '{"n":2}' })
    assert.equal(port.take({ menukey: 'a' }), false)
    give(made.value)
    await later()
    port.take({ portline: '{"n":3}' })
    assert.deepEqual(made.written, ['{"n":1}\n', '{"n":2}\n', '{"n":3}\n'])
    made.box.data!(bytes('{"ok":true}\n'))
    assert.deepEqual(posted, [{ type: 'portline', line: '{"ok":true}' }])
    made.box.close!()
    assert.deepEqual(posted.at(-1), { type: 'portclosed' })
    assert.equal(calls, 1)
  })

  test('the page closing its end closes the connection; a reload connects again and drops the old one', async () => {
    const first = link()
    const second = link()
    const links = [first, second]
    const port = new PagePort(null, async () => links.shift()!.value as never, () => {})
    port.open(() => {})
    await later()
    port.take({ portclosed: true })
    assert.equal(first.box.closed, 1)
    port.open(() => {})
    await later()
    port.open(() => {})
    assert.equal(second.box.closed, 1)
  })

  test('a refused connection closes the page end and says why', async () => {
    const posted: Record<string, unknown>[] = []
    const said: string[] = []
    const failure = Object.assign(new Error('no back end'), { status: 503, code: 'service_not_running' })
    const refused = new PagePort(null, () => Promise.reject(failure), (line) => said.push(line))
    refused.open((message) => void posted.push(message))
    await later()
    assert.deepEqual(posted, [{ type: 'portclosed' }])
    assert.deepEqual(said, ['the page port has no connection: 503 service_not_running no back end'])
    const old = new PagePort(null, undefined, (line) => said.push(line))
    old.open(() => {})
    await later()
    assert.match(said.at(-1)!, /501 unsupported/)
  })
})
