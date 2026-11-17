import assert from 'node:assert/strict'
import { test } from 'node:test'
import { callsOf, page } from './harness.ts'

test('child lists are re-read only when children come or go, not when attributes or text change', async () => {
  using web = await page(`<!doctype html><html><body><ul id="l">${'<li>x</li>'.repeat(200)}</ul></body></html>`)
  const calls = callsOf(web, `
    const list = document.getElementById('l')
    for (let i = 0; i < list.childNodes.length; i++) list.childNodes[i].setAttribute('data-i', String(i))
    for (let i = 0; i < list.children.length; i++) list.children[i].firstChild.data = 'y' + i
    void document.body; void document.head; void document.documentElement; void document.body
  `)
  assert.equal(calls.get('childNodes'), 1)
  assert.equal(calls.get('children') ?? 0, 1, 'the children list once; head / body / documentElement known since the page was parsed')
  assert.equal(calls.get('querySelector') ?? 0, 0)
  assert.equal(web.run('document.getElementById("l").childNodes[199].getAttribute("data-i")'), '199')
  assert.equal(web.run('const l = document.getElementById("l"); l.append(document.createElement("li")); l.childNodes.length + " " + l.children.length'), '201 201')
})

test('style.setProperty reads the attribute only after a mutation; computed values are kept until something changes', async () => {
  using web = await page('<!doctype html><html><body><div id="d" style="color: red"></div></body></html>')
  const set = callsOf(web, `
    const d = document.getElementById('d')
    for (let i = 0; i < 50; i++) d.style.transform = 'translateX(' + i + 'px)'
    for (let i = 0; i < 50; i++) d.style.transform = 'translateX(49px)'
  `)
  assert.ok((set.get('getAttribute') ?? 0) <= 50, `getAttribute ${set.get('getAttribute')}`)
  assert.equal(set.get('setAttribute'), 50, 'the same value again writes nothing')
  assert.equal(web.run('document.getElementById("d").getAttribute("style")'), 'color: red; transform: translateX(49px);')
  const read = callsOf(web, `
    const s = getComputedStyle(document.getElementById('d'))
    globalThis.c1 = s.color; globalThis.c2 = getComputedStyle(document.getElementById('d')).color
  `)
  assert.equal(read.get('computedStyle'), 1)
  const after = callsOf(web, `document.getElementById('d').style.color = 'blue'; globalThis.c3 = getComputedStyle(document.getElementById('d')).color`)
  assert.equal(after.get('computedStyle'), 1)
  assert.equal(web.run('c3'), 'blue')
})

test('live ranges move with the tree; ranges in text cost unrelated mutations nothing', async () => {
  using web = await page('<!doctype html><html><body><div id="a"><b>1</b><b>2</b><b>3</b></div><p id="t">text</p></body></html>')
  const result = web.json(`(() => {
    const a = document.getElementById('a')
    const r = document.createRange()
    r.setStart(a, 2); r.setEnd(a, 3)
    a.insertBefore(document.createElement('i'), a.firstChild)
    const afterInsert = [r.startOffset, r.endOffset]
    a.firstChild.remove()
    a.firstChild.remove()
    const afterRemove = [r.startOffset, r.endOffset]
    const inner = document.createRange()
    inner.selectNodeContents(a.lastChild)
    a.lastChild.remove()
    const removedAround = [inner.startContainer === a, inner.startOffset, inner.collapsed]
    a.innerHTML = '<u>x</u><u>y</u>'
    const replaced = [r.startContainer === a, r.startOffset, r.endOffset]
    const sel = getSelection()
    const t = document.getElementById('t').firstChild
    sel.collapse(a, 1)
    const taken = sel.getRangeAt(0)
    sel.collapse(t, 2)
    a.insertBefore(document.createElement('i'), a.firstChild)
    return { afterInsert, afterRemove, removedAround, replaced, taken: [taken.startOffset, sel.anchorNode === t, sel.anchorOffset] }
  })()`)
  assert.deepEqual(result, { afterInsert: [3, 4], afterRemove: [1, 2], removedAround: [true, 1, true], replaced: [true, 0, 0], taken: [2, true, 2] })
  const calls = callsOf(web, `
    getSelection().collapse(document.getElementById('t').firstChild, 1)
    globalThis.fresh = document.createRange()
    const host = document.body.appendChild(document.createElement('section'))
    for (let i = 0; i < 100; i++) host.appendChild(document.createElement('span'))
  `)
  assert.equal(calls.get('childNodes') ?? 0, 0)
  assert.equal(calls.get('parent') ?? 0, 0)
})

test('Range contents across levels: toString, cloneContents, extractContents, deleteContents', async () => {
  using web = await page('<!doctype html><html><body><div id="r"><p>ab<i>cd</i>ef</p><p>gh</p><p>i<b>jk</b>l</p></div></body></html>')
  const result = web.json(`(() => {
    const root = document.getElementById('r')
    const make = () => {
      const [p1, , p3] = root.children
      const r = document.createRange()
      r.setStart(p1.querySelector('i').firstChild, 1)
      r.setEnd(p3.querySelector('b').firstChild, 1)
      return r
    }
    const text = make().toString()
    const cloned = make().cloneContents()
    const clonedHtml = [...cloned.childNodes].map((n) => n.outerHTML ?? n.data).join('|')
    const before = root.innerHTML
    const r = make()
    const extracted = r.extractContents()
    const extractedHtml = [...extracted.childNodes].map((n) => n.outerHTML ?? n.data).join('|')
    const afterExtract = root.innerHTML
    const collapsed = [r.collapsed, r.startContainer === root, r.startOffset]
    root.innerHTML = before
    const d = make()
    d.deleteContents()
    return { text, clonedHtml, unchanged: root.innerHTML !== before, extractedHtml, afterExtract, collapsed, afterDelete: root.innerHTML }
  })()`)
  assert.deepEqual(result, {
    text: 'defghij',
    clonedHtml: '<p><i>d</i>ef</p>|<p>gh</p>|<p>i<b>j</b></p>',
    unchanged: true,
    extractedHtml: '<p><i>d</i>ef</p>|<p>gh</p>|<p>i<b>j</b></p>',
    afterExtract: '<p>ab<i>c</i></p><p><b>k</b>l</p>',
    collapsed: [true, true, 1],
    afterDelete: '<p>ab<i>c</i></p><p><b>k</b>l</p>',
  })
})

test('MutationObserver: a subtree record names its target even when the page never held it; no observer, no record work', async () => {
  using web = await page('<!doctype html><html><body><div id="o"><p><span id="s"></span></p></div><div id="x"></div></body></html>')
  const result = await web.run<Promise<string[]>>(`(async () => {
    const out = []
    new MutationObserver((records) => { for (const r of records) out.push(r.type + ' ' + r.target.nodeName + ' -' + r.removedNodes.length + ' +' + r.addedNodes.length) }).observe(document.getElementById('o'), { childList: true, subtree: true })
    document.getElementById('s').remove()
    document.getElementById('x').appendChild(document.createElement('b'))
    await null
    return out
  })()`)
  assert.deepEqual(JSON.parse(JSON.stringify(result)), ['childList P -1 +0'])
})
