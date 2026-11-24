import assert from 'node:assert/strict'
import { test } from 'node:test'
import { bindHost } from '../../../dom/src/core/host.ts'
import { page } from '../../../dom/test/harness.ts'
import { MockHost } from '../../../dom/test/mock/host.ts'
import { InputMethod, type NativeIme } from '../../src/ime.ts'
import { TextEntry, type TextInputState } from '../../src/text.ts'

bindHost({}, new MockHost())

function recorded() {
  const calls: string[] = []
  const note = (name: string) => () => void calls.push(name)
  const native: NativeIme = {
    backend: 'host', focusIn: note('focusIn'), focusOut: note('focusOut'), reset: note('reset'), setCursorRect: note('rect'),
    setSurroundingText: note('surrounding'), setCapability: note('capability'), processKey: () => false, selectCandidate: note('select'),
    prevPage: note('prev'), nextPage: note('next'), close: note('close'),
  }
  return { calls, ime: new InputMethod(() => native) }
}

const state = (secret: boolean): TextInputState => ({ text: 'a', cursor: 1, anchor: 1, secret, rect: { x: 0, y: 0, width: 1, height: 10 } })

test('a secret field never enables the input method', () => {
  const { calls, ime } = recorded()
  ime.focus(state(true))
  assert.deepEqual(calls, [])
  assert.equal(ime.key('a', 38, undefined, false), false)
})

test('an ordinary field enables the input method and moving to a secret field disables it', () => {
  const { calls, ime } = recorded()
  ime.focus(state(false))
  assert.ok(calls.includes('focusIn'))
  calls.length = 0
  ime.focus(state(true))
  assert.deepEqual(calls, ['focusOut'])
})

test('password inputs and secret autocomplete tokens count as secret fields', async () => {
  using web = await page(`<!doctype html><html><body>
    <input id="plain"><input id="pw" type="password"><input id="otp" autocomplete="one-time-code">
    <input id="login" autocomplete="section-a Current-Password"><textarea id="key" autocomplete="new-password"></textarea>
    <input id="user" autocomplete="username"><div id="div" autocomplete="current-password"></div>
  </body></html>`)
  const secret = (id: string) => TextEntry.secret(web.run(`document.getElementById('${id}')`))
  assert.deepEqual(['plain', 'pw', 'otp', 'login', 'key', 'user', 'div'].map(secret), [false, true, true, true, true, false, false])
})

test('a field focused by script or by the window coming back takes the input method without a frame or a key', async () => {
  using web = await page('<!doctype html><html><body><input id="search"><button id="other">x</button></body></html>')
  const { calls, ime } = recorded()
  const text = new TextEntry(web.internals, web.dom as never)
  text.attachIme(ime)
  web.run(`document.getElementById('search').focus()`)
  await Promise.resolve()
  assert.ok(calls.includes('focusIn'), `focus() alone hands the field to the input method: ${calls}`)
  calls.length = 0
  web.run(`document.getElementById('other').focus()`)
  await Promise.resolve()
  assert.deepEqual(calls, ['focusOut'])
  calls.length = 0
  text.surfaceActive = false
  web.run(`document.getElementById('search').focus()`)
  await Promise.resolve()
  assert.equal(calls.length, 0, 'not while the window is inactive')
  text.surfaceActive = true
  await Promise.resolve()
  assert.ok(calls.includes('focusIn'), `the window coming back hands it over again: ${calls}`)
})
