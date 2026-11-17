import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createWeb, page, type Web } from './harness.ts'

const FAKE = `(() => {
  const backend = globalThis.backend = { calls: [], aborted: [], sent: [], closed: [], connections: 0 }
  const enc = new TextEncoder(), dec = new TextDecoder()
  const later = (ms, fn) => setTimeout(fn, ms)
  let next = 1
  const answer = (on, status, headers, chunks, gap = 1) => {
    const steps = [{ type: 'head', status, statusText: status === 200 ? 'OK' : 'X', headers }, ...chunks.map((c) => ({ type: 'chunk', data: enc.encode(c) })), { type: 'end' }]
    const step = () => {
      on(steps.shift())
      if (steps.length) later(gap, step)
    }
    later(1, step)
  }
  __acdeDom.installBackendTransport({
    port: 3000,
    request(port, method, path, headers, body, on) {
      const id = next++
      if (typeof port === 'string') {
        const call = { id, url: port, method, headers: Object.fromEntries(headers), body: body ? dec.decode(body) : null }
        backend.calls.push(call)
        const echo = JSON.stringify(call)
        const any = ['access-control-allow-origin', '*']
        const p = new URL(port).pathname
        if (p === '/open') answer(on, 200, [any, ['access-control-expose-headers', 'x-shown'], ['x-shown', '1'], ['x-hidden', '1'], ['set-cookie', 'a=1'], ['content-type', 'application/json']], [echo])
        else if (p === '/preflight' && method === 'OPTIONS') answer(on, 204, [any, ['access-control-allow-methods', 'PUT, DELETE'], ['access-control-allow-headers', 'authorization, x-a, content-type']], [])
        else if (p === '/wild' && method === 'OPTIONS') answer(on, 204, [any, ['access-control-allow-methods', '*'], ['access-control-allow-headers', '*']], [])
        else if (p === '/preflight' || p === '/wild') answer(on, 200, [any, ['content-type', 'application/json']], [echo])
        else if (p === '/creds') answer(on, 200, [['access-control-allow-origin', 'http://acde.local'], ['access-control-allow-credentials', 'true']], ['ok'])
        else if (p === '/hop') answer(on, 302, [any, ['location', 'https://other.example.com/open']], [])
        else if (p === '/lib.js') answer(on, 200, [['content-type', 'text/javascript']], ['globalThis.libLoaded = (globalThis.libLoaded ?? 0) + 1'])
        else answer(on, 404, [['content-type', 'text/plain']], ['nope'])
        return id
      }
      const call = { id, port, method, path, headers: Object.fromEntries(headers), body: body ? dec.decode(body) : null }
      backend.calls.push(call)
      const p = path.split('?')[0]
      if (p === '/api/echo') answer(on, 200, [['content-type', 'application/json']], [JSON.stringify(call)])
      else if (p === '/stream') answer(on, 200, [['content-type', 'text/plain']], ['a', 'b', 'c'], 5)
      else if (p === '/redirect') answer(on, 303, [['location', '/api/echo?from=redirect']], ['moved'])
      else if (p === '/cookie') answer(on, 200, [['set-cookie', 'sid=abc; Path=/; HttpOnly'], ['set-cookie', 'theme=dark; Path=/'], ['content-type', 'text/plain']], ['ok'])
      else if (p === '/slow') later(1, () => { on({ type: 'head', status: 200, statusText: 'OK', headers: [] }); on({ type: 'chunk', data: enc.encode('first') }) })
      else if (p === '/fail') later(1, () => on({ type: 'error', message: 'connect: no such file' }))
      else if (p === '/events') {
        backend.connections++
        if (backend.connections === 1) answer(on, 200, [['content-type', 'text/event-stream']], ['\\uFEFFretry: 20\\n: comment\\n', 'data: one\\n\\n', 'event: ping\\nid: 7\\ndata: a\\r\\ndata: b\\r', '\\n\\r\\ndata: unfinished'])
        else later(1, () => { on({ type: 'head', status: 200, statusText: 'OK', headers: [['content-type', 'text/event-stream; charset=utf-8']] }); on({ type: 'chunk', data: enc.encode('data: again ' + call.headers['last-event-id'] + '\\n\\n') }) })
      } else if (p === '/not-events') answer(on, 200, [['content-type', 'text/html']], ['<p>'])
      else answer(on, 404, [['content-type', 'text/plain']], ['not found: ' + path])
      return id
    },
    abort(id) { backend.aborted.push(id) },
    websocket(port, path, headers, protocols, on) {
      const id = next++
      backend.calls.push({ id, port, ws: true, path, headers: Object.fromEntries(headers), protocols })
      backend.sockets ??= {}
      backend.sockets[id] = on
      later(1, () => on({ type: 'open', protocol: protocols[0] ?? '', extensions: '' }))
      return id
    },
    send(id, data) {
      backend.sent.push(typeof data === 'string' ? data : [...data])
      const on = backend.sockets[id]
      later(1, () => on(typeof data === 'string' ? { type: 'message', text: 'echo:' + data } : { type: 'message', data }))
    },
    close(id, code, reason) {
      backend.closed.push([id, code, reason])
      later(1, () => backend.sockets[id]({ type: 'close', code: code ?? 1005, reason: reason ?? '', wasClean: true }))
    },
  })
})()`

async function webWithBackend(): Promise<Web> {
  const web = createWeb()
  web.host.assets.set('/app/demo/index.js', 'export {}')
  await web.internals.loadPage('<!doctype html><html><body></body></html>')
  web.run(FAKE)
  return web
}

test('backend routing: assets stay assets, same-origin and loopback URLs reach the backend, other hosts are refused', async () => {
  using web = await webWithBackend()
  const result = await web.run<Promise<string>>(`(async () => {
    const out = {}
    const asset = await fetch('/app/demo/index.js')
    out.asset = [asset.status, asset.headers.get('content-type'), await asset.text(), backend.calls.length]
    const post = await (await fetch('/api/echo?x=1', { method: 'POST', body: 'hi', headers: { 'X-A': '1', Host: 'evil', Cookie: 'no' } })).json()
    out.post = [post.port, post.method, post.path, post.body, post.headers.host, post.headers.origin, post.headers['x-a'], post.headers['content-type'], post.headers.cookie ?? null]
    const get = await (await fetch('/api/echo')).json()
    out.get = [get.port, get.headers.origin ?? null, get.headers.accept]
    for (const url of ['http://localhost:4000/api/echo', 'http://127.0.0.1:5000/api/echo', 'http://[::1]:6000/api/echo', 'http://0.0.0.0:7000/api/echo', 'http://localhost/api/echo']) {
      const r = await (await fetch(url)).json()
      out[url] = [r.port, r.headers.host]
    }
    const missing = await fetch('/missing')
    out.missing = [missing.status, missing.ok, await missing.text()]
    for (const bad of ['https://example.com/x', 'http://10.0.0.1:3000/x']) {
      try { await fetch(bad); out[bad] = 'fetched?' } catch (e) { out[bad] = e instanceof TypeError }
    }
    try { await fetch('/fail'); out.fail = 'fetched?' } catch (e) { out.fail = [e instanceof TypeError, e.message, e.cause] }
    return JSON.stringify(out)
  })()`)
  assert.deepEqual(JSON.parse(result), {
    asset: [200, 'text/javascript', 'export {}', 0],
    post: [3000, 'POST', '/api/echo?x=1', 'hi', 'localhost', 'http://localhost', '1', 'text/plain;charset=UTF-8', null],
    get: [3000, null, '*/*'],
    'http://localhost:4000/api/echo': [4000, 'localhost:4000'],
    'http://127.0.0.1:5000/api/echo': [5000, 'localhost:5000'],
    'http://[::1]:6000/api/echo': [6000, 'localhost:6000'],
    'http://0.0.0.0:7000/api/echo': [7000, 'localhost:7000'],
    'http://localhost/api/echo': [80, 'localhost'],
    missing: [404, false, 'not found: /missing'],
    'https://example.com/x': true,
    'http://10.0.0.1:3000/x': true,
    fail: [true, 'Failed to fetch', 'connect: no such file'],
  })
  assert.deepEqual(web.host.errors, [])
})

test('backend responses stream: chunks as they come, clone, pipeThrough, abort', async () => {
  using web = await webWithBackend()
  const result = await web.run<Promise<string>>(`(async () => {
    const out = {}
    const r = await fetch('/stream')
    out.head = [r.status, r.headers.get('content-type'), r.bodyUsed, r.body instanceof ReadableStream]
    const copy = r.clone()
    const reader = r.body.getReader()
    const chunks = []
    for (;;) { const { value, done } = await reader.read(); if (done) break; chunks.push(new TextDecoder().decode(value)) }
    out.chunks = chunks
    out.used = r.bodyUsed
    out.copy = await copy.text()
    try { await r.text(); out.again = 'read twice?' } catch (e) { out.again = e instanceof TypeError }
    const piped = (await fetch('/stream')).body.pipeThrough(new TextDecoderStream())
    const parts = []
    for await (const part of piped) parts.push(part)
    out.piped = parts.join('')
    const controller = new AbortController()
    const slow = await fetch('/slow', { signal: controller.signal })
    const slowReader = slow.body.getReader()
    out.first = new TextDecoder().decode((await slowReader.read()).value)
    controller.abort()
    try { await slowReader.read(); out.aborted = 'read on?' } catch (e) { out.aborted = e.name }
    await new Promise((resolve) => setTimeout(resolve, 5))
    out.abortCalls = backend.aborted.length
    const early = new AbortController()
    const pending = fetch('/slow', { signal: early.signal })
    early.abort()
    try { await pending } catch (e) { out.early = e.name }
    return JSON.stringify(out)
  })()`)
  assert.deepEqual(JSON.parse(result), {
    head: [200, 'text/plain', false, true],
    chunks: ['a', 'b', 'c'],
    used: true,
    copy: 'abc',
    again: true,
    piped: 'abc',
    first: 'first',
    aborted: 'AbortError',
    abortCalls: 1,
    early: 'AbortError',
  })
  assert.deepEqual(web.host.errors, [])
})

test('redirects are followed (303 → GET), cookies are kept and sent, Set-Cookie is not shown', async () => {
  using web = await webWithBackend()
  const result = await web.run<Promise<string>>(`(async () => {
    const out = {}
    const r = await fetch('/redirect', { method: 'POST', body: 'x' })
    const body = await r.json()
    out.redirect = [r.status, r.redirected, r.url, body.method, body.path, body.body, backend.aborted.length]
    const manual = await fetch('/redirect', { redirect: 'manual' })
    out.manual = [manual.status, manual.headers.get('location')]
    const c = await fetch('/cookie')
    out.setCookie = [c.headers.get('set-cookie'), document.cookie]
    document.cookie = 'lang=zh'
    document.cookie = 'sid=stolen'
    const echo = await (await fetch('/api/echo')).json()
    out.cookie = echo.headers.cookie
    const omit = await (await fetch('/api/echo', { credentials: 'omit' })).json()
    out.omit = omit.headers.cookie ?? null
    document.cookie = 'theme=; Max-Age=0'
    out.after = document.cookie
    return JSON.stringify(out)
  })()`)
  assert.deepEqual(JSON.parse(result), {
    redirect: [200, true, 'http://acde.local/api/echo?from=redirect', 'GET', '/api/echo?from=redirect', null, 1],
    manual: [303, '/api/echo?from=redirect'],
    setCookie: [null, 'theme=dark'],
    cookie: 'sid=abc; theme=dark; lang=zh',
    omit: null,
    after: 'lang=zh',
  })
  assert.deepEqual(web.host.errors, [])
})

test('XMLHttpRequest to the backend: states, progress, headers, response types, errors', async () => {
  using web = await webWithBackend()
  const result = await web.run<Promise<string>>(`(async () => {
    const out = {}
    const run = (setup) => new Promise((resolve) => {
      const xhr = new XMLHttpRequest()
      const seen = []
      xhr.onreadystatechange = () => seen.push(xhr.readyState)
      xhr.onprogress = () => seen.push('p')
      xhr.onload = () => seen.push('load')
      xhr.onerror = () => seen.push('error')
      xhr.onloadend = () => resolve({ xhr, seen })
      setup(xhr)
    })
    const post = await run((xhr) => { xhr.open('POST', '/api/echo'); xhr.setRequestHeader('X-B', '2'); xhr.responseType = 'json'; xhr.send(JSON.stringify({ a: 1 })) })
    out.post = [post.xhr.status, post.xhr.response.method, post.xhr.response.body, post.xhr.response.headers['x-b'], post.xhr.getResponseHeader('Content-Type'), post.seen.join()]
    const stream = await run((xhr) => { xhr.open('GET', '/stream'); xhr.send() })
    out.stream = [stream.xhr.responseText, stream.seen.join(), stream.xhr.getAllResponseHeaders()]
    const fail = await run((xhr) => { xhr.open('GET', '/fail'); xhr.send() })
    out.fail = [fail.xhr.status, fail.xhr.readyState, fail.seen.join()]
    const sync = new XMLHttpRequest()
    sync.open('GET', '/api/echo', false)
    try { sync.send(); out.sync = 'sent?' } catch (e) { out.sync = e.name }
    return JSON.stringify(out)
  })()`)
  assert.deepEqual(JSON.parse(result), {
    post: [200, 'POST', '{"a":1}', '2', 'application/json', '1,2,3,p,4,load'],
    stream: ['abc', '1,2,3,p,3,p,3,p,4,load', 'content-type: text/plain\r\n'],
    fail: [0, 4, '1,4,error'],
    sync: 'NetworkError',
  })
  assert.deepEqual(web.host.errors, [])
})

test('WebSocket to the backend: open, text and binary messages, close; refused without a backend route', async () => {
  using web = await webWithBackend()
  const result = await web.run<Promise<string>>(`(async () => {
    const out = {}
    const ws = new WebSocket('/ws?room=1', ['chat', 'v2'])
    out.url = [ws.url, ws.readyState, ws.binaryType, WebSocket.OPEN, ws.CLOSED]
    try { ws.send('too early') } catch (e) { out.early = e.name }
    const events = []
    await new Promise((resolve) => { ws.onopen = resolve })
    out.open = [ws.readyState, ws.protocol]
    const call = backend.calls.at(-1)
    out.call = [call.port, call.path, call.protocols, call.headers.host, call.headers.origin]
    ws.binaryType = 'arraybuffer'
    const messages = []
    ws.onmessage = (e) => messages.push(typeof e.data === 'string' ? e.data : [...new Uint8Array(e.data)])
    ws.send('hello')
    ws.send(new Uint8Array([1, 2, 3]))
    await new Promise((resolve) => setTimeout(resolve, 10))
    out.messages = messages
    out.sent = backend.sent
    try { ws.close(1001) } catch (e) { out.badCode = e.name }
    const closed = new Promise((resolve) => { ws.onclose = (e) => resolve([e.code, e.reason, e.wasClean, ws.readyState]) })
    ws.onerror = () => events.push('error')
    ws.close(1000, 'bye')
    out.closing = ws.readyState
    out.close = await closed
    out.errors = events
    const other = new WebSocket('wss://example.com/x#no'.replace('#no', ''))
    await new Promise((resolve) => { other.onopen = resolve })
    const remote = backend.calls.at(-1)
    out.remote = [remote.port, remote.path, remote.headers.origin, remote.headers.host ?? null, remote.headers.cookie ?? null]
    const port = new WebSocket('ws://127.0.0.1:9000/live')
    await new Promise((resolve) => { port.onopen = resolve })
    out.port = [backend.calls.at(-1).port, backend.calls.at(-1).path]
    return JSON.stringify(out)
  })()`)
  assert.deepEqual(JSON.parse(result), {
    url: ['ws://acde.local/ws?room=1', 0, 'blob', 1, 3],
    early: 'InvalidStateError',
    open: [1, 'chat'],
    call: [3000, '/ws?room=1', ['chat', 'v2'], 'localhost', 'http://localhost'],
    messages: ['echo:hello', [1, 2, 3]],
    sent: ['hello', [1, 2, 3]],
    badCode: 'InvalidAccessError',
    closing: 2,
    close: [1000, 'bye', true, 3],
    errors: [],
    remote: ['wss://example.com/x', '', 'http://acde.local', null, null],
    port: [9000, '/live'],
  })
  assert.deepEqual(web.host.errors, [])
})

test('network: other hosts are reached with CORS: preflight, credentials, exposed headers, no-cors, redirects', async () => {
  using web = await webWithBackend()
  const result = await web.run<Promise<string>>(`(async () => {
    const out = {}
    const calls = () => backend.calls.filter((c) => c.url)
    const open = await fetch('https://api.example.com/open?q=1#frag', { headers: { 'Content-Type': 'text/plain', Cookie: 'no', 'Accept-Encoding': 'br' } })
    const sent = await open.json()
    out.open = [open.status, open.type, open.headers.get('x-shown'), open.headers.get('x-hidden'), open.headers.get('set-cookie'), open.headers.get('content-type')]
    out.sent = [sent.url, sent.method, sent.headers.origin, sent.headers.referer, sent.headers.accept, sent.headers.cookie ?? null, sent.headers['accept-encoding'] ?? null, sent.headers.host ?? null, !!sent.headers['user-agent']]
    out.calls = calls().length
    const put = await (await fetch('https://api.example.com/preflight', { method: 'PUT', body: '{}', headers: { Authorization: 'Bearer t', 'X-A': '1', 'Content-Type': 'application/json' } })).json()
    const options = calls().at(-2)
    out.preflight = [options.method, options.headers['access-control-request-method'], options.headers['access-control-request-headers'], options.headers.authorization ?? null, put.method, put.headers.authorization, put.body]
    try { await fetch('https://api.example.com/preflight', { method: 'PATCH' }); out.patch = 'fetched?' } catch (e) { out.patch = [e instanceof TypeError, e.cause] }
    const before = calls().length
    try { await fetch('https://api.example.com/wild', { headers: { Authorization: 'x' } }); out.wild = 'fetched?' } catch (e) { out.wild = [e instanceof TypeError, calls().length - before] }
    out.wildOther = (await (await fetch('https://api.example.com/wild', { method: 'DELETE', headers: { 'X-B': '2' } })).json()).method
    try { await fetch('https://api.example.com/closed'); out.closed = 'fetched?' } catch (e) { out.closed = e.cause }
    try { await fetch('https://api.example.com/open', { credentials: 'include' }); out.include = 'fetched?' } catch (e) { out.include = e.cause.slice(0, 40) }
    out.creds = await (await fetch('https://api.example.com/creds', { credentials: 'include' })).text()
    const opaque = await fetch('https://api.example.com/closed', { mode: 'no-cors' })
    out.opaque = [opaque.status, opaque.type, opaque.ok, [...opaque.headers].length, opaque.body, calls().at(-1).headers.origin ?? null]
    try { await fetch('https://api.example.com/open', { mode: 'same-origin' }); out.same = 'fetched?' } catch (e) { out.same = e instanceof TypeError }
    try { new Request('https://api.example.com/open', { mode: 'no-cors', method: 'PUT' }); out.noCorsPut = 'made?' } catch (e) { out.noCorsPut = e instanceof TypeError }
    const hop = await fetch('https://api.example.com/hop')
    out.hop = [hop.url, hop.redirected, calls().at(-1).headers.origin]
    const xhr = new XMLHttpRequest()
    xhr.open('GET', 'https://api.example.com/open')
    await new Promise((resolve) => { xhr.onloadend = resolve; xhr.send() })
    out.xhr = [xhr.status, xhr.getResponseHeader('x-shown'), xhr.getResponseHeader('x-hidden'), xhr.getAllResponseHeaders().includes('set-cookie')]
    const plain = await (await fetch('http://192.168.1.5:8080/open')).json()
    out.plain = plain.url
    return JSON.stringify(out)
  })()`)
  assert.deepEqual(JSON.parse(result), {
    open: [200, 'cors', '1', null, null, 'application/json'],
    sent: ['https://api.example.com/open?q=1', 'GET', 'http://acde.local', 'http://acde.local/', '*/*', null, null, null, true],
    calls: 1,
    preflight: ['OPTIONS', 'PUT', 'authorization,content-type,x-a', null, 'PUT', 'Bearer t', '{}'],
    patch: [true, "CORS: Response to preflight request doesn't pass access control check: Method PATCH is not allowed by Access-Control-Allow-Methods in preflight response."],
    wild: [true, 1],
    wildOther: 'DELETE',
    closed: "CORS: No 'Access-Control-Allow-Origin' header is present on the requested resource.",
    include: "CORS: The value of the 'Access-Control-A",
    creds: 'ok',
    opaque: [0, 'opaque', false, 0, null, null],
    same: true,
    noCorsPut: true,
    hop: ['https://other.example.com/open', true, 'null'],
    xhr: [200, '1', null, false],
    plain: 'http://192.168.1.5:8080/open',
  })
})

test('a classic script from another host runs without CORS, while fetch of it is blocked', async () => {
  using web = await webWithBackend()
  const result = await web.run<Promise<string>>(`(async () => {
    const script = document.createElement('script')
    script.src = 'https://cdn.example.com/lib.js'
    const loaded = new Promise((resolve) => { script.onload = () => resolve('load'); script.onerror = () => resolve('error') })
    document.body.appendChild(script)
    const event = await loaded
    let fetched
    try { await fetch('https://cdn.example.com/lib.js'); fetched = 'fetched?' } catch (e) { fetched = e instanceof TypeError }
    return JSON.stringify([event, globalThis.libLoaded, fetched, backend.calls.filter((c) => c.url)[0].headers.origin ?? null])
  })()`)
  assert.deepEqual(JSON.parse(result), ['load', 1, true, null])
})

test('EventSource from the backend: events, types, ids, retry, reconnect with Last-Event-ID, close', async () => {
  using web = await webWithBackend()
  const result = await web.run<Promise<string>>(`(async () => {
    const out = { log: [] }
    const es = new EventSource('/events')
    out.start = [es.url, es.readyState]
    es.onopen = () => out.log.push('open ' + es.readyState)
    es.onmessage = (e) => out.log.push('message ' + JSON.stringify(e.data) + ' ' + e.lastEventId)
    es.addEventListener('ping', (e) => out.log.push('ping ' + JSON.stringify(e.data) + ' ' + e.lastEventId))
    es.onerror = () => out.log.push('error ' + es.readyState)
    await new Promise((resolve) => { const t = setInterval(() => { if (out.log.some((l) => l.startsWith('message "again'))) { clearInterval(t); resolve() } }, 2) })
    es.close()
    out.closed = es.readyState
    const bad = new EventSource('/not-events')
    out.bad = await new Promise((resolve) => { bad.onerror = () => resolve(bad.readyState) })
    return JSON.stringify(out)
  })()`)
  assert.deepEqual(JSON.parse(result), {
    start: ['http://acde.local/events', 0],
    log: ['open 1', 'message "one" ', 'ping "a\\nb" 7', 'error 0', 'open 1', 'message "again 7" 7'],
    closed: 2,
    bad: 2,
  })
  assert.deepEqual(web.host.errors, [])
})

test('without a backend transport nothing changes: non-asset requests are refused, WebSocket and EventSource fail', async () => {
  using web = await page()
  const result = await web.run<Promise<string>>(`(async () => {
    const out = []
    for (const [url, init] of [['/api/x', { method: 'POST', body: 'x' }], ['http://localhost:3000/api'], ['/missing']]) {
      try { await fetch(url, init); out.push('fetched?') } catch (e) { out.push(e instanceof TypeError) }
    }
    const ws = new WebSocket('ws://localhost:3000/ws')
    out.push(await new Promise((resolve) => { const seen = []; ws.onerror = () => seen.push('error'); ws.onclose = (e) => resolve([...seen, e.code]) }))
    const es = new EventSource('/events')
    out.push(await new Promise((resolve) => { es.onerror = () => resolve(es.readyState) }))
    return JSON.stringify(out)
  })()`)
  assert.deepEqual(JSON.parse(result), [true, true, true, ['error', 1006], 2])
  assert.deepEqual(web.host.errors, [])
})

test('streams: tee, pipeTo through a TransformStream, ReadableStream.from, async iteration, errors, Blob.stream', async () => {
  using web = await page()
  const result = await web.run<Promise<string>>(`(async () => {
    const out = {}
    const all = async (stream) => { const got = []; for await (const c of stream) got.push(c); return got }
    const [a, b] = ReadableStream.from([1, 2, 3]).tee()
    out.tee = [await all(a), await all(b)]
    const upper = new TransformStream({ transform(chunk, c) { c.enqueue(String(chunk).toUpperCase()) }, flush(c) { c.enqueue('!') } })
    const collected = []
    await ReadableStream.from(['a', 'b']).pipeThrough(upper).pipeTo(new WritableStream({ write(c) { collected.push(c) } }))
    out.piped = collected
    const failing = new ReadableStream({ start(c) { c.enqueue('x'); c.error(new Error('boom')) } })
    try { await all(failing); out.error = 'no error?' } catch (e) { out.error = e.message }
    let pulls = 0
    const pulled = new ReadableStream({ pull(c) { pulls++; if (pulls > 3) c.close(); else c.enqueue(pulls) } }, { highWaterMark: 0 })
    out.pulled = await all(pulled)
    const reader = new ReadableStream({ start(c) { c.enqueue('q') } }).getReader()
    out.locked = [await reader.read(), reader.closed instanceof Promise]
    reader.releaseLock()
    out.blob = new TextDecoder().decode((await new Blob(['hi']).stream().getReader().read()).value)
    const enc = new TextEncoderStream()
    const w = enc.writable.getWriter(); w.write('é'); w.close()
    out.encoded = (await all(enc.readable)).map((u) => [...u])
    const response = new Response(ReadableStream.from([new TextEncoder().encode('from '), new TextEncoder().encode('stream')]))
    out.response = await response.text()
    return JSON.stringify(out)
  })()`)
  assert.deepEqual(JSON.parse(result), {
    tee: [[1, 2, 3], [1, 2, 3]],
    piped: ['A', 'B', '!'],
    error: 'boom',
    pulled: [1, 2, 3],
    locked: [{ value: 'q', done: false }, true],
    blob: 'hi',
    encoded: [[195, 169]],
    response: 'from stream',
  })
  assert.deepEqual(web.host.errors, [])
})

test('Set-Cookie values stay apart; formData parses forms', async () => {
  using web = createWeb()
  const result = await web.run<Promise<string>>(`(async () => {
    const out = []
    const h = new Headers([['Set-Cookie', 'a=1; Expires=Wed, 21 Oct 2026 07:28:00 GMT'], ['set-cookie', 'b=2'], ['x-one', '1']])
    out.push(h.getSetCookie(), [...h], h.get('set-cookie'))
    const urlencoded = await new Request('/f', { method: 'POST', body: new URLSearchParams({ a: '1', b: 'two' }) }).formData()
    out.push(urlencoded.get('a'), urlencoded.get('b'))
    const multipart = '--XyZ\\r\\nContent-Disposition: form-data; name="name"\\r\\n\\r\\n值\\r\\n' +
      '--XyZ\\r\\nContent-Disposition: form-data; name="file"; filename="f.txt"\\r\\nContent-Type: text/plain\\r\\n\\r\\nfile body\\r\\n--XyZ--\\r\\n'
    const parsed = await new Request('/f', { method: 'POST', body: multipart, headers: { 'content-type': 'multipart/form-data; boundary=XyZ' } }).formData()
    const file = parsed.get('file')
    out.push(parsed.get('name'), file.name, file.type, await file.text())
    return JSON.stringify(out)
  })()`)
  assert.deepEqual(JSON.parse(result), [
    ['a=1; Expires=Wed, 21 Oct 2026 07:28:00 GMT', 'b=2'],
    [['set-cookie', 'a=1; Expires=Wed, 21 Oct 2026 07:28:00 GMT'], ['set-cookie', 'b=2'], ['x-one', '1']],
    'a=1; Expires=Wed, 21 Oct 2026 07:28:00 GMT, b=2',
    '1', 'two',
    '值', 'f.txt', 'text/plain', 'file body',
  ])
})

test('observe: console lines, page errors and every request with its outcome reach the observer', async () => {
  using web = await webWithBackend()
  web.run(`globalThis.seen = []
    __acdeDom.observe({
      console: (level, text) => seen.push(['console', level, text.split('\\n')[0]]),
      error: (kind, text) => seen.push([kind, text.split('\\n')[0]]),
      request: (e) => seen.push(['request', e.phase, ...(e.phase === 'start' ? [e.kind, e.method, e.url.replace(location.origin, ''), e.bytes] : e.phase === 'response' ? [e.status] : e.phase === 'end' ? [e.bytes] : [e.message])]),
    })`)
  await web.run<Promise<void>>(`(async () => {
    console.log('hello %s', 'there')
    console.info('fyi')
    console.warn('careful', { n: 1 })
    console.error(new Error('boom'))
    await (await fetch('/app/demo/index.js')).text()
    await (await fetch('/api/echo', { method: 'POST', body: 'abc' })).text()
    await (await fetch('/missing')).text()
    try { await fetch('/fail') } catch {}
    try { await fetch('https://example.com/x') } catch {}
    await (await fetch('/nothing-here')).text()
    await new Promise((resolve) => { const x = new XMLHttpRequest(); x.open('GET', '/stream'); x.onloadend = resolve; x.send() })
    const ws = new WebSocket('/socket')
    await new Promise((resolve) => (ws.onopen = resolve))
    ws.send('ping')
    await new Promise((resolve) => (ws.onmessage = resolve))
    ws.close()
    await new Promise((resolve) => (ws.onclose = resolve))
    setTimeout(() => { throw new TypeError('late') })
    await new Promise((resolve) => setTimeout(resolve, 10))
  })()`)
  web.internals.unhandledRejection(Promise.resolve(), new RangeError('nobody waits'))
  web.run(`__acdeDom.observe(null); console.log('not seen')`)
  const seen = JSON.parse(web.run<string>('JSON.stringify(seen)')) as unknown[][]
  assert.deepEqual(seen.filter((e) => e[0] === 'console'), [
    ['console', 'log', 'hello there'],
    ['console', 'info', 'fyi'],
    ['console', 'warn', 'careful { n: 1 }'],
    ['console', 'error', 'Error: boom'],
  ])
  const echo = seen.filter((e) => e[0] === 'request')[5]![2]
  assert.deepEqual(seen.filter((e) => e[0] === 'request').map((e) => e.slice(1)), [
    ['start', 'fetch', 'GET', '/app/demo/index.js', 0], ['response', 200], ['end', 9],
    ['start', 'fetch', 'POST', '/api/echo', 3], ['response', 200], ['end', echo],
    ['start', 'fetch', 'GET', '/missing', 0], ['response', 404], ['end', 'not found: /missing'.length],
    ['start', 'fetch', 'GET', '/fail', 0], ['error', 'connect: no such file'],
    ['start', 'fetch', 'GET', 'https://example.com/x', 0], ['error', "CORS: No 'Access-Control-Allow-Origin' header is present on the requested resource."],
    ['start', 'fetch', 'GET', '/nothing-here', 0], ['response', 404], ['end', 'not found: /nothing-here'.length],
    ['start', 'xhr', 'GET', '/stream', 0], ['response', 200], ['end', 3],
    ['start', 'websocket', 'GET', 'ws://acde.local/socket', 0], ['response', 101], ['end', 'echo:ping'.length],
  ])
  assert.deepEqual(seen.filter((e) => e[0] === 'error' || e[0] === 'rejection'), [['error', 'TypeError: late'], ['rejection', 'RangeError: nobody waits']])
})

test('observe: route answers fetch and XHR in place of the app, its backend and the network, and marks them mocked', async () => {
  using web = await webWithBackend()
  const result = await web.run<Promise<string>>(`(async () => {
    const starts = []
    const bytes = (text) => new TextEncoder().encode(text)
    __acdeDom.observe({
      request: (e) => { if (e.phase === 'start') starts.push([e.kind, e.method, e.url.replace(location.origin, ''), e.mocked === true]) },
      route: (kind, method, url) => {
        if (url.endsWith('/api/echo') && method === 'POST') return { status: 201, statusText: 'Created', headers: [['content-type', 'application/json'], ['x-mock', kind]], body: bytes('{"ok":true}') }
        if (url.startsWith('https://example.com/')) return { status: 200, statusText: 'OK', headers: [], body: bytes('remote') }
        if (url.endsWith('/gone')) return { error: 'offline' }
        if (url.endsWith('/empty')) return { status: 204, statusText: 'No Content', headers: [], body: bytes('dropped') }
        return null
      },
    })
    const out = []
    const echo = await fetch('/api/echo', { method: 'POST', body: 'abc' })
    out.push(echo.status, echo.statusText, echo.headers.get('x-mock'), await echo.json(), echo.url.replace(location.origin, ''))
    out.push(await (await fetch('https://example.com/data')).text())
    try { await fetch('/gone') } catch (e) { out.push(e.name, e.message, e.cause) }
    const empty = await fetch('/empty')
    out.push(empty.status, empty.body === null)
    out.push(await (await fetch('/app/demo/index.js')).text())
    out.push(await new Promise((resolve) => {
      const x = new XMLHttpRequest()
      x.open('POST', '/api/echo')
      x.onload = () => resolve([x.status, x.statusText, x.getResponseHeader('content-type'), x.getResponseHeader('x-mock'), x.responseText])
      x.send('abc')
    }))
    out.push(await new Promise((resolve) => {
      const x = new XMLHttpRequest()
      x.open('GET', '/gone')
      x.onerror = () => resolve(['error', x.readyState, x.status])
      x.send()
    }))
    const sync = new XMLHttpRequest()
    sync.open('GET', 'https://example.com/sync', false)
    sync.send()
    out.push(sync.status, sync.responseText)
    __acdeDom.observe(null)
    out.push(starts)
    return JSON.stringify(out)
  })()`)
  assert.deepEqual(JSON.parse(result), [
    201, 'Created', 'fetch', { ok: true }, '/api/echo',
    'remote',
    'TypeError', 'Failed to fetch', 'offline',
    204, true,
    'export {}',
    [201, 'Created', 'application/json', 'xhr', '{"ok":true}'],
    ['error', 4, 0],
    200, 'remote',
    [
      ['fetch', 'POST', '/api/echo', true],
      ['fetch', 'GET', 'https://example.com/data', true],
      ['fetch', 'GET', '/gone', true],
      ['fetch', 'GET', '/empty', true],
      ['fetch', 'GET', '/app/demo/index.js', false],
      ['xhr', 'POST', '/api/echo', true],
      ['xhr', 'GET', '/gone', true],
      ['xhr', 'GET', 'https://example.com/sync', true],
    ],
  ])
})

test('observe: without a backend, a missing file of the app and another host fail with different reasons', async () => {
  using web = await page()
  const result = await web.run<Promise<string>>(`(async () => {
    const seen = []
    __acdeDom.observe({ request: (e) => e.phase === 'error' && seen.push(e.message) })
    for (const url of ['/nope.json', 'https://example.com/x']) { try { await fetch(url) } catch {} }
    __acdeDom.observe(null)
    return JSON.stringify(seen)
  })()`)
  assert.deepEqual(JSON.parse(result), ['no such file in the app, and no backend to ask', 'another host, and this page has no network'])
})

test("an app's own resources get page loading types; documents and unknown names are octet-stream", async () => {
  using web = await page()
  const assets: Record<string, string> = {
    '/app/main.mjs': 'export {}', '/app/style.css': 'body {}', '/app/mod.wasm': '\0asm', '/app/font.woff2': 'wOF2', '/app/data.json': '{}',
    '/app/report.docx': 'PK', '/app/notes.md': '# n', '/app/table.csv': 'a,b', '/app/blob.zzz': 'x',
  }
  for (const [path, body] of Object.entries(assets)) web.host.assets.set(path, body)
  const paths = JSON.stringify(Object.keys(assets))
  web.run(`globalThis.out = {}; Promise.all(${paths}.map((p) => fetch(p).then((r) => { out[p] = r.headers.get('content-type') })))`)
  await web.wait(20)
  assert.deepEqual(web.json('out'), {
    '/app/main.mjs': 'text/javascript', '/app/style.css': 'text/css', '/app/mod.wasm': 'application/wasm', '/app/font.woff2': 'font/woff2',
    '/app/data.json': 'application/json', '/app/report.docx': 'application/octet-stream', '/app/notes.md': 'application/octet-stream',
    '/app/table.csv': 'application/octet-stream', '/app/blob.zzz': 'application/octet-stream',
  })
})

test('moduleSource: the text of blob: and data: modules, for JavaScript MIME types only', async () => {
  using web = await page()
  const moduleSource = web.host.moduleSource!
  const url = web.run(`URL.createObjectURL(new Blob(['export default 1'], { type: 'Text/JavaScript; charset=utf-8' }))`) as string
  assert.equal(moduleSource(url), 'export default 1')
  assert.equal(moduleSource('data:text/javascript,export%20default%202'), 'export default 2')
  const plain = web.run(`URL.createObjectURL(new Blob(['x'], { type: '' }))`) as string
  assert.throws(() => moduleSource(plain), /expected a JavaScript module script/)
  assert.equal(moduleSource('blob:http://acde.local/none'), null)
})
