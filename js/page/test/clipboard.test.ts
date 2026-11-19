import assert from 'node:assert/strict'
import { test } from 'node:test'
import { page } from '../../dom/test/harness.ts'
import type { ClipboardBytes, Shell } from '../src/native.ts'
import { TextEntry } from '../src/text.ts'

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])

class HostClipboard implements Shell {
  entries: ClipboardBytes[] = []

  setClipboard(entries: readonly ClipboardBytes[]): void {
    this.entries = entries.map((entry) => ({ type: entry.type, data: entry.data.slice() }))
  }

  async clipboardTypes(): Promise<string[]> {
    return this.entries.map((entry) => entry.type)
  }

  async readClipboard(type: string): Promise<Uint8Array | null> {
    return this.entries.find((entry) => entry.type === type)?.data ?? null
  }
}

async function attached() {
  const web = await page('<!doctype html><html><body><div id="editor" tabindex="0"></div></body></html>')
  const host = new HostClipboard()
  const entry = new TextEntry(web.internals, web.dom as never)
  entry.attach(host)
  return { web, host, entry }
}

test('navigator.clipboard.write puts every type of a ClipboardItem on the host; read gives them back', async () => {
  const { web, host } = await attached()
  using _ = web
  web.context['__png'] = PNG
  await web.run(`navigator.clipboard.write([new ClipboardItem({
    'text/plain': 'hello',
    'image/png': new Blob([globalThis.__png], { type: 'image/png' }),
  })])`)
  assert.deepEqual(Array.from(host.entries, (e) => e.type), ['text/plain', 'image/png'])
  assert.equal(new TextDecoder().decode(host.entries[0]!.data), 'hello')
  assert.deepEqual([...host.entries[1]!.data], [...PNG])
  const read = await web.run<Promise<{ types: string[]; text: string; png: number[] }>>(`(async () => {
    const [item] = await navigator.clipboard.read()
    const png = new Uint8Array(await (await item.getType('image/png')).arrayBuffer())
    return { types: [...item.types], text: await (await item.getType('text/plain')).text(), png: [...png] }
  })()`)
  assert.deepEqual(JSON.parse(JSON.stringify(read)), { types: ['text/plain', 'image/png'], text: 'hello', png: [...PNG] })
  assert.equal(await web.run('navigator.clipboard.readText()'), 'hello')
  assert.deepEqual(web.json(`[ClipboardItem.supports('image/png'), ClipboardItem.supports('text/html')]`), [true, false])
  await assert.rejects(web.run(`navigator.clipboard.write([new ClipboardItem({ 'text/html': '<b>x</b>' })])`), /not supported on write/)
})

test('paste hands the image to the page as a file item next to the text', async () => {
  const { web, host, entry } = await attached()
  using _ = web
  host.setClipboard([{ type: 'text/plain', data: new TextEncoder().encode('caption') }, { type: 'image/png', data: PNG }])
  web.run(`globalThis.__pasted = null
    document.getElementById('editor').addEventListener('paste', (e) => {
      const items = [...e.clipboardData.items].map((i) => [i.kind, i.type])
      globalThis.__pasted = { items, files: [...e.clipboardData.files].map((f) => [f.name, f.type, f.size]), text: e.clipboardData.getData('text/plain') }
      e.preventDefault()
    })`)
  await entry.clipboard(web.run('document.getElementById("editor")'), 'v')
  assert.deepEqual(web.json('globalThis.__pasted'), {
    items: [['string', 'text/plain'], ['file', 'image/png']],
    files: [['image.png', 'image/png', PNG.length]],
    text: 'caption',
  })
})
