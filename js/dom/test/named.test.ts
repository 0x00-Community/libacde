import assert from 'node:assert/strict'
import { test } from 'node:test'
import { callsOf, loadReact, page } from './harness.ts'

const FORM = `<!doctype html><html><body>
  <form id="f" name="login">
    <input name="title" value="t"><input id="byid" value="i">
    <input type="radio" name="r" value="a"><input type="radio" name="r" value="b" checked>
    <input type="image" name="im" id="imid">
    <img name="pic"><img id="pic2">
    <select name="sel"><option id="o1" name="on">1</option><option>2</option></select>
    <button name="submit">go</button><input name="action" value="clobber"><input name="length">
  </form>
  <input id="outside" name="out" form="f">
  <form name="second"><input name="title"></form>
  <svg><a id="svgid" name="svgname"></a></svg>
</body></html>`

test('HTMLCollection: named properties by id and by name, builtins first, not enumerable', async () => {
  using web = await page(FORM)
  const result = web.json(`(() => {
    const inputs = document.getElementsByTagName('input')
    const all = document.getElementsByTagName('*')
    const out = {}
    out.byName = [inputs.title === inputs[0], inputs.byid.id, inputs.out.id, inputs.nothing === undefined]
    out.builtins = [typeof inputs.length, typeof inputs.item, typeof inputs.namedItem]
    out.has = ['title' in inputs, 'byid' in inputs, 'nothing' in inputs, '0' in inputs, '99' in inputs, 'length' in inputs]
    out.keys = Object.keys(inputs).join()
    out.names = Object.getOwnPropertyNames(inputs).filter((k) => !/^\\d+$/.test(k)).join()
    out.descriptor = Object.getOwnPropertyDescriptor(inputs, 'title')
    out.descriptor.value = out.descriptor.value === inputs[0]
    out.index = Object.getOwnPropertyDescriptor(inputs, '0').enumerable
    out.forms = [document.forms.login === document.forms[0], document.forms.f === document.forms[0], document.forms.second === document.forms[1], document.forms.length]
    out.children = document.body.children.f === document.forms[0]
    out.svg = [all.svgid.localName, all.svgname === undefined, all.namedItem('svgname')]
    out.first = [all.namedItem('title') === inputs[0], all.namedItem('f') === document.forms[0], all.namedItem('')]
    const table = document.createElement('table')
    table.innerHTML = '<thead><tr id="head"><th>h</th></tr></thead><tbody><tr name="first"><td id="cell">c</td></tr></tbody>'
    out.table = [table.rows.constructor.name, table.rows === table.rows, table.rows.head.id, table.rows.first.rowIndex, table.rows.length, table.tBodies[0].rows.first === table.rows[1], table.rows[1].cells.cell.cellIndex]
    table.insertRow()
    out.live = [table.rows.length, table.tBodies[0].rows.length]
    out.options = [document.forms[0].elements.sel.options.on.id, document.forms[0].elements.sel.options.o1.id, document.forms[0].elements.sel.options[1].text]
    inputs.expando = 1
    out.expando = [inputs.expando, Object.keys(inputs).includes('expando')]
    const strict = (fn) => { try { fn(); return 'ok' } catch (e) { return e.constructor.name } }
    out.assign = [
      strict(function () { 'use strict'; inputs.title = 1 }), inputs.title === inputs[0],
      strict(function () { 'use strict'; inputs[0] = 1 }), inputs[0].name,
      strict(function () { 'use strict'; delete inputs.title }), strict(function () { 'use strict'; delete inputs[0] }), strict(function () { 'use strict'; delete inputs[99] }),
      strict(function () { Object.preventExtensions(inputs) }),
    ]
    return out
  })()`)
  assert.deepEqual(result, {
    byName: [true, 'byid', 'outside', true],
    builtins: ['number', 'function', 'function'],
    has: [true, true, false, true, false, true],
    keys: '0,1,2,3,4,5,6,7,8',
    names: 'title,byid,r,imid,im,action,outside,out',
    descriptor: { value: true, writable: false, enumerable: false, configurable: true },
    index: true,
    forms: [true, true, true, 2],
    table: ['HTMLCollection', true, 'head', 1, 2, true, 0],
    live: [3, 2],
    children: true,
    svg: ['a', true, null],
    first: [true, true, null],
    options: ['o1', 'o1', '2'],
    expando: [1, true],
    assign: ['TypeError', true, 'TypeError', 'title', 'TypeError', 'TypeError', 'ok', 'TypeError'],
  })
})

test('HTMLFormControlsCollection: an element, or a live RadioNodeList when several share the name', async () => {
  using web = await page(FORM)
  const result = web.json(`(() => {
    const f = document.getElementById('f')
    const e = f.elements
    const out = {}
    out.same = [f.elements === e, e.constructor.name, e.length, [...e].map((n) => n.name || n.id).join()]
    out.named = [e.title === e[0], e.byid === e[1], e.out.id, e.im === undefined, e.pic === undefined, e.namedItem('title') === e[0], e.namedItem('nope'), e.namedItem('')]
    const r = e.r
    out.radio = [String(r), r instanceof NodeList, r instanceof RadioNodeList, r.length, r[0].value, r.item(1).value, r.value, [...r].length, e.namedItem('r').length]
    r.value = 'a'
    out.set = [r[0].checked, r[1].checked, r.value]
    const more = document.createElement('input'); more.type = 'radio'; more.name = 'r'
    f.append(more)
    out.live = [r.length, r[2] === more]
    more.checked = true
    out.on = r.value
    r.value = 'b'
    out.back = [r.value, more.checked]
    more.remove(); r[0].remove()
    out.one = [r.length, e.r === r[0], e.r.value]
    out.fieldset = (() => { const s = document.createElement('fieldset'); s.innerHTML = '<input name=q><input type=image name=i>'; return [s.elements.constructor.name, s.elements === s.elements, s.elements.length, s.elements.q.name, s.elements.i.name] })()
    return out
  })()`)
  assert.deepEqual(result, {
    same: [true, 'HTMLFormControlsCollection', 9, 'title,byid,r,r,sel,submit,action,length,out'],
    named: [true, true, 'outside', true, true, true, null, null],
    radio: ['[object RadioNodeList]', true, true, 2, 'a', 'b', 'b', 2, 2],
    set: [true, false, 'a'],
    live: [3, true],
    on: 'on',
    back: ['b', false],
    one: [1, true, 'b'],
    fieldset: ['HTMLCollection', true, 2, 'q', 'i'],
  })
})

test('HTMLFormElement: named and indexed properties override builtins, past names, images', async () => {
  using web = await page(FORM)
  const result = web.json(`(() => {
    const f = document.getElementById('f')
    const title = f.elements[0]
    const out = {}
    out.named = [f.title === title, f.byid.id, f.out.id, f.sel.localName, f.im === undefined, f.imid === undefined, f.nothing === undefined]
    out.radio = [String(f.r), f.r.length, f.r.value, f.r === f.r]
    out.image = [f.pic.localName, f.pic2.localName, f.elements.pic === undefined]
    out.indexed = [f[0] === title, f[8].id, f[9] === undefined, f.length.localName, f.elements.length]
    out.override = [f.submit.localName, f.action.value, typeof f.reset, typeof f.addEventListener, f.id, f.name]
    out.has = ['title' in f, 'pic' in f, 'nothing' in f, '0' in f, '9' in f, 'reset' in f]
    out.keys = Object.keys(f).join()
    out.names = Object.getOwnPropertyNames(f).filter((k) => !/^\\d+$/.test(k)).join()
    out.descriptor = Object.getOwnPropertyDescriptor(f, 'title')
    out.descriptor.value = out.descriptor.value === title
    out.extras = [f.item, f.namedItem].map((v) => typeof v)
    f.action = '/post'
    f.title = 'ignored'
    out.set = [f.getAttribute('action'), f.action.value, f.getAttribute('title'), f.title === title]
    const strict = (fn) => { try { fn(); return 'ok' } catch (e) { return e.constructor.name } }
    out.strict = [strict(function () { 'use strict'; f.byid = 'x' }), f.byid.id, strict(function () { 'use strict'; f[0] = 1 }), strict(function () { 'use strict'; delete f.title }), delete f.nothing]
    f.mine = 1
    const mine = document.createElement('input'); mine.name = 'mine'
    f.append(mine)
    out.expando = [f.mine, f.elements.mine === mine]
    mine.remove()
    title.name = 'renamed'
    out.past = [f.title === title, f.renamed === title, 'title' in f, Object.getOwnPropertyNames(f).includes('title')]
    const again = document.createElement('input'); again.name = 'title'
    f.append(again)
    out.replaced = [f.title === again, f.renamed === title]
    again.remove()
    out.forgotten = [f.title, 'title' in f]
    title.remove()
    out.gone = [f.renamed, 'renamed' in f]
    document.forms.second.append(title)
    out.moved = [f.renamed, document.forms.second.renamed === title, String(document.forms.second.title)]
    const img = document.createElement('img'); img.name = 'late'
    f.append(img)
    out.lateImage = f.late === img
    out.instance = [f instanceof HTMLFormElement, f instanceof Node, Object.prototype.toString.call(f), document.getElementById('f') === f, f === document.forms.login, title.form === document.forms.second, f.elements[0].form === f]
    let got = null
    f.addEventListener('custom', (e) => { got = [e.target === f, e.currentTarget === f, this === undefined] })
    f.dispatchEvent(new Event('custom'))
    out.event = got
    return out
  })()`)
  assert.deepEqual(result, {
    named: [true, 'byid', 'outside', 'select', true, true, true],
    radio: ['[object RadioNodeList]', 2, 'b', true],
    image: ['img', 'img', true],
    indexed: [true, 'outside', true, 'input', 9],
    override: ['button', 'clobber', 'function', 'function', 'f', 'login'],
    has: [true, true, false, true, false, true],
    keys: '0,1,2,3,4,5,6,7,8',
    names: 'title,byid,r,pic,pic2,sel,submit,action,length,outside,out',
    descriptor: { value: true, writable: false, enumerable: false, configurable: true },
    extras: ['undefined', 'undefined'],
    set: ['/post', 'clobber', 'ignored', true],
    strict: ['TypeError', 'byid', 'TypeError', 'TypeError', true],
    expando: [1, true],
    past: [true, true, true, true],
    replaced: [true, true],
    forgotten: ['ignored', true],
    gone: [null, false],
    moved: [null, true, '[object HTMLInputElement]'],
    lateImage: true,
    instance: [true, true, '[object HTMLFormElement]', true, true, true, true],
    event: [true, true, false],
  })
})

test('HTMLSelectElement and its options: indexed getters and setters, length', async () => {
  using web = await page('<!doctype html><html><body><select id="s"><option id="a">A</option><optgroup><option name="b">B</option></optgroup></select></body></html>')
  const result = web.json(`(() => {
    const s = document.getElementById('s')
    const out = {}
    out.get = [s[0].id, s[1].text, s[2] === undefined, s.item(1).text, s.namedItem('b').text, s.options.b.text, s.options.a === s[0], '1' in s, '2' in s, Object.keys(s).join()]
    s[2] = new Option('C', 'c')
    s.options[4] = new Option('E')
    out.grown = [s.length, [...s.options].map((o) => o.text).join('|'), s[3].parentNode === s]
    s[0] = new Option('A2')
    out.replaced = [s.length, s[0].text, document.getElementById('a')]
    s.options[1] = null
    out.removed = [s.length, [...s.options].map((o) => o.text).join('|')]
    s.length = 2
    out.cut = [s.options.length, s[1].text]
    s.options.length = 4
    out.padded = [s.length, s[3].localName, s[3].text]
    s.length = 0
    out.empty = [s.length, s.options.length]
    const strict = (fn) => { try { fn(); return 'ok' } catch (e) { return e.constructor.name } }
    out.wrong = [strict(() => { s[0] = document.createElement('div') }), s.length]
    out.same = [document.getElementById('s') === s, s instanceof HTMLSelectElement, s.options === s.options]
    return out
  })()`)
  assert.deepEqual(result, {
    get: ['a', 'B', true, 'B', 'B', 'B', true, true, false, '0,1'],
    grown: [5, 'A|B|C||E', true],
    replaced: [5, 'A2', null],
    removed: [4, 'A2|C||E'],
    cut: [2, 'C'],
    padded: [4, 'option', ''],
    empty: [0, 0],
    wrong: ['TypeError', 0],
    same: [true, true, true],
  })
})

const INSIDE = `(() => {
  const names = []
  for (let p = HTMLFormElement.prototype; p; p = Object.getPrototypeOf(p)) for (const k of Object.getOwnPropertyNames(p)) if (!names.includes(k)) names.push(k)
  const call = (target, name, ...args) => {
    for (let p = Object.getPrototypeOf(target); p; p = Object.getPrototypeOf(p)) {
      const d = Object.getOwnPropertyDescriptor(p, name)
      if (d) return typeof d.value === 'function' ? d.value.apply(target, args) : d.get.call(target)
    }
    throw new Error('no ' + name)
  }
  const put = (target, name, value) => {
    for (let p = Object.getPrototypeOf(target); p; p = Object.getPrototypeOf(p)) {
      const d = Object.getOwnPropertyDescriptor(p, name)
      if (d) return d.set.call(target, value)
    }
  }
  const exercise = (prefix) => {
    const out = {}
    const f = document.createElement('form')
    call(f, 'setAttribute', 'id', prefix + 'form')
    call(f, 'setAttribute', 'method', 'dialog')
    for (const name of names) {
      const input = document.createElement('input')
      input.setAttribute('name', prefix + name)
      input.setAttribute('value', 'v')
      call(f, 'appendChild', input)
    }
    const seen = []
    out.records = seen
    new MutationObserver((records) => seen.push(...records.map((r) => [r.type, r.target === f, r.attributeName]))).observe(f, { attributes: true, childList: true })
    const dialog = document.createElement('dialog')
    document.body.appendChild(dialog)
    dialog.appendChild(f)
    dialog.setAttribute('open', '')
    out.tree = [call(f, 'parentNode') === dialog, call(f, 'isConnected'), document.getElementById(prefix + 'form') === f, document.forms[prefix + 'form'] === f, dialog.contains(f), dialog.firstChild === f,
      document.querySelector('#' + prefix + 'form') === f, call(f, 'ownerDocument') === document, call(f, 'getRootNode') === document, call(f, 'nodeName'), call(f, 'previousSibling'), call(f, 'parentElement') === dialog,
      call(f, 'compareDocumentPosition', dialog), call(f, 'contains', call(f, 'firstChild')), call(f, 'hasChildNodes'), call(f, 'isEqualNode', f), call(f, 'isSameNode', f)]
    const events = []
    out.events = events
    call(f, 'addEventListener', 'submit', (e) => { events.push(['submit', e.target === f, e.currentTarget === f, e.submitter ? e.submitter.localName : null]); if (events.length < 3) e.preventDefault() })
    call(f, 'addEventListener', 'reset', (e) => events.push(['reset', e.target === f]))
    call(f, 'addEventListener', 'toggle', () => events.push(['toggle']))
    put(f, 'onclick', (e) => events.push(['onclick', e.target === f]))
    document.addEventListener('click', (e) => events.push(['click', e.target === f, e.composedPath().indexOf(f), e.composedPath().indexOf(dialog)]), { once: true })
    call(f, 'click')
    const button = document.createElement('button')
    call(f, 'appendChild', button)
    const reset = document.createElement('input')
    reset.type = 'reset'
    call(f, 'appendChild', reset)
    button.click()
    call(f, 'requestSubmit')
    const first = call(f, 'firstChild')
    first.value = 'changed'
    reset.click()
    out.reset = first.value
    first.value = 'changed'
    call(f, 'reset')
    out.resetAgain = first.value
    first.required = true
    first.setAttribute('value', '')
    call(f, 'reset')
    out.valid = [call(f, 'checkValidity'), call(f, 'reportValidity')]
    call(f, 'requestSubmit', button)
    first.required = false
    out.open = [dialog.open]
    call(f, 'requestSubmit', button)
    out.open.push(dialog.open)
    out.data = [...new FormData(f)].length
    out.read = [call(f, 'length'), call(f, 'elements').length, call(f, 'method'), call(f, 'id').slice(prefix.length), call(f, 'children').length, call(f, 'childElementCount'), call(f, 'childNodes').length, call(f, 'action'), call(f, 'tagName'),
      call(f, 'localName'), call(f, 'namespaceURI'), call(f, 'nodeType'), call(f, 'noValidate'), call(f, 'hidden'), call(f, 'tabIndex'), call(f, 'isContentEditable'), call(f, 'offsetParent') === null, typeof call(f, 'offsetWidth'),
      call(f, 'getBoundingClientRect').width >= 0, call(f, 'firstElementChild') === first, call(f, 'lastElementChild') === reset, call(f, 'lastChild') === reset, call(f, 'nextSibling'), call(f, 'nextElementSibling')]
    call(f, 'setAttribute', 'data-x', '1')
    call(f, 'toggleAttribute', 'novalidate')
    call(f, 'classList').add('k')
    call(f, 'style').color = 'red'
    call(f, 'dataset').y = '2'
    put(f, 'title', 'T')
    put(f, 'className', 'k l')
    put(f, 'hidden', true)
    put(f, 'hidden', false)
    put(f, 'lang', 'en')
    put(f, 'tabIndex', 3)
    put(f, 'action', '/go')
    put(f, 'enctype', 'text/plain')
    put(f, 'noValidate', false)
    out.attributes = [call(f, 'getAttribute', 'class'), call(f, 'getAttribute', 'style'), call(f, 'getAttribute', 'data-y'), call(f, 'attributes').length, call(f, 'getAttributeNames').join(), call(f, 'hasAttribute', 'title'),
      call(f, 'getAttributeNode', 'lang').value, call(f, 'relList').length, call(f, 'title'), call(f, 'encoding')]
    out.text = [call(f, 'outerHTML').slice(0, 9), call(f, 'innerHTML').split('<input').length, call(f, 'textContent'), call(f, 'innerText'), call(f, 'matches', 'dialog > form'), call(f, 'closest', 'dialog') === dialog, call(f, 'querySelector', 'button') === button,
      call(f, 'querySelectorAll', 'input').length, call(f, 'getElementsByTagName', 'input').length, call(f, 'getElementsByClassName', 'none').length]
    const clone = call(f, 'cloneNode', true)
    out.clone = [clone !== f, clone instanceof HTMLFormElement, call(clone, 'childNodes').length, call(clone, 'isEqualNode', f), call(clone, 'parentNode')]
    call(f, 'insertAdjacentHTML', 'beforeend', '<input name="late">')
    call(f, 'insertAdjacentElement', 'afterbegin', document.createElement('output'))
    call(f, 'insertAdjacentText', 'beforeend', 'text')
    call(f, 'append', document.createElement('textarea'), 'more')
    call(f, 'prepend', document.createElement('select'))
    call(f, 'insertBefore', document.createElement('fieldset'), first)
    call(f, 'removeChild', call(f, 'firstChild'))
    call(f, 'replaceChild', document.createElement('object'), call(f, 'firstChild'))
    call(f, 'normalize')
    out.late = [f.late.localName, call(f, 'length'), call(f, 'childNodes').length]
    call(f, 'before', document.createElement('p'))
    call(f, 'after', document.createElement('p'))
    out.siblings = [call(f, 'previousElementSibling').localName, call(f, 'nextElementSibling').localName, dialog.childNodes.length]
    put(f, 'tabIndex', 0)
    call(f, 'focus')
    out.focus = document.activeElement === f
    call(f, 'blur')
    call(f, 'scrollIntoView')
    call(f, 'scrollTo', 0, 0)
    out.animation = call(f, 'getAnimations').length
    const range = document.createRange()
    range.selectNodeContents(f)
    out.range = [range.startContainer === f, range.endOffset === call(f, 'childNodes').length, range.commonAncestorContainer === f]
    getSelection().selectAllChildren(f)
    out.selection = getSelection().anchorNode === f
    getSelection().removeAllRanges()
    const walker = document.createTreeWalker(dialog, NodeFilter.SHOW_ELEMENT)
    out.walker = [walker.nextNode() && walker.currentNode.localName, walker.nextNode() === f, walker.parentNode() === dialog]
    call(f, 'replaceChildren', button)
    out.replaced = [call(f, 'childNodes').length, call(f, 'firstChild') === button, call(f, 'length')]
    put(f, 'innerHTML', '<input name="a"><b>x</b>')
    put(f, 'textContent', 'plain')
    out.content = [call(f, 'innerHTML'), call(f, 'childNodes').length]
    const moved = document.createElement('div')
    document.body.appendChild(moved)
    moved.appendChild(f)
    out.moved = [call(f, 'parentNode') === moved, dialog.contains(f), moved.children[0] === f, moved.children[prefix + 'form'] === f]
    put(f, 'outerHTML', '<i></i>')
    out.outer = [call(f, 'isConnected'), call(f, 'parentNode'), moved.firstChild.localName]
    moved.appendChild(f)
    call(f, 'replaceWith', document.createElement('u'))
    out.replacedWith = [call(f, 'isConnected'), moved.lastChild.localName]
    moved.appendChild(f)
    call(f, 'remove')
    out.removed = [call(f, 'isConnected'), call(f, 'parentNode'), moved.childNodes.length]
    return out
  }
  globalThis.inside = { count: names.length, clobbered: exercise(''), plain: exercise('zz_') }
})()`

test('a form whose controls are named after every member of its interfaces works from the inside as any other', async () => {
  using web = await page()
  web.run(INSIDE)
  await web.wait(5)
  const inside = web.json<{ count: number; clobbered: Record<string, unknown>; plain: Record<string, unknown> }>('inside')
  assert.ok(inside.count > 250, `${inside.count} names`)
  assert.deepEqual(web.host.errors, [])
  assert.deepEqual(inside.clobbered, inside.plain)
  assert.deepEqual((inside.plain['events'] as unknown[][]).map((e) => e[0]), ['onclick', 'click', 'onclick', 'submit', 'submit', 'onclick', 'reset', 'reset', 'reset', 'submit'])
  assert.deepEqual([inside.plain['valid'], inside.plain['open']], [[false, false], [false, false]])
  assert.deepEqual(inside.plain['outer'], [false, null, 'i'])
})

test('window: elements by id and the listed elements by name, real globals first, following the DOM', async () => {
  using web = await page(`<!doctype html><html><body>
    <div id="box"></div><span id="taken"></span><div id="Array"></div><img name="logo"><input name="field"><div id="later"></div>
  </body></html>`)
  web.run('let taken = 1')
  const result = web.json(`(() => {
    const out = {}
    out.byId = [box === document.getElementById('box'), window.box === box, 'box' in window, typeof nothingHere, window.nothingHere]
    out.byName = [window.logo && window.logo.localName, window.field]
    out.globals = [taken, window.taken === document.getElementById('taken'), Array === globalThis.Array, typeof Array.isArray]
    out.hidden = [Object.keys(window).includes('box'), Object.getOwnPropertyDescriptor(window, 'box') === undefined]
    document.getElementById('box').remove()
    out.removed = [typeof box, window.box]
    const later = document.getElementById('later')
    later.id = 'renamed'
    out.renamed = [window.later, renamed === later]
    later.id = 'later'
    out.back = window.later === later
    window.box = 'mine'
    out.assigned = window.box
    return out
  })()`)
  assert.deepEqual(result, {
    byId: [true, true, true, 'undefined', null],
    byName: ['img', null],
    globals: [1, true, true, 'function'],
    hidden: [false, true],
    removed: ['undefined', null],
    renamed: [null, true],
    back: true,
    assigned: 'mine',
  })
})

test('window: a name no element has is answered from a cached name set, rebuilt after a DOM change', async () => {
  const items = Array.from({ length: 5000 }, (_, i) => `<div id="item${i}"><span name="n${i}"></span></div>`).join('')
  using web = await page(`<!doctype html><html><body>${items}<img name="logo"></body></html>`)
  const result = web.json<{ first: string; found: unknown[]; after: boolean; perMiss: number }>(`(() => {
    const first = typeof missingName
    const start = performance.now()
    for (let i = 0; i < 1000; i++) if (typeof missingName !== 'undefined') throw new Error('found')
    const missing = performance.now() - start
    const found = [typeof item4999, window.logo && window.logo.localName, typeof n1]
    const div = document.createElement('div')
    div.id = 'missingName'
    document.body.append(div)
    const after = window.missingName === div
    return { first, found, after, perMiss: missing / 1000 }
  })()`)
  assert.deepEqual([result.first, result.found, result.after], ['undefined', ['object', 'img', 'undefined'], true])
  assert.ok(result.perMiss < 0.05, `a missing name took ${result.perMiss} ms`)
})

