import assert from 'node:assert/strict'
import { test } from 'node:test'
import { chordCodes, keyCode } from '../src/keys.ts'

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
