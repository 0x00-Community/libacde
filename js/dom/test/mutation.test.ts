import assert from 'node:assert/strict'
import { test } from 'node:test'
import { page } from './harness.ts'

test('MutationObserver: childList, attributes with old values and filters, characterData, subtree, takeRecords, microtask delivery', async () => {
  using web = await page('<!doctype html><html><body><div id="host"><p id="p">one</p></div></body></html>')
  web.run(`
    globalThis.records = []
    const host = document.getElementById('host')
    globalThis.observer = new MutationObserver((list, obs) => {
      for (const r of list) records.push({
        type: r.type, target: r.target.id || r.target.nodeName, name: r.attributeName, old: r.oldValue,
        added: [...r.addedNodes].map((n) => n.id || n.nodeName), removed: [...r.removedNodes].map((n) => n.id || n.nodeName),
        prev: r.previousSibling && (r.previousSibling.id || r.previousSibling.nodeName), next: r.nextSibling && (r.nextSibling.id || r.nextSibling.nodeName),
      })
      records.push(obs === observer ? 'same observer' : 'other')
    })
    observer.observe(host, { childList: true, subtree: true, attributes: true, attributeOldValue: true, attributeFilter: ['class', 'title'], characterData: true, characterDataOldValue: true })
    const p = document.getElementById('p')
    const q = document.createElement('q'); q.id = 'q'
    host.appendChild(q)
    p.className = 'a'
    p.className = 'b'
    p.setAttribute('data-x', '1')
    p.firstChild.data = 'two'
    q.remove()
    host.insertBefore(q, p)
    globalThis.sync = records.length
  `)
  assert.equal(web.run('sync'), 0, 'records are delivered in a microtask, not synchronously')
  await web.wait()
  const records = web.json<unknown[]>('records')
  assert.deepEqual(records, [
    { type: 'childList', target: 'host', name: null, old: null, added: ['q'], removed: [], prev: 'p', next: null },
    { type: 'attributes', target: 'p', name: 'class', old: null, added: [], removed: [], prev: null, next: null },
    { type: 'attributes', target: 'p', name: 'class', old: 'a', added: [], removed: [], prev: null, next: null },
    { type: 'characterData', target: '#text', name: null, old: 'one', added: [], removed: [], prev: null, next: null },
    { type: 'childList', target: 'host', name: null, old: null, added: [], removed: ['q'], prev: 'p', next: null },
    { type: 'childList', target: 'host', name: null, old: null, added: ['q'], removed: [], prev: null, next: 'p' },
    'same observer',
  ])
  const later = web.json(`(() => {
    records.length = 0
    document.getElementById('host').innerHTML = '<i>x</i><i>y</i>'
    const taken = observer.takeRecords().map((r) => [r.type, r.addedNodes.length, r.removedNodes.length])
    observer.disconnect()
    document.getElementById('host').textContent = 'gone'
    return taken
  })()`)
  assert.deepEqual(later, [['childList', 2, 2]])
  await web.wait()
  assert.deepEqual(web.json('records'), [])
})

test('textContent and replaceChildren and fragments produce one record each', async () => {
  using web = await page('<!doctype html><html><body><ul id="l"><li>a</li><li>b</li></ul></body></html>')
  web.run(`
    globalThis.seen = []
    const l = document.getElementById('l')
    new MutationObserver((rs) => rs.forEach((r) => seen.push(r.target.nodeName + ' +' + r.addedNodes.length + ' -' + r.removedNodes.length))).observe(document.body, { childList: true, subtree: true })
    const f = document.createDocumentFragment()
    f.append('t', document.createElement('li'))
    l.appendChild(f)
    l.textContent = 'flat'
    l.replaceChildren(document.createElement('li'), 'tail')
  `)
  await web.wait()
  assert.deepEqual(web.json('seen'), ['UL +2 -0', 'UL +1 -4', 'UL +2 -1'])
})
