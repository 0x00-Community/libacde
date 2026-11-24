import assert from 'node:assert/strict'
import { test } from 'node:test'
import { ShellInput, chordCodes, chordMatches, keyCode } from '../src/keys.ts'

const shift = keyCode('shift')!

test('printable characters press their key, with Shift for the shifted ones', () => {
  assert.deepEqual(chordCodes('7'), [keyCode('7')])
  assert.deepEqual(chordCodes('.'), [keyCode('.')])
  assert.deepEqual(chordCodes('*'), [shift, keyCode('8')])
  assert.deepEqual(chordCodes('+'), [shift, keyCode('=')])
  assert.deepEqual(chordCodes('('), [shift, keyCode('9')])
  assert.deepEqual(chordCodes('A'), [shift, keyCode('a')])
})

test('named keys and chords with a plus', () => {
  assert.deepEqual(chordCodes('plus'), [shift, keyCode('=')])
  assert.deepEqual(chordCodes('multiply'), [shift, keyCode('8')])
  assert.deepEqual(chordCodes('NumpadMultiply'), [keyCode('NumpadMultiply')])
  assert.deepEqual(chordCodes('shift+8'), [shift, keyCode('8')])
  assert.deepEqual(chordCodes('ctrl++'), [keyCode('ctrl'), shift, keyCode('=')])
  assert.equal(chordCodes('nosuchkey'), null)
  assert.equal(chordCodes('ctrl+'), null)
})

test('the numeric keypad has its own keys', () => {
  for (const name of ['Numpad0', 'Numpad7', 'NumpadAdd', 'NumpadSubtract', 'NumpadMultiply', 'NumpadDivide', 'NumpadDecimal', 'NumpadEnter']) {
    assert.notEqual(keyCode(name), null, name)
  }
  assert.notEqual(keyCode('Numpad7'), keyCode('7'))
})

test('a key with the held modifiers matches a menu chord only when both agree', () => {
  const n = keyCode('n')!
  assert.equal(chordMatches('Alt+N', n, { alt: true }), true)
  assert.equal(chordMatches('Alt+N', n, { alt: true, shift: true }), false)
  assert.equal(chordMatches('Alt+N', n, {}), false)
  assert.equal(chordMatches('Ctrl+Shift+S', keyCode('s')!, { ctrl: true, shift: true }), true)
  assert.equal(chordMatches('mod+1', keyCode('1')!, { ctrl: true }), true)
  assert.equal(chordMatches('mod+1', keyCode('1')!, { meta: true }, true), true)
})

test('a pen keeps its state on pointer events and the eraser tip presses button 5', () => {
  const input = new ShellInput(() => 7, 1)
  const pen = { pressure: 0.4, tangentialPressure: 0, tiltX: 12, tiltY: -3, twist: 0, eraser: false }
  const eraser = { ...pen, eraser: true }
  const pressed = (raw: Parameters<ShellInput['normalize']>[0]) => {
    const event = input.normalize(raw)
    return event && 'button' in event ? [event.button, 'pen' in event ? event.pen : undefined] : null
  }
  assert.deepEqual(pressed({ type: 'pointerdown', x: 1, y: 2, button: 272, pen }), [0, pen])
  assert.deepEqual(pressed({ type: 'pointerdown', x: 1, y: 2, button: 272, pen: eraser }), [5, eraser])
  assert.deepEqual(pressed({ type: 'pointerup', x: 1, y: 2, button: 273, pen: eraser }), [2, eraser])
  assert.deepEqual(pressed({ type: 'pointerdown', x: 1, y: 2, button: 272 }), [0, undefined])
})
