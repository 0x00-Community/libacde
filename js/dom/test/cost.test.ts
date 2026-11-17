import assert from 'node:assert/strict'
import { test } from 'node:test'
import { callsOf, loadReact, page } from './harness.ts'

test('React mounting 2000 elements costs a few native calls each', async () => {
  using web = await page('<!doctype html><html><body><div id="root"></div><p id="sel">text</p></body></html>')
  loadReact(web)
  web.run('getSelection().collapse(document.getElementById("sel").firstChild, 1)')
  const start = performance.now()
  const counted = callsOf(web, `
    const h = React.createElement
    globalThis.root = ReactDOMClient.createRoot(document.getElementById('root'))
    React.act(() => root.render(h('ul', null, Array.from({ length: 2000 }, (_, i) => h('li', { key: i, className: 'row', title: 't' + i }, 'item ', i)))))
  `)
  const ms = performance.now() - start
  const calls = [...counted.values()].reduce((a, b) => a + b, 0)
  const perNode = calls / 2000
  const top = [...counted].sort((a, b) => b[1] - a[1]).slice(0, 8).map(([k, v]) => `${k} ${v}`).join(', ')
  console.log(`React mount, 2000 <li>: ${calls} native calls (${perNode.toFixed(2)} per <li>), ${ms.toFixed(0)} ms on the happy-dom mock; top: ${top}`)
  assert.equal(web.run('document.querySelectorAll("li").length'), 2000)
  assert.ok(perNode < 9, `${perNode} native calls per element`)
})

test('a form costs one query for each name read after a change, and none while nothing changes', async () => {
  using web = await page('<!doctype html><html><body><div id="root"></div></body></html>')
  loadReact(web)
  const mount = callsOf(web, `
    const h = React.createElement
    globalThis.root = ReactDOMClient.createRoot(document.getElementById('root'))
    React.act(() => root.render(h('form', { id: 'f' }, Array.from({ length: 500 }, (_, i) => h('input', { key: i, name: 'n' + i, defaultValue: 'v' })))))
    globalThis.f = document.getElementById('f')
  `)
  const calls = [...mount.values()].reduce((a, b) => a + b, 0)
  const top = [...mount].sort((a, b) => b[1] - a[1]).slice(0, 6).map(([k, v]) => `${k} ${v}`).join(', ')
  console.log(`React mount, a form of 500 <input>: ${calls} native calls (${(calls / 500).toFixed(2)} per <input>); top: ${top}`)
  assert.equal(web.run('f.length'), 500)
  assert.ok((mount.get('querySelectorAll') ?? 0) <= 510, `${mount.get('querySelectorAll')} queries for 500 appends`)
  const steady = callsOf(web, `for (let i = 0; i < 1000; i++) { f.n7; f.nodeType; f.appendChild; f.nothing; f.__reactFiber }`)
  const first = [...steady.values()].reduce((a, b) => a + b, 0)
  const again = callsOf(web, `for (let i = 0; i < 1000; i++) { f.n7; f.nodeType; f.appendChild; f.nothing; f.__reactFiber }`)
  assert.ok(first <= 12, `${first} native calls for the first reads: ${[...steady].join(' ')}`)
  assert.equal([...again.values()].reduce((a, b) => a + b, 0), 0)
})
