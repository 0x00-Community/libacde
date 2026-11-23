import assert from 'node:assert/strict'
import { test } from 'node:test'
import { counterStyleName, counterStyleParts, counterStyleSettable, identOf } from '../src/dom/counter-style.ts'
import { page } from './harness.ts'

test('identOf reads one CSS identifier with its escapes, anything else is null', () => {
  assert.equal(identOf('foo'), 'foo')
  assert.equal(identOf('  foo\t'), 'foo')
  assert.equal(identOf('foo\\{\\}bar'), 'foo{}bar')
  assert.equal(identOf('\\31 23'), '123')
  assert.equal(identOf('--x'), '--x')
  for (const text of ['', '123', '-1x', 'foo bar', '"foo"', 'a,b']) assert.equal(identOf(text), null, text)
})

test('counterStyleName refuses none, CSS-wide keywords, default and the names that cannot be redefined', () => {
  assert.equal(counterStyleName('lower-alpha'), 'lower-alpha')
  for (const name of ['none', 'INITIAL', 'inherit', 'unset', 'revert-layer', 'default', 'decimal', 'Disc', 'square', 'circle', 'disclosure-open', 'disclosure-closed']) {
    assert.equal(counterStyleName(name), null, name)
  }
})

test('descriptors split at top-level semicolons only; system keeps its algorithm; alphabetic needs two symbols', () => {
  const { prelude, descriptors } = counterStyleParts('@counter-style foo { system: alphabetic; symbols: ";" url("a;b") X; }')
  assert.equal(prelude, '@counter-style foo')
  assert.deepEqual([...descriptors], [['system', 'alphabetic'], ['symbols', '";" url("a;b") X']])
  assert.equal(counterStyleSettable(descriptors, 'system', 'alphabetic'), true)
  assert.equal(counterStyleSettable(descriptors, 'system', 'numeric'), false)
  assert.equal(counterStyleSettable(descriptors, 'symbols', '"a b"'), false)
  assert.equal(counterStyleSettable(descriptors, 'symbols', '"a b" c'), true)
  assert.equal(counterStyleSettable(new Map(), 'system', 'symbolic'), true)
  assert.equal(counterStyleSettable(new Map([['system', 'extends decimal']]), 'system', 'extends upper-alpha'), true)
})

test('@counter-style rules are CSSCounterStyleRule with name and descriptor accessors', async () => {
  using web = await page('<!doctype html><html><head></head><body></body></html>')
  const result = web.json(`(() => {
    const sheet = new CSSStyleSheet()
    sheet.insertRule('@counter-style foo { system: fixed; symbols: A B C; }')
    const rule = sheet.cssRules[0]
    const read = () => [rule.name, rule.system, rule.symbols, rule.pad, rule.cssText]
    const start = read()
    rule.name = 'bar'
    rule.name = '123'
    rule.name = 'none'
    rule.name = 'decimal'
    rule.system = 'numeric'
    rule.system = 'fixed 3'
    rule.pad = '2 "0"'
    return [rule.constructor.name, rule.type, CSSRule.COUNTER_STYLE_RULE, rule instanceof CSSRule, start, read()]
  })()`)
  assert.deepEqual(result, [
    'CSSCounterStyleRule', 11, 11, true,
    ['foo', 'fixed', 'A B C', '', '@counter-style foo { system: fixed; symbols: A B C; }'],
    ['bar', 'fixed 3', 'A B C', '2 "0"', '@counter-style bar { system: fixed 3; symbols: A B C; pad: 2 "0"; }'],
  ])
})
