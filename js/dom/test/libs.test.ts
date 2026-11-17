import assert from 'node:assert/strict'
import { test } from 'node:test'
import vm from 'node:vm'
import { bundle, page, type Web } from './harness.ts'

async function libsWeb(): Promise<Web> {
  using web = await page('<!doctype html><html><body><div id="editor"></div><div id="term"></div></body></html>')
  vm.runInContext(bundle('test/fixtures/libs.ts', '__libs', { 'process.env.NODE_ENV': '"production"' }), web.context, { filename: 'libs.js' })
  return web
}

test('CodeMirror 6 in a shadow root: mounts, styles itself, edits by transaction and by key', async () => {
  using web = await libsWeb()
  web.run(`
    const host = document.getElementById('editor')
    const root = host.attachShadow({ mode: 'open' })
    const parent = document.createElement('div')
    root.replaceChildren(parent)
    globalThis.view = new CM.EditorView({
      parent, root,
      state: CM.EditorState.create({ doc: 'line one\\nline two', extensions: [CM.lineNumbers(), CM.history(), CM.keymap.of([...CM.defaultKeymap, ...CM.historyKeymap])] }),
    })
  `)
  web.frame()
  await web.wait(5)
  web.frame()
  assert.equal(web.run('view.dom.isConnected'), true)
  assert.equal(web.run('document.querySelectorAll(".cm-line").length'), 2)
  assert.equal(web.run('!!document.querySelector("style")'), true, 'style-mod falls back to a <style> element (no adoptedStyleSheets)')
  web.run(`view.dispatch({ changes: { from: 0, to: 4, insert: 'LINE' } })`)
  assert.equal(web.run('view.state.doc.toString()'), 'LINE one\nline two')
  assert.equal(web.run('view.contentDOM.querySelector(".cm-line").textContent'), 'LINE one')
  web.run(`view.focus()`)
  web.internals.input.key('keydown', 'z', 'KeyZ', { modifiers: { ctrl: true } })
  assert.equal(web.run('view.state.doc.toString()'), 'line one\nline two')
  web.run(`view.dispatch({ selection: { anchor: 4 } })`)
  web.frame()
  await web.wait(5)
  web.frame()
  assert.equal(web.run('view.hasFocus'), true)
  web.run('view.destroy()')
  assert.deepEqual(web.host.errors.map(String), [])
})

test('xterm.js opens, writes and takes keys', async () => {
  using web = await libsWeb()
  web.run(`
    globalThis.term = new Terminal({ cols: 40, rows: 10 })
    globalThis.typed = []
    term.onData((data) => typed.push(data))
    term.open(document.getElementById('term'))
    term.write('hello acde')
  `)
  for (let i = 0; i < 5; i++) {
    web.frame()
    await web.wait(5)
  }
  assert.equal(web.run('!!document.querySelector(".xterm textarea")'), true)
  assert.equal(web.run('term.buffer.active.getLine(0).translateToString(true)'), 'hello acde')
  web.run('term.focus()')
  web.internals.input.key('keydown', 'x', 'KeyX')
  web.internals.input.key('keyup', 'x', 'KeyX')
  assert.deepEqual(web.json('typed'), ['x'])
  web.run('term.dispose()')
  assert.deepEqual(web.host.errors.map(String), [])
})

test('DOMPurify is supported and sanitizes into an inert document', async () => {
  using web = await libsWeb()
  const result = web.json(`(() => {
    const purify = DOMPurify(window)
    const fragment = purify.sanitize('<b>ok</b><img src="x" onerror="window.pwned = 1"><script>window.pwned = 2</script>', { RETURN_DOM_FRAGMENT: true })
    const html = purify.sanitize('<a href="javascript:alert(1)">x</a><p style="color:red">y</p>')
    return { supported: purify.isSupported, kids: [...fragment.childNodes].map((n) => n.nodeName), html, pwned: window.pwned ?? null }
  })()`)
  assert.deepEqual(result, { supported: true, kids: ['B', 'IMG'], html: '<a>x</a><p style="color:red">y</p>', pwned: null })
  await web.wait(5)
  web.frame()
  assert.equal(web.run('window.pwned ?? null'), null)
})
