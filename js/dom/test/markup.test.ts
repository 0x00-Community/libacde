import assert from 'node:assert/strict'
import { test } from 'node:test'
import { page } from './harness.ts'

test('innerHTML / outerHTML / insertAdjacentHTML / template round trips', async () => {
  using web = await page('<!doctype html><html><body><div id="box"></div></body></html>')
  const result = web.json(`(() => {
    const box = document.getElementById('box')
    const html = '<p class="a">x <b>y</b></p><!--c--><svg viewBox="0 0 1 1"><path d="M0 0"></path></svg>'
    box.innerHTML = html
    const round = box.innerHTML === html
    const svg = box.querySelector('svg')
    const kinds = [...box.childNodes].map((n) => n.nodeType)
    box.querySelector('p').outerHTML = '<h2 id="h">t</h2>'
    box.insertAdjacentHTML('afterbegin', '<span id="first"></span>')
    box.insertAdjacentHTML('beforeend', '<span id="last"></span>')
    box.firstElementChild.insertAdjacentHTML('afterend', '<em>e</em>')
    const order = [...box.children].map((e) => e.localName)
    const template = document.createElement('template')
    template.innerHTML = '<li>1</li><li>2</li>'
    const fromTemplate = template.content.childNodes.length
    const clone = template.content.cloneNode(true)
    const h = (s) => { const t = document.createElement('template'); t.innerHTML = s; return t.content.firstElementChild }
    const made = h('<div class="card"><i>k</i></div>')
    return { round, svgNs: svg.namespaceURI, svgClass: svg instanceof SVGSVGElement, path: svg.firstChild instanceof SVGPathElement, kinds, order, fromTemplate, cloned: clone.childNodes.length, inBox: box.contains(svg), made: made.outerHTML, isElement: made instanceof HTMLDivElement, text: box.textContent }
  })()`)
  assert.deepEqual(result, {
    round: true, svgNs: 'http://www.w3.org/2000/svg', svgClass: true, path: true, kinds: [1, 8, 1], order: ['span', 'em', 'h2', 'svg', 'span'],
    fromTemplate: 2, cloned: 2, inBox: true, made: '<div class="card"><i>k</i></div>', isElement: true, text: 'et',
  })
})

test('classList, dataset, style, attributes, reflection', async () => {
  using web = await page('<!doctype html><html><body><div id="d" class="a b" data-user-id="7" style="width: min(860px, 60%); color: red"></div><a id="a" href="/x?y=1#z">l</a></body></html>')
  const result = web.json(`(() => {
    const d = document.getElementById('d')
    d.classList.add('c'); d.classList.remove('a'); d.classList.toggle('b'); const t = d.classList.toggle('z', true)
    const cls = d.className
    d.dataset.fooBar = 'baz'
    const data = { ...d.dataset }
    delete d.dataset.userId
    d.style.zIndex = 3
    d.style.setProperty('--accent', 'blue')
    d.style.removeProperty('color')
    const style = d.getAttribute('style')
    const a = document.getElementById('a')
    d.hidden = true; d.title = 'tt'; d.tabIndex = 2
    const attrs = [...d.attributes].map((x) => x.name)
    return { t, cls, data, has: d.hasAttribute('data-user-id'), style, width: d.style.width, accent: d.style.getPropertyValue('--accent'), href: a.href, hash: a.hash, pathname: a.pathname, hidden: d.hidden, attrs, tabIndex: d.tabIndex, aTab: a.tabIndex, divTab: document.body.tabIndex }
  })()`)
  assert.deepEqual(result, {
    t: true, cls: 'c z', data: { userId: '7', fooBar: 'baz' }, has: false,
    style: 'width: min(860px, 60%); z-index: 3; --accent: blue;', width: 'min(860px, 60%)', accent: 'blue',
    href: 'http://acde.local/x?y=1#z', hash: '#z', pathname: '/x', hidden: true,
    attrs: ['id', 'class', 'style', 'data-foo-bar', 'hidden', 'title', 'tabindex'], tabIndex: 2, aTab: 0, divTab: -1,
  })
})

test('tree operations: before/after/replaceWith, cloneNode, contains, compareDocumentPosition, normalize, live lists', async () => {
  using web = await page('<!doctype html><html><body><ol id="o"><li id="one">1</li><li id="two">2</li></ol></body></html>')
  const result = web.json(`(() => {
    const o = document.getElementById('o'), one = document.getElementById('one'), two = document.getElementById('two')
    const kids = o.children, nodes = o.childNodes
    const before = kids.length
    one.after('a', document.createElement('br'))
    two.before(document.createTextNode('b'))
    two.replaceWith(Object.assign(document.createElement('li'), { id: 'three' }))
    const ids = [...o.children].map((e) => e.id || e.localName)
    const live = [kids.length, nodes.length, kids[0] === one, nodes[1].nodeType]
    const copy = o.cloneNode(true)
    const deepCopy = copy.outerHTML === o.outerHTML && copy !== o && !copy.isConnected
    const pos = one.compareDocumentPosition(document.getElementById('three'))
    const p = document.createElement('p'); p.append('x', 'y', ''); p.normalize()
    return { before, ids, live, deepCopy, contains: o.contains(one) && !one.contains(o), pos, normalized: p.childNodes.length, text: p.textContent, root: one.getRootNode() === document, detachedRoot: p.firstChild.getRootNode() === p }
  })()`)
  assert.deepEqual(result, { before: 2, ids: ['one', 'br', 'three'], live: [3, 5, true, 3], deepCopy: true, contains: true, pos: 4, normalized: 1, text: 'xy', root: true, detachedRoot: true })
  const error = web.json(`(() => { try { document.body.appendChild(document.documentElement) } catch (e) { return [e instanceof DOMException, e.name] } })()`)
  assert.deepEqual(error, [true, 'HierarchyRequestError'])
  const syntax = web.json(`(() => { try { document.querySelector('a[') } catch (e) { return [e instanceof DOMException, e.name] } })()`)
  assert.deepEqual(syntax, [true, 'SyntaxError'])
})

test('DOMParser and createHTMLDocument give inert documents; XMLSerializer', async () => {
  using web = await page()
  const result = web.json(`(() => {
    const doc = new DOMParser().parseFromString('<p id="x">hi <img src=x onerror="window.pwned = 1"></p>', 'text/html')
    const p = doc.getElementById('x')
    const imp = document.implementation.createHTMLDocument('t')
    const it = document.createNodeIterator(doc.body, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT)
    const seen = []
    for (let n = it.nextNode(); n; n = it.nextNode()) seen.push(n.nodeName)
    const imported = document.importNode(p, true)
    document.body.appendChild(imported)
    return { text: p.textContent, inBody: p.parentNode === doc.body, title: imp.title, bodyKids: imp.body.childNodes.length, seen, imported: document.body.innerHTML, svg: new XMLSerializer().serializeToString(Object.assign(document.createElementNS('http://www.w3.org/2000/svg', 'svg'))), docType: doc.nodeType, connected: p.isConnected }
  })()`)
  assert.deepEqual(result, { text: 'hi ', inBody: true, title: 't', bodyKids: 0, seen: ['BODY', 'P', '#text', 'IMG'], imported: '<p id="x">hi <img src="x" onerror="window.pwned = 1"></p>', svg: '<svg xmlns="http://www.w3.org/2000/svg"/>', docType: 9, connected: false })
})
