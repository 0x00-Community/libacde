import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createWeb } from './harness.ts'

function fakeWorkers() {
  const calls: { name: string; args: unknown[] }[] = []
  let emit: ((id: number, kind: string, payload: unknown) => void) | null = null
  const record = (name: string) => (...args: unknown[]) => void calls.push({ name, args })
  const native = {
    spawn: record('spawn'),
    load: record('load'),
    post: record('post'),
    terminate: record('terminate'),
    onEvent(fn: typeof emit) {
      emit = fn
    },
  }
  return { native, calls, named: (name: string) => calls.filter((c) => c.name === name).map((c) => c.args), emit: (id: number, kind: string, payload?: unknown) => emit!(id, kind, payload) }
}

const SCRIPT = 'data:text/javascript,onmessage%3D()%3D%3E0'

test('Worker: checks its options and URL, then loads its script into the worker it spawned', async () => {
  const workers = fakeWorkers()
  const w = createWeb({ workers: workers.native as never })
  const errors = w.json<string[]>(`(() => {
    const errors = []
    for (const make of [
      () => new Worker('a.js', { type: 'bogus' }),
      () => new Worker('https://example.com/a.js'),
      () => new Worker('http://[bad'),
      () => new Worker(),
    ]) { try { make(); errors.push('none') } catch (e) { errors.push(e.name) } }
    globalThis.worker = new Worker('${SCRIPT}', { name: 'n1' })
    worker.postMessage('early')
    return errors
  })()`)
  assert.deepEqual(errors, ['TypeError', 'SecurityError', 'SyntaxError', 'TypeError'])
  const [[id, name]] = workers.named('spawn') as [[number, string]]
  assert.equal(name, 'n1')
  assert.deepEqual(JSON.parse(JSON.stringify(workers.named('post'))), [[id, 'early', []]], 'a message posted before the script loads goes to the worker at once; the worker holds it')
  await w.wait(5)
  const [[loadId, url, source, module, env]] = workers.named('load') as [[number, string, string, boolean, string]]
  assert.deepEqual([loadId, url, source, module], [id, SCRIPT, 'onmessage=()=>0', false])
  assert.equal(JSON.parse(env).name, 'n1')
  w.close()
})

test('Worker: messages, errors and the end of the worker reach the Worker object', async () => {
  const workers = fakeWorkers()
  const w = createWeb({ workers: workers.native as never })
  w.run(`
    globalThis.log = []
    globalThis.worker = new Worker('${SCRIPT}')
    worker.onmessage = (e) => log.push('message ' + JSON.stringify(e.data))
    worker.onmessageerror = (e) => log.push('messageerror ' + e.constructor.name)
    worker.addEventListener('error', (e) => { log.push('error ' + [e.constructor.name, e.message, e.filename, e.lineno, e.colno].join(' ')); e.preventDefault() })
  `)
  const [[id]] = workers.named('spawn') as [[number]]
  workers.emit(id, 'message', { a: 1 })
  workers.emit(id, 'messageerror')
  workers.emit(id, 'error', { message: 'Uncaught Error: x', filename: 'blob:a', lineno: 3, colno: 9 })
  workers.emit(id, 'exit')
  workers.emit(id, 'message', 'after exit')
  assert.deepEqual(w.json('log'), ['message {"a":1}', 'messageerror MessageEvent', 'error ErrorEvent Uncaught Error: x blob:a 3 9'])
  const transfer = w.run<string>(`(() => { const c = new MessageChannel(); try { worker.postMessage(1, [c.port1]); return 'none' } catch (e) { return e.name } })()`)
  assert.equal(transfer, 'DataCloneError')
  w.close()
})

test('Worker: terminate ends it once, drops what comes after, and a script that cannot load fires error', async () => {
  const workers = fakeWorkers()
  const w = createWeb({ workers: workers.native as never })
  w.run(`
    globalThis.log = []
    globalThis.a = new Worker('${SCRIPT}')
    a.onmessage = () => log.push('a message')
    a.terminate(); a.terminate()
    a.postMessage('dropped')
    globalThis.b = new Worker('missing.js')
    b.onerror = (e) => log.push('b ' + e.constructor.name)
  `)
  const [[a], [b]] = workers.named('spawn') as [[number], [number]]
  workers.emit(a, 'message', 1)
  await w.wait(5)
  assert.deepEqual(w.json('log'), ['b Event'])
  assert.deepEqual(workers.named('terminate'), [[a], [b]], 'the worker whose script failed is let go too')
  assert.deepEqual(workers.named('post'), [])
  assert.deepEqual(workers.named('load').map((c) => c[0]), [], 'neither ever got a script')
  w.close()
})
