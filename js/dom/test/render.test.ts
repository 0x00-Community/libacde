import assert from 'node:assert/strict'
import { test } from 'node:test'
import { page } from './harness.ts'

test("a <style>'s sheet is render's: insertRule styles the page, the text stays, document.styleSheets lists it", async () => {
  using web = await page('<!doctype html><html><head><style>p { margin: 0 }</style></head><body><p id="p">x</p></body></html>')
  const result = web.json(`(() => {
    const parsed = document.querySelector('style')
    const style = document.createElement('style')
    const detached = style.sheet
    document.head.appendChild(style)
    const sheet = style.sheet
    sheet.insertRule('p { color: rgb(255, 0, 0) }', sheet.cssRules.length)
    sheet.insertRule('.x { color: blue }', sheet.cssRules.length)
    const texts = [...sheet.cssRules].map((r) => r.cssText)
    let syntax = ''
    try { sheet.insertRule('p', 0) } catch (e) { syntax = e.name }
    let index = ''
    try { sheet.insertRule('a {}', 9) } catch (e) { index = e.name }
    sheet.deleteRule(1)
    return {
      detached, same: style.sheet === sheet, owner: sheet.ownerNode === style, count: sheet.cssRules.length, first: sheet.cssRules[0].cssText,
      item: sheet.cssRules.item(0).selectorText, texts, syntax, index, text: style.textContent,
      sheets: document.styleSheets.length, parsedRules: parsed.sheet.cssRules.length, listed: document.styleSheets[1] === sheet,
    }
  })()`)
  assert.deepEqual(result, {
    detached: null, same: true, owner: true, count: 1, first: 'p { color: rgb(255, 0, 0); }', item: 'p',
    texts: ['p { color: rgb(255, 0, 0); }', '.x { color: blue; }'], syntax: 'SyntaxError', index: 'IndexSizeError', text: '',
    sheets: 2, parsedRules: 1, listed: true,
  })
})

test('canvas colours are parsed by render: every CSS name and syntax, invalid ones ignored', async () => {
  using web = await page('<!doctype html><html><body><canvas id="c" width="10" height="10"></canvas></body></html>')
  const result = web.json(`(() => {
    const ctx = document.getElementById('c').getContext('2d')
    const seen = []
    for (const color of ['darkblue', 'rebeccapurple', 'oklch(0.628 0.2577 29.23)', 'not-a-colour']) {
      ctx.fillStyle = color
      seen.push(ctx.fillStyle)
      ctx.fillRect(0, 0, 1, 1)
    }
    return seen
  })()`)
  assert.deepEqual(result, ['darkblue', 'rebeccapurple', 'oklch(0.628 0.2577 29.23)', 'oklch(0.628 0.2577 29.23)'])
  web.endTask()
  const canvas = web.internals.idOf(web.run('document.getElementById("c")'))
  const bytes = Buffer.concat(web.dom.canvasCommands.get(canvas)!)
  const fills: number[][] = []
  for (let i = 0; i + 18 <= bytes.length; i++) {
    if (bytes[i] !== 4 || bytes[i + 1] !== 0) continue
    const rgba = [0, 1, 2, 3].map((k) => Math.round(bytes.readFloatLE(i + 2 + k * 4) * 255))
    if (rgba[3] === 255) fills.push(rgba)
  }
  assert.deepEqual(fills, [[0, 0, 139, 255], [102, 51, 153, 255], [255, 0, 0, 255]])
  assert.equal(web.dom.calls.get('parseColor'), 4, 'one native parse per distinct colour')
})

test("DOMParser parses a whole page with render's HTML parser (head, body, doctype, <html> attributes)", async () => {
  using web = await page()
  const result = web.json(`(() => {
    const doc = new DOMParser().parseFromString('<!doctype html><html lang="fr"><head><title>T</title><style>p{}</style></head><body class="b"><p>a</p><!-- </body> --><p>b</p></body></html>', 'text/html')
    const bare = new DOMParser().parseFromString('<p>only</p>', 'text/html')
    return {
      title: doc.title, lang: doc.documentElement.getAttribute('lang'), doctype: doc.doctype && doc.doctype.name, head: doc.head.innerHTML,
      body: doc.body.innerHTML, bodyClass: doc.body.className, connected: doc.body.isConnected, scripts: doc.scripts === undefined,
      bare: bare.body.innerHTML, bareHead: bare.head !== null, live: document.body.childNodes.length,
    }
  })()`)
  assert.deepEqual(result, {
    title: 'T', lang: 'fr', doctype: 'html', head: '<title>T</title><style>p{}</style>', body: '<p>a</p><!-- </body> --><p>b</p>', bodyClass: 'b',
    connected: false, scripts: false, bare: '<p>only</p>', bareHead: true, live: 0,
  })
})
