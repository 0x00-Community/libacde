import assert from 'node:assert/strict'
import { test } from 'node:test'
import { page } from './harness.ts'

test('install makes the global a Window with a document', async () => {
  using web = await page('<!doctype html><html><head><title>t</title></head><body><div id="a" class="x y">hi</div></body></html>')
  assert.equal(web.run('Object.getPrototypeOf(globalThis) === Window.prototype'), true)
  assert.equal(web.run('Object.prototype.toString.call(Object.getPrototypeOf(Window.prototype))'), '[object WindowProperties]')
  assert.equal(web.run('Object.getPrototypeOf(Object.getPrototypeOf(Window.prototype)) === EventTarget.prototype'), true)
  assert.equal(web.run('window === globalThis && self === window && document.defaultView === window'), true)
  assert.equal(web.run('document.title'), 't')
  assert.equal(web.run('document.getElementById("a").textContent'), 'hi')
  assert.equal(web.run('document.getElementById("a") === document.querySelector(".x")'), true)
  assert.equal(web.run('document.body.firstChild.classList.contains("y")'), true)
  assert.equal(web.run('document.doctype.name'), 'html')
  assert.equal(web.run('typeof setTimeout + typeof queueMicrotask + typeof fetch + typeof MutationObserver'), 'functionfunctionfunctionfunction')
  assert.deepEqual(web.host.errors, [])
})
