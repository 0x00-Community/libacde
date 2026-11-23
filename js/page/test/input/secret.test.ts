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
