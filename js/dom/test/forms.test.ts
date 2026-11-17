import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createWeb, page } from './harness.ts'

test('form controls: value / defaultValue, checked and radio groups, select, selection, validity, FormData, reset', async () => {
  using web = createWeb()
  await web.internals.loadPage(`<!doctype html><html><body><form id="f">
    <input id="t" name="t" value="start" maxlength="5" required>
    <input id="r1" type="radio" name="g" value="1" checked><input id="r2" type="radio" name="g" value="2">
    <input id="c" type="checkbox" name="c">
    <select id="s" name="s"><option value="a">A</option><option value="b" selected>B</option><optgroup><option>C</option></optgroup></select>
    <textarea id="ta" name="ta">hello</textarea>
    <input id="e" type="email" name="e" value="bad">
  </form></body></html>`)
  const result = web.json(`(() => {
    const $ = (id) => document.getElementById(id)
    const t = $('t'), r1 = $('r1'), r2 = $('r2'), s = $('s'), ta = $('ta'), f = $('f')
    const out = {}
    out.initial = [t.value, t.defaultValue, t.type, t.maxLength, r1.checked, s.value, s.selectedIndex, s.options.length, ta.value, ta.defaultValue]
    t.value = 'typed'
    out.dirty = [t.value, t.getAttribute('value'), t.selectionStart, t.selectionEnd]
    t.setSelectionRange(1, 3)
    out.selection = [t.selectionStart, t.selectionEnd, t.selectionDirection]
    r2.checked = true
    out.radios = [r1.checked, r2.checked]
    s.value = 'C'
    out.select = [s.selectedIndex, s.value, s.options[2].selected, s.selectedOptions.length]
    out.validity = [$('e').validity.typeMismatch, $('e').checkValidity(), f.checkValidity(), t.willValidate]
    $('e').value = 'a@b.c'
    out.data = [...new FormData(f)].map(([k, v]) => k + '=' + v)
    out.elements = [f.elements.length, f.elements.namedItem('g').value, f.elements.namedItem('t') === t]
    f.reset()
    out.reset = [t.value, r1.checked, s.value, ta.value]
    return out
  })()`)
  assert.deepEqual(result, {
    initial: ['start', 'start', 'text', 5, true, 'b', 1, 3, 'hello', 'hello'],
    dirty: ['typed', 'start', 5, 5],
    selection: [1, 3, 'none'],
    radios: [false, true],
    select: [2, 'C', true, 1],
    validity: [true, false, false, true],
    data: ['t=typed', 'g=2', 's=C', 'ta=hello', 'e=a@b.c'],
    elements: [7, '2', true],
    reset: ['start', true, 'b', 'hello'],
  })
  assert.deepEqual(web.dom.selectionRanges.get(web.internals.idOf(web.run('document.getElementById("t")'))), [1, 3])
})

test('focus: focus() / blur(), focus events with relatedTarget, activeElement, fixup on removal, tabIndex', async () => {
  using web = await page('<!doctype html><html><body><input id="a"><button id="b">b</button><div id="d">plain</div><div id="t" tabindex="0">t</div></body></html>')
  const result = web.json(`(() => {
    const log = []
    for (const type of ['focus', 'blur', 'focusin', 'focusout']) document.addEventListener(type, (e) => log.push(type + ':' + e.target.id + '<' + (e.relatedTarget && e.relatedTarget.id)), true)
    const $ = (id) => document.getElementById(id)
    $('a').focus()
    $('b').focus()
    $('d').focus()
    const notFocusable = document.activeElement.id
    $('t').focus()
    const tab = document.activeElement.id
    $('t').remove()
    const afterRemoval = document.activeElement === document.body
    $('b').focus(); $('b').blur()
    return { log, notFocusable, tab, afterRemoval, active: document.activeElement.nodeName, hasFocus: document.hasFocus() }
  })()`)
  assert.deepEqual(result, {
    log: ['focus:a<null', 'focusin:a<null', 'blur:a<b', 'focusout:a<b', 'focus:b<a', 'focusin:b<a', 'blur:b<t', 'focusout:b<t', 'focus:t<b', 'focusin:t<b', 'focus:b<null', 'focusin:b<null', 'blur:b<null', 'focusout:b<null'],
    notFocusable: 'b', tab: 't', afterRemoval: true, active: 'BODY', hasFocus: true,
  })
  assert.equal(web.dom.focus.id, -1)
})

test('Range and Selection: boundaries, toString, extract / delete / insert, live updates, selectionchange', async () => {
  using web = await page('<!doctype html><html><body><p id="p">Hello <b>big</b> world</p></body></html>')
  const result = web.json(`(() => {
    const p = document.getElementById('p')
    const hello = p.firstChild, world = p.lastChild
    const r = document.createRange()
    r.setStart(hello, 2)
    r.setEnd(world, 3)
    const text = r.toString()
    const common = r.commonAncestorContainer === p
    const cmp = r.comparePoint(p.querySelector('b'), 0)
    const copy = r.cloneContents()
    const cloned = [...copy.childNodes].map((n) => n.textContent)
    const sel = getSelection()
    sel.removeAllRanges()
    sel.addRange(r)
    const selInfo = [sel.rangeCount, sel.type, sel.anchorNode === hello, sel.focusOffset, sel.toString()]
    p.querySelector('b').remove()
    const afterRemove = [r.startContainer === hello, r.endContainer === world, r.toString()]
    r.deleteContents()
    const afterDelete = [p.textContent, r.collapsed, r.startContainer === hello, r.startOffset]
    r.insertNode(document.createElement('i'))
    const afterInsert = p.innerHTML
    sel.collapse(p, 0)
    sel.extend(p, 1)
    return { text, common, cmp, cloned, selInfo, afterRemove, afterDelete, afterInsert, extended: [sel.anchorOffset, sel.focusOffset, sel.direction] }
  })()`)
  assert.deepEqual(result, {
    text: 'llo big wo', common: true, cmp: 0, cloned: ['llo ', 'big', ' wo'],
    selInfo: [1, 'Range', true, 3, 'llo big wo'],
    afterRemove: [true, true, 'llo  wo'],
    afterDelete: ['Herld', true, false, 1],
    afterInsert: 'He<i></i>rld',
    extended: [0, 1, 'forward'],
  })
  assert.equal(web.dom.selection[1], 0)
  assert.equal(web.dom.selection[3], 1)
  web.run('globalThis.changes = 0; document.addEventListener("selectionchange", () => changes++)')
  web.run('getSelection().collapse(document.body, 0)')
  web.run('getSelection().collapse(document.body, 1)')
  await web.wait(5)
  assert.equal(web.run('changes'), 1, 'one selectionchange per turn')
})

test('editing: keys and execCommand share one path; beforeinput cancels typing, not execCommand', async () => {
  using web = createWeb()
  await web.internals.loadPage(`<!doctype html><html><body>
    <input id="t" value="abc"><input id="m" type="email" value="a@b"><textarea id="ta">x</textarea>
    <div id="ed" contenteditable="true">hello</div>
  </body></html>`)
  web.run(`globalThis.__log = []
    for (const id of ['t', 'm', 'ta', 'ed']) for (const type of ['beforeinput', 'input'])
      document.getElementById(id).addEventListener(type, (e) => __log.push([id, type, e.inputType, e.data, e.isTrusted]))`)
  const key = (id: string, k: string, ctrl = false) => web.internals.editing.editKey(web.run(`document.getElementById('${id}')`), k, ctrl)
  key('t', 'd')
  key('t', 'ArrowLeft')
  key('t', 'ArrowLeft')
  key('t', 'Backspace')
  key('t', 'Delete')
  key('m', 'x')
  key('ta', 'Enter')
  key('ta', 'y')
  const keyed = web.json(`(() => {
    const $ = (id) => document.getElementById(id)
    const out = [$('t').value, $('t').selectionStart, $('m').value, $('ta').value, __log.splice(0)]
    $('t').addEventListener('beforeinput', (e) => e.preventDefault(), { once: true })
    return out
  })()`)
  key('t', 'z')
  const cancelled = web.json(`[document.getElementById('t').value, __log.splice(0)]`)
  const commands = web.json(`(() => {
    const $ = (id) => document.getElementById(id)
    const out = []
    $('t').focus()
    $('t').setSelectionRange(1, 2)
    out.push(document.execCommand('insertText', false, 'Q'), $('t').value, $('t').selectionStart)
    out.push(document.execCommand('delete'), $('t').value)
    out.push(document.execCommand('insertParagraph'), $('t').value)
    $('ed').focus()
    getSelection().collapse($('ed').firstChild, 5)
    out.push(document.execCommand('insertText', false, '!'), $('ed').textContent)
    out.push(document.execCommand('delete'), $('ed').textContent)
    out.push(document.execCommand('insertHTML', false, '<b>B</b>'), $('ed').innerHTML)
    out.push(document.execCommand('selectAll'), getSelection().toString())
    out.push(document.execCommand('forwardDelete'), $('ed').textContent)
    out.push(document.execCommand('bold'))
    out.push(__log.splice(0))
    return out
  })()`)
  assert.deepEqual(keyed, ['ad', 1, 'a@bx', 'x\ny', [
    ['t', 'beforeinput', 'insertText', 'd', true], ['t', 'input', 'insertText', 'd', true],
    ['t', 'beforeinput', 'deleteContentBackward', null, true], ['t', 'input', 'deleteContentBackward', null, true],
    ['t', 'beforeinput', 'deleteContentForward', null, true], ['t', 'input', 'deleteContentForward', null, true],
    ['m', 'beforeinput', 'insertText', 'x', true], ['m', 'input', 'insertText', 'x', true],
    ['ta', 'beforeinput', 'insertLineBreak', null, true], ['ta', 'input', 'insertLineBreak', null, true],
    ['ta', 'beforeinput', 'insertText', 'y', true], ['ta', 'input', 'insertText', 'y', true],
  ]])
  assert.deepEqual(cancelled, ['ad', [['t', 'beforeinput', 'insertText', 'z', true]]])
  assert.deepEqual(commands, [
    true, 'aQ', 2,
    true, 'a',
    false, 'a',
    true, 'hello!',
    true, 'hello',
    true, 'hello<b>B</b>',
    true, 'helloB',
    true, '',
    false,
    [
      ['t', 'input', 'insertText', 'Q', true],
      ['t', 'input', 'deleteContentBackward', null, true],
      ['ed', 'input', 'insertText', '!', true],
      ['ed', 'input', 'deleteContentBackward', null, true],
      ['ed', 'input', 'insertHTML', '<b>B</b>', true],
      ['ed', 'input', 'deleteContentForward', null, true],
    ],
  ])
})

test('select() on number and email inputs selects the value, so typing replaces it', async () => {
  using web = createWeb()
  await web.internals.loadPage(`<!doctype html><html><body>
    <input id="n" type="number" value="25"><input id="m" type="email" value="a@b">
  </body></html>`)
  const key = (id: string, k: string) => web.internals.editing.editKey(web.run(`document.getElementById('${id}')`), k, false)
  web.run(`document.getElementById('n').select(); document.getElementById('m').select()`)
  key('n', '1')
  key('m', 'x')
  assert.deepEqual(web.json(`[document.getElementById('n').value, document.getElementById('m').value, document.getElementById('n').selectionStart]`), ['1', 'x', null])
})

test('editing history: typing runs, deletes and pastes undo and redo with their selection; script values and cancelled beforeinput', async () => {
  using web = createWeb()
  await web.internals.loadPage(`<!doctype html><html><body>
    <input id="t"><textarea id="ta"></textarea><div id="ed" contenteditable="true">hello</div>
  </body></html>`)
  web.run(`globalThis.__log = []
    for (const id of ['t', 'ta', 'ed']) for (const type of ['beforeinput', 'input'])
      document.getElementById(id).addEventListener(type, (e) => e.inputType.startsWith('history') && __log.push([id, type, e.inputType, e.data, e.cancelable, e.isTrusted]))`)
  const el = (id: string) => web.run<never>(`document.getElementById('${id}')`)
  const key = (id: string, k: string, ctrl = false, shift = false) => web.internals.editing.editKey(el(id), k, ctrl, shift)
  const type = (id: string, text: string) => { for (const c of text) key(id, c) }
  const state = (id: string) => web.json(`(() => { const e = document.getElementById('${id}'); return [e.value, e.selectionStart, e.selectionEnd] })()`)
  const log = () => web.json<unknown[][]>('__log.splice(0)')

  type('t', 'hello')
  key('t', 'z', true)
  assert.deepEqual(state('t'), ['', 0, 0], 'one typing run is one step')
  assert.deepEqual(log(), [
    ['t', 'beforeinput', 'historyUndo', null, true, true], ['t', 'input', 'historyUndo', null, false, true],
  ])
  key('t', 'Z', true, true)
  assert.deepEqual(state('t'), ['hello', 5, 5])
  key('t', 'z', true)
  key('t', 'y', true)
  assert.deepEqual(state('t'), ['hello', 5, 5])
  assert.equal(log().length, 6)
  key('t', 'y', true)
  assert.deepEqual([state('t'), log()], [['hello', 5, 5], []], 'nothing to redo fires nothing')

  key('t', 'ArrowLeft')
  key('t', 'ArrowLeft')
  type('t', 'XY')
  assert.deepEqual(state('t'), ['helXYlo', 5, 5])
  key('t', 'Backspace')
  key('t', 'Backspace')
  key('t', 'Backspace')
  assert.deepEqual(state('t'), ['helo', 2, 2])
  key('t', 'z', true)
  assert.deepEqual(state('t'), ['helXYlo', 5, 5], 'a backspace run is one step')
  key('t', 'z', true)
  assert.deepEqual(state('t'), ['hello', 3, 3], 'moving the caret starts a new step')
  web.run(`document.getElementById('t').setSelectionRange(1, 4)`)
  web.internals.editing.insertText(el('t'), 'PP', 'insertFromPaste')
  assert.deepEqual(state('t'), ['hPPo', 3, 3])
  type('t', 'q')
  key('t', 'z', true)
  assert.deepEqual(state('t'), ['hPPo', 3, 3], 'a paste does not join the typing after it')
  key('t', 'z', true)
  assert.deepEqual(state('t'), ['hello', 1, 4], 'undo selects what the paste replaced')
  type('t', 'n')
  key('t', 'y', true)
  assert.deepEqual(state('t'), ['hno', 2, 2], 'a new edit drops the redo steps')
  log()

  web.run(`document.getElementById('t').value = document.getElementById('t').value`)
  key('t', 'z', true)
  assert.deepEqual(state('t'), ['hello', 1, 4], 'setting the same value keeps the history')
  web.run(`document.getElementById('t').value = 'script'`)
  key('t', 'z', true)
  assert.deepEqual(state('t'), ['script', 6, 6], 'a script value drops the history')
  assert.deepEqual(log().map((e) => e[2]), ['historyUndo', 'historyUndo'])

  type('ta', 'a')
  key('ta', 'Enter')
  type('ta', 'b')
  web.run(`document.getElementById('ta').addEventListener('beforeinput', (e) => e.preventDefault(), { once: true })`)
  key('ta', 'z', true)
  assert.deepEqual(state('ta'), ['a\nb', 3, 3], 'a cancelled beforeinput keeps the value')
  key('ta', 'z', true)
  assert.deepEqual(state('ta'), ['', 0, 0], 'line breaks join the typing run')
  assert.deepEqual(log(), [
    ['ta', 'beforeinput', 'historyUndo', null, true, true],
    ['ta', 'beforeinput', 'historyUndo', null, true, true], ['ta', 'input', 'historyUndo', null, false, true],
  ])

  const commands = web.json(`(() => {
    const ta = document.getElementById('ta')
    ta.focus()
    const out = [document.queryCommandSupported('undo'), document.queryCommandSupported('redo'), document.queryCommandEnabled('undo'), document.queryCommandEnabled('redo')]
    out.push(document.execCommand('insertText', false, 'cmd'), ta.value, document.queryCommandEnabled('undo'))
    out.push(document.execCommand('undo'), ta.value, document.queryCommandEnabled('redo'))
    out.push(document.execCommand('redo'), ta.value, document.execCommand('redo'))
    return out
  })()`)
  assert.deepEqual(commands, [true, true, false, true, true, 'cmd', true, true, '', true, true, 'cmd', false])
  assert.deepEqual(log(), [['ta', 'input', 'historyUndo', null, false, true], ['ta', 'input', 'historyRedo', null, false, true]])

  web.run(`getSelection().collapse(document.getElementById('ed').firstChild, 5)`)
  key('ed', '!')
  key('ed', 'z', true)
  assert.equal(web.run(`document.getElementById('ed').textContent`), 'hello!')
  assert.deepEqual(log(), [])
})
