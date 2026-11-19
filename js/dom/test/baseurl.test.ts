import assert from 'node:assert/strict'
import { test } from 'node:test'
import { page } from './harness.ts'

test('base url: the renderer resolves against the document url, and follows pushState / replaceState / traversal', async () => {
  using web = await page('<!doctype html><html><body></body></html>', { url: 'http://acde.local/games/kart/index.html' })
  assert.equal(web.dom.baseUrl, 'http://acde.local/games/kart/index.html')
  web.run('history.pushState(null, "", "levels/2.html")')
  assert.equal(web.dom.baseUrl, 'http://acde.local/games/kart/levels/2.html')
  web.run('history.replaceState(null, "", "/games/other/")')
  assert.equal(web.dom.baseUrl, 'http://acde.local/games/other/')
  web.run('history.back()')
  await web.wait(5)
  assert.equal(web.dom.baseUrl, 'http://acde.local/games/kart/index.html')
})
