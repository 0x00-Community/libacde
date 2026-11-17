import assert from 'node:assert/strict'
import { test } from 'node:test'
import { page } from './harness.ts'

const LINK = `
  globalThis.log = []
  globalThis.link = document.createElement('link')
  link.rel = 'stylesheet'
  link.addEventListener('load', () => log.push('load'))
  link.addEventListener('error', () => log.push('error'))
`

test('a link fires load once its stylesheet is fetched and applied; link.sheet and document.styleSheets have it', async () => {
  using web = await page('<!doctype html><html><head><style>p {}</style></head><body></body></html>')
  web.run(`${LINK}; link.href = '/site.css'; document.head.append(link)`)
  const id = web.internals.idOf(web.run('link'))
  web.dom._sheet(id, 0)
  await web.wait(5)
  assert.deepEqual(web.json('[log, link.sheet, document.styleSheets.length]'), [[], null, 1], 'nothing while it is being fetched')
  web.dom._sheet(id, 1)
  web.frame()
  const loaded = web.json<unknown[]>(`[log, link.sheet instanceof CSSStyleSheet, link.sheet === link.sheet, link.sheet.ownerNode === link, link.sheet.href,
    document.styleSheets.length, document.styleSheets[1] === link.sheet, link.sheet.cssRules.length, link.sheet.media.mediaText]`)
  assert.deepEqual(loaded, [['load'], true, true, true, 'http://acde.local/site.css', 2, true, 0, ''])
  web.frame()
  assert.deepEqual(web.json('log'), ['load'], 'once')
})

test('a link fires error when its stylesheet cannot be had, and has no sheet', async () => {
  using web = await page('<!doctype html><html><head></head><body></body></html>')
  web.run(`${LINK}; link.href = '/missing.css'; document.head.append(link)`)
  web.dom._sheet(web.internals.idOf(web.run('link')), 2)
  await web.wait(5)
  assert.deepEqual(web.json('[log, link.sheet, document.styleSheets.length]'), [['error'], null, 0])
})

test('the events of a link are delivered without a frame, also for a new href, and each load has its own sheet object', async () => {
  using web = await page('<!doctype html><html><head></head><body></body></html>')
  web.run(`${LINK}; link.href = '/a.css'; document.head.append(link)`)
  const id = web.internals.idOf(web.run('link'))
  web.dom._sheet(id, 1)
  await web.wait(5)
  assert.deepEqual(web.json('log'), ['load'])
  web.run(`globalThis.first = link.sheet; link.media = 'print'; link.href = '/b.css'`)
  assert.deepEqual(web.json('[link.sheet === first, link.sheet.media.mediaText, link.sheet.media.length]'), [true, 'print', 1], 'the sheet it had stays until the answer')
  web.dom._sheet(id, 1)
  await web.wait(5)
  assert.deepEqual(web.json('[log, link.sheet === first, link.sheet.href]'), [['load', 'load'], false, 'http://acde.local/b.css'])
})

test('a link that changed or left before its answer was delivered fires nothing for it', async () => {
  using web = await page('<!doctype html><html><head></head><body></body></html>')
  web.run(`${LINK}; link.href = '/a.css'; document.head.append(link)`)
  const id = web.internals.idOf(web.run('link'))
  web.dom._sheet(id, 1)
  web.dom.sheets.set(id, 0)
  await web.wait(5)
  assert.deepEqual(web.json('log'), [], 'a newer fetch is out')
  web.dom._sheet(id, 2)
  web.run('link.remove()')
  await web.wait(5)
  assert.deepEqual(web.json('[log, link.sheet]'), [[], null], 'out of the document')
})

test('a link in the parsed document reaches its inline handlers', async () => {
  using web = await page(`<!doctype html><html><head><link id="l" rel="stylesheet" href="/site.css" onload="globalThis.seen = 'load ' + (this === document.getElementById('l'))"></head><body></body></html>`)
  const id = [...web.dom.querySelectorAll(web.internals.idOf(web.run('document')), 'link')][0]!
  web.dom._sheet(id, 1)
  await web.wait(5)
  assert.equal(web.run('seen'), 'load true')
  web.run(`document.body.innerHTML = '<link id="m" rel="stylesheet" href="/missing.css" onerror="globalThis.seen = \\'error\\'">'`)
  web.dom._sheet([...web.dom.querySelectorAll(web.internals.idOf(web.run('document')), '#m')][0]!, 2)
  await web.wait(5)
  assert.equal(web.run('seen'), 'error')
})

test('the rules of a sheet whose response was opaque are closed, whatever the link says afterwards', async () => {
  using web = await page('<!doctype html><html><head></head><body></body></html>')
  web.run(`${LINK}; link.href = 'https://cdn.example.com/site.css'; document.head.append(link)`)
  const id = web.internals.idOf(web.run('link'))
  web.dom._sheet(id, 1, true)
  await web.wait(5)
  const access = `[() => link.sheet.cssRules.length, () => link.sheet.rules.length, () => link.sheet.insertRule('p {}'), () => link.sheet.deleteRule(0)].map((access) => {
    try { access(); return 'ok' } catch (error) { return error.name + ': ' + error.message }
  })`
  const closed = [
    "SecurityError: Failed to read the 'cssRules' property from 'CSSStyleSheet': Cannot access rules",
    "SecurityError: Failed to read the 'rules' property from 'CSSStyleSheet': Cannot access rules",
    "SecurityError: Failed to execute 'insertRule' on 'CSSStyleSheet': Cannot access StyleSheet to insertRule",
    "SecurityError: Failed to execute 'deleteRule' on 'CSSStyleSheet': Cannot access StyleSheet to deleteRule",
  ]
  assert.deepEqual(web.json(access), closed)
  assert.equal(web.run('link.sheet.href'), 'https://cdn.example.com/site.css')
  web.run(`globalThis.held = link.sheet; link.crossOrigin = 'anonymous'; link.href = '/own.css'`)
  assert.deepEqual(web.json(access), closed, 'the sheet it still has is the opaque one')
  assert.deepEqual(web.json('[link.sheet === held, link.sheet.href]'), [true, 'https://cdn.example.com/site.css'])
  web.dom._sheet(id, 1, false)
  await web.wait(5)
  assert.deepEqual(web.json<string[]>(access).slice(0, 2), ['ok', 'ok'], 'the next sheet is its own')
  assert.deepEqual(web.json(`[link.sheet.href, link.sheet === held, (() => { try { return held.cssRules.length } catch (error) { return error.name } })()]`), ['http://acde.local/own.css', false, 'SecurityError'])
})

test('what was taken from a sheet reads that sheet only, not the one the link gets next', async () => {
  using web = await page('<!doctype html><html><head></head><body></body></html>')
  web.run(`${LINK}; link.href = '/own.css'; document.head.append(link)`)
  const id = web.internals.idOf(web.run('link'))
  web.dom._sheet(id, 1, false, 'http://acde.local/own.css', ['p { color: red }', 'a { color: blue }'])
  await web.wait(5)
  web.run('globalThis.sheet = link.sheet; globalThis.list = sheet.cssRules; globalThis.rule = list[0]')
  assert.deepEqual(web.json('[list.length, rule.cssText, list[1].cssText, [...list].length]'), [2, 'p { color: red }', 'a { color: blue }', 2])
  web.run(`link.href = 'https://cdn.example.com/secret.css'`)
  web.dom._sheet(id, 1, true, 'https://cdn.example.com/secret.css', ['.secret { color: green }', '.more { color: black }', '.third {}'])
  await web.wait(5)
  const after = web.json(`[list.length, list[0] === undefined, list.item(0), [...list].length, 0 in list, sheet.cssRules === list, sheet.cssRules.length, sheet.rules.length, sheet.href, rule.cssText,
    link.sheet === sheet, (() => { try { return link.sheet.cssRules.length } catch (error) { return error.name } })()]`)
  assert.deepEqual(after, [0, true, null, 0, false, true, 0, 0, 'http://acde.local/own.css', 'p { color: red }', false, 'SecurityError'])
  assert.equal(web.run(`sheet.insertRule('b {}')`), 0)
  assert.deepEqual(web.json('[sheet.cssRules.length, link.textContent, document.styleSheets[0] === link.sheet]'), [1, '', true], 'the sheet that left keeps what is put in it to itself')
})

test('only a constructed sheet can be replaced', async () => {
  using web = await page('<!doctype html><html><head><style>p {}</style></head><body></body></html>')
  const result = web.json<string[]>(`(() => {
    const out = []
    const made = new CSSStyleSheet()
    made.replaceSync('a {} b {}')
    out.push(made.cssRules.length, made.href)
    try { document.styleSheets[0].replaceSync('a {}') } catch (error) { out.push(error.name + ': ' + error.message) }
    return out
  })()`)
  assert.deepEqual(result, [2, null, "NotAllowedError: Failed to execute 'replaceSync' on 'CSSStyleSheet': Can't call replaceSync on non-constructed CSSStyleSheets."])
  assert.equal(await web.run<Promise<string>>(`document.styleSheets[0].replace('a {}').then(() => 'resolved', (error) => error.name)`), 'NotAllowedError')
})
