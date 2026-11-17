import assert from 'node:assert/strict'
import { test } from 'node:test'
import { page, type Web } from './harness.ts'

const FAKE = `(() => {
  const seen = globalThis.seen = { calls: [], console: [], requests: [] }
  const enc = new TextEncoder()
  const answer = (on, status, headers, body) => setTimeout(() => {
    on({ type: 'head', status, statusText: status === 200 ? 'OK' : 'X', headers })
    if (body) on({ type: 'chunk', data: enc.encode(body) })
    on({ type: 'end' })
  }, 1)
  __acdeDom.observe({
    console: (level, text) => seen.console.push([level, text]),
    request: (e) => e.phase === 'start' ? seen.requests.push([e.kind, e.url]) : e.phase === 'error' && seen.requests.push(['error', e.message]),
  })
  __acdeDom.installBackendTransport({
    port: 3000,
    request(to, method, path, headers, body, on) {
      const url = typeof to === 'string' ? to : 'backend:' + to + path
      seen.calls.push([method, url, Object.fromEntries(headers)])
      const any = ['access-control-allow-origin', '*']
      const p = typeof to === 'string' ? new URL(to).pathname : path
      if (p === '/a.png' || p === '/api/pic.png') answer(on, 200, [['content-type', 'image/png']], 'PNG')
      else if (p === '/missing.png') answer(on, 404, [['content-type', 'image/png']], 'PNG404')
      else if (p === '/site.css') answer(on, 200, [['content-type', 'text/css; charset=utf-8']], 'body{color:red}')
      else if (p === '/plain.css') answer(on, 200, [['content-type', 'text/plain']], 'body{}')
      else if (p === '/open.css') answer(on, 200, [any, ['content-type', 'text/css']], 'body{color:blue}')
      else if (p === '/moved.css' || p === '/api/moved.css') answer(on, 302, [['location', 'https://cdn.example.com/site.css']], '')
      else if (p === '/api/own.css') answer(on, 200, [['content-type', 'text/css']], 'body{color:green}')
      else if (p === '/closed.woff2') answer(on, 200, [['content-type', 'font/woff2']], 'wOF2')
      else if (p === '/open.woff2') answer(on, 200, [any, ['content-type', 'font/woff2']], 'wOF2')
      else setTimeout(() => on({ type: 'error', message: 'connect: refused' }), 1)
      return seen.calls.length
    },
    abort() {},
    websocket() { throw new Error('no') },
    send() {},
    close() {},
  })
})()`

async function load(web: Web, url: string, destination: string): Promise<string | null> {
  const id = web.dom.loads.size + 1
  web.dom.loader!(id, url, destination, '')
  for (let i = 0; i < 50 && !web.dom.loads.has(id); i++) await web.wait(2)
  assert.ok(web.dom.loads.has(id), `${url} was answered`)
  const bytes = web.dom.loads.get(id)
  return bytes ? new TextDecoder().decode(bytes) : null
}

test('remote images, stylesheets and fonts load through the page network stack with their destination rules', async () => {
  using web = await page()
  web.run(FAKE)
  assert.equal(await load(web, 'https://img.example.com/a.png', 'image'), 'PNG')
  assert.equal(await load(web, 'https://img.example.com/missing.png', 'image'), 'PNG404')
  assert.equal(await load(web, 'https://cdn.example.com/site.css', 'style'), 'body{color:red}')
  assert.equal(await load(web, 'https://cdn.example.com/plain.css', 'style'), null)
  assert.equal(await load(web, 'https://other.example.com/moved.css', 'style'), 'body{color:red}')
  assert.equal(await load(web, 'https://fonts.example.com/closed.woff2', 'font'), null)
  assert.equal(await load(web, 'https://fonts.example.com/open.woff2', 'font'), 'wOF2')
  assert.equal(await load(web, 'http://acde.local/api/pic.png', 'image'), 'PNG')
  assert.equal(await load(web, 'https://down.example.com/x.png', 'image'), null)
  const seen = web.json<{ calls: [string, string, Record<string, string>][]; console: [string, string][]; requests: string[][] }>('seen')
  const image = seen.calls[0]![2]
  const accept = 'image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8'
  assert.deepEqual([image['origin'] ?? null, image['referer'], image['accept']], [null, 'http://acde.local/', accept])
  const style = seen.calls[2]![2]
  assert.deepEqual([style['origin'] ?? null, style['accept']], [null, 'text/css,*/*;q=0.1'])
  const font = seen.calls.find(([, url]) => url.endsWith('/open.woff2'))![2]
  assert.equal(font['origin'], 'http://acde.local')
  assert.deepEqual(seen.calls.find(([, url]) => url.startsWith('backend:'))!.slice(0, 2), ['GET', 'backend:3000/api/pic.png'])
  assert.deepEqual(seen.requests.filter(([kind]) => kind !== 'error').map(([kind]) => kind), [
    'image', 'image', 'stylesheet', 'stylesheet', 'stylesheet', 'font', 'font', 'image', 'image',
  ])
  assert.deepEqual(seen.console.map(([level, text]) => [level, text.split(': ').slice(0, 2).join(': ')]), [
    ['error', 'Failed to load resource: https://img.example.com/missing.png'],
    ['error', 'Failed to load resource: https://cdn.example.com/plain.css'],
    ['error', 'Failed to load resource: https://fonts.example.com/closed.woff2'],
    ['error', 'Failed to load resource: https://down.example.com/x.png'],
  ])
  assert.deepEqual(web.host.errors, [])
})

test('a page without network gets no remote resources and is told why', async () => {
  using web = await page()
  web.run(`globalThis.seen = []; __acdeDom.observe({ console: (level, text) => seen.push([level, text]) })`)
  assert.equal(await load(web, 'https://img.example.com/a.png', 'image'), null)
  assert.deepEqual(web.json('seen'), [['error', 'Failed to load resource: https://img.example.com/a.png: another host, and this page has no network']])
})

test('a link fires load or error by what its remote stylesheet answered, and crossorigin makes the request a CORS one', async () => {
  using web = await page('<!doctype html><html><head></head><body></body></html>')
  web.run(FAKE)
  web.run(`
    globalThis.log = []
    globalThis.add = (name, href, crossorigin) => {
      const link = document.createElement('link'); link.rel = 'stylesheet'; link.href = href
      if (crossorigin !== undefined) link.crossOrigin = crossorigin
      link.onload = () => log.push(name + ' load'); link.onerror = () => log.push(name + ' error')
      document.head.append(link)
      return link
    }
  `)
  const fetch = async (name: string, href: string, crossorigin?: string) => {
    const id = web.internals.idOf(web.run(`add(${JSON.stringify(name)}, ${JSON.stringify(href)}, ${JSON.stringify(crossorigin)})`))
    web.dom._fetchSheet(id, href, crossorigin === undefined ? '' : crossorigin === 'use-credentials' ? crossorigin : 'anonymous')
    for (let i = 0; i < 50 && web.dom.sheetState(id) === 0; i++) await web.wait(2)
    await web.wait(2)
  }
  await fetch('plain', 'https://cdn.example.com/site.css')
  await fetch('mime', 'https://cdn.example.com/plain.css')
  await fetch('down', 'https://down.example.com/x.css')
  await fetch('closed', 'https://cdn.example.com/site.css', '')
  await fetch('open', 'https://cdn.example.com/open.css', 'anonymous')
  await fetch('credentialed', 'https://cdn.example.com/open.css', 'use-credentials')
  await fetch('redirected', 'http://acde.local/api/moved.css')
  await fetch('own', 'http://acde.local/api/own.css')
  assert.deepEqual(web.json('log'), ['plain load', 'mime error', 'down error', 'closed error', 'open load', 'credentialed error', 'redirected load', 'own load'])
  const seen = web.json<{ calls: [string, string, Record<string, string>][] }>('seen')
  assert.deepEqual(seen.calls.slice(0, 6).map(([, url, headers]) => [new URL(url).pathname, headers['origin'] ?? null]), [
    ['/site.css', null], ['/plain.css', null], ['/x.css', null], ['/site.css', 'http://acde.local'], ['/open.css', 'http://acde.local'], ['/open.css', 'http://acde.local'],
  ])
  const sheets = web.json<(string | null)[][]>(`[...document.styleSheets].map((sheet) => {
    let rules
    try { rules = String(sheet.cssRules.length) } catch (error) { rules = error.name }
    return [sheet.href, sheet.ownerNode.crossOrigin, rules]
  })`)
  assert.deepEqual(sheets, [
    ['https://cdn.example.com/site.css', null, 'SecurityError'],
    ['https://cdn.example.com/open.css', 'anonymous', '0'],
    ['http://acde.local/api/moved.css', null, 'SecurityError'],
    ['http://acde.local/api/own.css', null, '0'],
  ])
  assert.deepEqual(web.host.errors, [])
})

test('FontFace.load fetches its url sources as fonts in order and reports the outcome', async () => {
  using web = await page()
  web.run(FAKE)
  web.run(`
    globalThis.out = []
    const both = new FontFace('Both', 'url(https://fonts.example.com/closed.woff2) format("woff2"), url("https://fonts.example.com/open.woff2")')
    const closed = new FontFace('Closed', "url('https://fonts.example.com/closed.woff2')")
    const bytes = new FontFace('Bytes', new Uint8Array([0, 1]))
    out.push(both.status, closed.status, bytes.status)
    both.load().then((face) => out.push(face === both, both.status))
    out.push(both.status)
    closed.load().catch((e) => out.push(e.name, closed.status))
  `)
  for (let i = 0; i < 50 && web.json<unknown[]>('out').length < 8; i++) await web.wait(2)
  assert.deepEqual(web.json('out'), ['unloaded', 'unloaded', 'loaded', 'loading', 'NetworkError', 'error', true, 'loaded'])
  const fonts = web.json<string[][]>('seen.requests').filter(([kind]) => kind === 'font').map(([, url]) => url)
  assert.deepEqual(fonts, ['https://fonts.example.com/closed.woff2', 'https://fonts.example.com/closed.woff2', 'https://fonts.example.com/open.woff2'])
})

test('images: a clone of a loaded image fires load once when it is inserted, even when no frame follows', async () => {
  using web = await page('<!doctype html><html><body><img id=a src="/a.png"></body></html>')
  await web.wait(5)
  web.run(`globalThis.log = []
    const a = document.getElementById('a')
    a.onload = () => log.push('original')
    globalThis.copy = a.cloneNode()
    copy.onload = () => log.push('load')
    copy.onerror = () => log.push('error')
    document.body.append(copy)`)
  web.dom._imageDone(web.internals.idOf(web.run('copy')), [16, 8, 1])
  web.endTask()
  await web.wait(5)
  assert.deepEqual(web.json('log'), ['load'])
  await web.wait(5)
  assert.deepEqual(web.json('log'), ['load'], 'and only once')
})
