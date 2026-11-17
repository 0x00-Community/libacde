import assert from 'node:assert/strict'
import { test } from 'node:test'
import { types } from 'node:util'
import { bindHost } from '../src/core/host.ts'
import { compare, fromKey, INVALID, toKey, toMultiEntryKeys, validKeyPath, type Key } from '../src/storage/idb/keys.ts'

bindHost(globalThis, { isProxy: (value: unknown) => types.isProxy(value) } as never)

function key(value: unknown): Key {
  const k = toKey(value)
  assert.notEqual(k, INVALID, `${String(value)} is a key`)
  return k as Key
}

test('byte order of encoded keys is the IndexedDB key order', () => {
  const ordered: unknown[] = [
    -Infinity, -1e300, -1, -Number.MIN_VALUE, 0, Number.MIN_VALUE, 0.5, 1, 2, 1e300, Infinity,
    new Date(-1), new Date(0), new Date(1),
    '', '\u0000', '\u0000\u0000', '\u0001', 'A', 'a', 'aa', 'ab', 'b', 'ÿ', 'Ā', '\ud800', '\udc00', '￿',
    new Uint8Array([]).buffer, new Uint8Array([0]).buffer, new Uint8Array([0, 0]).buffer, new Uint8Array([1]).buffer, new Uint8Array([255]).buffer,
    [], [-1], [0], [0, 0], [0, ''], [1], ['a'], [[]], [[0]], [[[]]],
  ]
  const encoded = ordered.map(key)
  for (let i = 0; i < encoded.length; i++) {
    for (let j = 0; j < encoded.length; j++) {
      assert.equal(compare(encoded[i]!, encoded[j]!), Math.sign(i - j), `${i} vs ${j}`)
    }
  }
})

test('keys decode back to equal values; -0 is 0', () => {
  assert.equal(Object.is(fromKey(key(-0)), 0), true)
  assert.deepEqual(fromKey(key(['x', 3, [new Date(7)], 'lone \ud800'])), ['x', 3, [new Date(7)], 'lone \ud800'])
  const bytes = fromKey(key(new Uint16Array([1, 0xffff])))
  assert.ok(bytes instanceof ArrayBuffer)
  assert.deepEqual([...new Uint8Array(bytes)], [...new Uint8Array(new Uint16Array([1, 0xffff]).buffer)])
  assert.equal(compare(key(new Uint8Array([1, 2]).buffer), key(new DataView(new Uint8Array([9, 1, 2]).buffer, 1))), 0)
})

test('values that are not keys', () => {
  const cyclic: unknown[] = []
  cyclic.push(cyclic)
  const shared: unknown[] = []
  for (const value of [NaN, new Date(NaN), null, undefined, true, {}, Symbol('s'), [NaN], [{}], cyclic, [shared, shared], [, 1], new Proxy([1], {})]) {
    assert.equal(toKey(value), INVALID, String(value))
  }
  const detached = new ArrayBuffer(4)
  structuredClone(detached, { transfer: [detached] })
  assert.equal(toKey(detached), INVALID)
})

test('multiEntry keys drop invalid members and duplicates', () => {
  const keys = toMultiEntryKeys(['a', 'a', NaN, {}, 1, [1], 1]) as Key[]
  assert.deepEqual(keys.map(fromKey), ['a', 1, [1]])
})

test('key paths', () => {
  for (const ok of ['', 'a', 'a.b', '$x._y', 'ünï', ['a', 'b.c']]) assert.equal(validKeyPath(ok), true, String(ok))
  for (const bad of ['a.', '.a', 'a..b', '1a', 'a b', 'a-b', [], ['a', '1']]) assert.equal(validKeyPath(bad as string), false, String(bad))
})
