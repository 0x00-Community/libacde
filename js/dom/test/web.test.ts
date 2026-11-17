import assert from 'node:assert/strict'
import { test } from 'node:test'
import vm from 'node:vm'
import { bundle, createWeb, page } from './harness.ts'
import { MockHost } from './mock/host.ts'
import { MockStorage } from './mock/storage.ts'

test('fetch and XMLHttpRequest: same-origin assets only, data: and blob: URLs; the rest is refused', async () => {
  using web = createWeb()
  web.host.assets.set('/ui/data.json', '{"a":1}')
  await web.internals.loadPage('<!doctype html><html><body></body></html>')
  const result = await web.run<Promise<string>>(`(async () => {
    const out = []
    const r = await fetch('/ui/data.json')
    out.push(r.ok, r.status, r.headers.get('content-type'), JSON.stringify(await r.json()))
    out.push(await (await fetch('data:text/plain;base64,aGk=')).text())
    const url = URL.createObjectURL(new Blob(['blob body'], { type: 'text/plain' }))
    out.push(await (await fetch(url)).text())
    out.push(await (await fetch(url + '#part')).text())
    for (const bad of ['https://example.com/x', '/missing']) {
      try { await fetch(bad); out.push('fetched?') } catch (e) { out.push(e instanceof TypeError) }
    }
    const xhr = new XMLHttpRequest()
    await new Promise((resolve) => { xhr.onload = resolve; xhr.open('GET', '/ui/data.json'); xhr.responseType = 'json'; xhr.send() })
    out.push(xhr.status, xhr.response.a)
    const controller = new AbortController(); controller.abort()
    try { await fetch('/ui/data.json', { signal: controller.signal }) } catch (e) { out.push(e.name) }
    return JSON.stringify(out)
  })()`)
  assert.deepEqual(JSON.parse(result), [true, 200, 'application/json', '{"a":1}', 'hi', 'blob body', 'blob body', true, true, 200, 1, 'AbortError'])
})

test('storage, location / history, matchMedia, navigator, screen, viewport', async () => {
  const storage = new MockStorage({ theme: 'dark' })
  using web = createWeb({ url: 'http://acde.local/index.html#desk', storage })
  web.dom.media['prefers-reduced-motion'] = 'reduce'
  await web.internals.loadPage('<!doctype html><html><body></body></html>')
  const result = web.json(`(() => {
    const out = {}
    out.storage = [localStorage.getItem('theme'), localStorage.theme, localStorage.length]
    localStorage.name = 'ada'
    localStorage.setItem('n', 2)
    delete localStorage.theme
    out.storageAfter = [localStorage.getItem('name'), localStorage.n, 'theme' in localStorage, Object.keys(localStorage).join()]
    sessionStorage.setItem('s', '1')
    out.location = [location.href, location.hash, location.pathname, location.origin]
    globalThis.hashes = []
    addEventListener('hashchange', (e) => hashes.push(e.newURL))
    location.hash = 'setup'
    history.replaceState({ step: 1 }, '', '?wp=2#setup')
    out.history = [location.search, location.hash, history.state.step, history.length]
    const mq = matchMedia('(prefers-reduced-motion: reduce)')
    out.media = [mq.matches, matchMedia('(min-width: 1000px)').matches, matchMedia('(max-width: 1000px)').matches]
    out.nav = [navigator.platform, navigator.language, typeof navigator.clipboard.writeText, navigator.userAgent.includes('Chrome')]
    out.ua = / acdert\\/\\d+\\.\\d+\\.\\d+$/.test(navigator.userAgent)
    out.view = [innerWidth, innerHeight, devicePixelRatio, screen.width]
    return out
  })()`)
  assert.deepEqual(result, {
    storage: ['dark', 'dark', 1],
    storageAfter: ['ada', '2', false, 'name,n'],
    location: ['http://acde.local/index.html#desk', '#desk', '/index.html', 'http://acde.local'],
    history: ['?wp=2', '#setup', 1, 2],
    media: [true, true, false],
    nav: ['Linux x86_64', 'en-US', 'function', false],
    ua: true,
    view: [1280, 800, 1, 1280],
  })
  assert.deepEqual(Object.fromEntries(storage.items), { name: 'ada', n: '2' })
  await web.wait(5)
  assert.deepEqual(web.json('hashes'), ['http://acde.local/index.html#setup'])
  web.run('globalThis.mediaLog = []; matchMedia("(prefers-reduced-motion: reduce)").addEventListener("change", (e) => mediaLog.push(e.matches))')
  web.dom.media['prefers-reduced-motion'] = 'no-preference'
  web.internals.mediaChanged()
  assert.deepEqual(web.json('mediaLog'), [false])
  web.run('globalThis.resized = 0; addEventListener("resize", () => resized++)')
  web.dom.view = { width: 800, height: 600, scale: 2 }
  web.internals.resize()
  assert.deepEqual(web.json('[resized, innerWidth, devicePixelRatio]'), [1, 800, 2])
})

test('encoding, URL, structuredClone, crypto, atob, MessageChannel, AbortSignal.timeout, console', async () => {
  using web = await page()
  const result = await web.run<Promise<string>>(`(async () => {
    const out = []
    const bytes = new TextEncoder().encode('héllo 你好')
    out.push(bytes.length, new TextDecoder().decode(bytes))
    const d = new TextDecoder()
    out.push(d.decode(bytes.subarray(0, 2), { stream: true }) + d.decode(bytes.subarray(2)))
    const u = new URL('../a/b?x=1&y=2#h', 'http://acde.local/ui/app/')
    u.searchParams.set('x', '3')
    out.push(u.href, u.searchParams.get('y'), new URLSearchParams({ a: 'b c' }).toString())
    const clone = structuredClone({ a: [1, { b: 2 }], d: new Date(0) })
    out.push(clone.a[1].b, clone.d.getTime())
    out.push(crypto.getRandomValues(new Uint8Array(4)).length, crypto.randomUUID().length, btoa('hi'), atob('aGk='))
    const channel = new MessageChannel()
    const got = await new Promise((resolve) => { channel.port1.onmessage = (e) => resolve(e.data); channel.port2.postMessage({ hi: 1 }) })
    out.push(got.hi)
    const signal = AbortSignal.timeout(1)
    await new Promise((resolve) => signal.addEventListener('abort', resolve))
    out.push(signal.reason.name)
    console.log('logged %s %d', 'x', 5, { k: [1] })
    console.warn(document.body)
    return JSON.stringify(out)
  })()`)
  assert.deepEqual(JSON.parse(result), [13, 'héllo 你好', 'héllo 你好', 'http://acde.local/ui/a/b?x=3&y=2#h', '2', 'a=b+c', 2, 0, 4, 36, 'aGk=', 'hi', 1, 'TimeoutError'])
  assert.deepEqual(web.host.logs, [[0, 'logged x 5 { k: [ 1 ] }'], [1, '<body>']])
})

test('TextDecoder: the labels of the Encoding Standard, streaming any of them, RangeError for the rest', async () => {
  using web = await page()
  const result = web.run<string>(`(() => {
    const samples = [
      ['gb18030', [0xc4, 0xe3, 0xba, 0xc3, 0x81, 0x30, 0x81, 0x30]], ['GBK', [0xc4, 0xe3, 0xba, 0xc3]], ['big5', [0xa4, 0xa4, 0xa4, 0xe5]],
      ['sjis', [0x93, 0xfa, 0x96, 0x7b]], ['euc-kr', [0xc7, 0xd1, 0xb1, 0xb9]], ['cp1251', [0xcf, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2]],
      ['windows-1250', [0xe8]], ['latin1', [0x80]],
    ]
    const out = []
    for (const [label, bytes] of samples) {
      const d = new TextDecoder(label)
      const whole = d.decode(new Uint8Array(bytes))
      const split = d.decode(new Uint8Array(bytes.slice(0, 1)), { stream: true }) + d.decode(new Uint8Array(bytes.slice(1)))
      out.push([d.encoding, whole, split === whole])
    }
    for (const label of ['', 'cp936', 'iso-2022-kr', 'nope']) {
      try { new TextDecoder(label); out.push('constructed ' + label) } catch (e) { out.push(e.name) }
    }
    try { new TextDecoder('gbk', { fatal: true }).decode(new Uint8Array([0xc4, 0xe3, 0xba])); out.push('decoded?') } catch (e) { out.push(e.name) }
    const fatal = new TextDecoder('gbk', { fatal: true })
    try { fatal.decode(new Uint8Array([0xc4]), { stream: true }); fatal.decode(new Uint8Array([0x30, 0x80])) } catch (e) { out.push(e.name) }
    out.push(fatal.decode(new Uint8Array([0xc4, 0xe3])))
    return JSON.stringify(out)
  })()`)
  assert.deepEqual(JSON.parse(result), [
    ['gb18030', '你好\u0080', true], ['gbk', '你好', true], ['big5', '中文', true], ['shift_jis', '日本', true], ['euc-kr', '한국', true],
    ['windows-1251', 'Привет', true], ['windows-1250', 'č', true], ['windows-1252', '€', true],
    'RangeError', 'RangeError', 'RangeError', 'RangeError', 'TypeError', 'TypeError', '你',
  ])
  assert.equal(web.host.decoders.size, 0, 'every streaming decoder was closed by its last chunk or its error')
})

test('canvas 2D commands reach render in page order; measureText; Path2D', async () => {
  using web = await page('<!doctype html><html><body><canvas id="c" width="100" height="50"></canvas></body></html>')
  web.run(`
    const ctx = document.getElementById('c').getContext('2d')
    ctx.fillStyle = '#ff0000'
    ctx.fillRect(0, 0, 10, 10)
    ctx.fill(new Path2D('M0 0 L10 0 A5 5 0 0 1 10 10 Z'))
    const off = document.createElement('canvas')
    off.getContext('2d').fillRect(1, 1, 2, 2)
    ctx.drawImage(off, 0, 0)
    globalThis.width = ctx.measureText('abc').width
    globalThis.webgl = document.createElement('canvas').getContext('webgl')
  `)
  const canvas = web.internals.idOf(web.run('document.getElementById("c")'))
  web.endTask()
  const commands = web.dom.canvasCommands.get(canvas)!
  assert.ok(commands.length >= 2, 'the run before the other canvas drew went first')
  assert.equal(web.run('width'), 18)
  assert.equal(web.run('webgl'), null)
})

test('attachShadow is a flattened root: CodeMirror-style use works', async () => {
  using web = await page('<!doctype html><html><body><div id="host"></div></body></html>')
  const result = web.json(`(() => {
    const host = document.getElementById('host')
    const root = host.shadowRoot ?? host.attachShadow({ mode: 'open' })
    const link = document.createElement('link'); link.rel = 'stylesheet'; link.href = '/editor.css'
    const parent = document.createElement('div')
    root.replaceChildren(link, parent)
    const style = document.createElement('style'); style.textContent = '.cm {}'
    root.insertBefore(style, root.firstChild)
    const editable = document.createElement('div'); editable.contentEditable = 'true'; parent.appendChild(editable)
    editable.focus()
    let origin
    host.addEventListener('keydown', (e) => { origin = e.composedPath()[0] === editable })
    editable.dispatchEvent(new KeyboardEvent('keydown', { key: 'a', bubbles: true, composed: true }))
    return {
      same: host.shadowRoot === root, nodeType: root.nodeType, host: root.host === host, kids: host.children.length,
      active: root.activeElement === editable, selection: root.getSelection() === document.getSelection(),
      origin, adopted: 'adoptedStyleSheets' in document, isRoot: root instanceof ShadowRoot, editable: editable.isContentEditable,
    }
  })()`)
  assert.deepEqual(result, { same: true, nodeType: 11, host: true, kids: 3, active: true, selection: true, origin: true, adopted: false, isRoot: true, editable: true })
})

test('custom elements: define, create, upgrade, callbacks', async () => {
  using web = await page('<!doctype html><html><body><x-early id="early"></x-early></body></html>')
  const result = web.json(`(() => {
    const log = []
    const early = document.getElementById('early')
    class XEl extends HTMLElement {
      static observedAttributes = ['mode']
      constructor() { super(); this.made = true }
      connectedCallback() { log.push('connected ' + this.id) }
      attributeChangedCallback(n, o, v) { log.push(n + ' ' + o + '>' + v) }
    }
    customElements.define('x-early', XEl)
    const upgraded = early instanceof XEl && early.made
    const fresh = document.createElement('x-early'); fresh.id = 'fresh'
    fresh.setAttribute('mode', 'a')
    document.body.appendChild(fresh)
    const byNew = new XEl()
    return { upgraded, log, fresh: fresh instanceof XEl, byNew: byNew.localName, get: customElements.get('x-early') === XEl }
  })()`)
  assert.deepEqual(result, { upgraded: true, log: ['connected early', 'mode null>a', 'connected fresh'], fresh: true, byNew: 'x-early', get: true })
})

test('installCore gives the Broker isolate the web globals without a DOM', () => {
  const host = new MockHost()
  const context = vm.createContext({})
  host.context = context
  vm.runInContext(bundle('src/core/index.ts', '__acdeCore'), context)
  context['__host'] = host
  vm.runInContext('__acdeCore.installCore(globalThis, __host, { nodeTimers: true })', context)
  const result = vm.runInContext(`JSON.stringify([
    typeof document, typeof URL, typeof TextEncoder, typeof AbortController, typeof EventTarget, typeof Blob, typeof MessageChannel,
    typeof setTimeout(() => {}, 1).unref, new URL('http://a/b').pathname, typeof structuredClone, typeof performance.now(),
  ])`, context) as string
  assert.deepEqual(JSON.parse(result), ['undefined', 'function', 'function', 'function', 'function', 'function', 'function', 'function', '/b', 'function', 'number'])
  host.dispose()
})

test('alert goes to the host when it takes it; Notification is denied and fires error', async () => {
  using web = await page()
  const alerts: string[] = []
  web.internals.env.alert = (message) => void alerts.push(message)
  web.run(`alert('saved'); alert()`)
  assert.deepEqual(alerts, ['saved', ''])
  web.run(`globalThis.out = []
    Notification.requestPermission((p) => out.push('callback ' + p)).then((p) => out.push('promise ' + p))
    const n = new Notification('hi', { body: 'there' })
    n.onerror = () => out.push('error ' + n.title + ' ' + n.body)
    out.push(Notification.permission, String(n))`)
  await web.wait(5)
  assert.deepEqual(web.json<string[]>('out').sort(), ['[object Notification]', 'callback denied', 'denied', 'error hi there', 'promise denied'])
})

test('Path2D strings go to render as they are, with addPath transforms composed', async () => {
  using web = await page('<!doctype html><html><body><canvas id="c" width="100" height="50"></canvas></body></html>')
  web.run(`
    const ctx = document.getElementById('c').getContext('2d')
    const outer = new Path2D()
    outer.addPath(new Path2D('M0 0 h10 v10 z'), { a: 2, d: 2, e: 5 })
    const twice = new Path2D()
    twice.addPath(outer, { e: 1, f: 3 })
    ctx.fill(twice)
  `)
  const canvas = web.internals.idOf(web.run('document.getElementById("c")'))
  web.endTask()
  const bytes = Buffer.concat(web.dom.canvasCommands.get(canvas)!)
  const d = Buffer.from('M0 0 h10 v10 z')
  const at = bytes.indexOf(d)
  assert.ok(at > 5 && bytes[at - 5] === 31 && bytes.readUInt32LE(at - 4) === d.length, 'opcode 31, then the string')
  const m = [0, 1, 2, 3, 4, 5].map((i) => bytes.readFloatLE(at + d.length + i * 4))
  assert.deepEqual(m, [2, 0, 0, 2, 6, 3])
})
