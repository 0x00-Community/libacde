import assert from 'node:assert/strict'
import { test } from 'node:test'
import { bindHost } from '../../../dom/src/core/host.ts'
import { page } from '../../../dom/test/harness.ts'
import { MockHost } from '../../../dom/test/mock/host.ts'
import { InputMethod, type ImePanelEvent, type NativeIme } from '../../src/ime.ts'
import { ImeLayer, ImePanel } from '../../src/panel.ts'
import { TextEntry } from '../../src/text.ts'

bindHost({}, new MockHost())

const BOX = { x: 40, y: 60, width: 120, height: 30 }
const EVENT: ImePanelEvent = {
  type: 'panel', preedit: 'ni', preeditCursor: 2, auxUp: '', auxDown: '', candidates: [{ label: '1', text: '你' }],
  highlight: 0, layout: 'horizontal', hasPrev: false, hasNext: false,
}
const CARET = { x: 50, y: 40, width: 1, height: 18 }

async function floating(imeFloat = true) {
  const web = await page('<!doctype html><html><body><input id="field"></body></html>')
  ;(web.dom as unknown as { _box: () => typeof BOX })._box = () => BOX
  const shell: [number, number, number, number][] = []
  const dom: [number, number, number, number, number][] = []
  const native = Object.assign(web.dom, { setImeFloat: (id: number, x: number, y: number, w: number, h: number) => void dom.push([id, x, y, w, h]) })
  const surface = {
    imeFloat, setImeFloat: (x: number, y: number, w: number, h: number) => void shell.push([x, y, w, h]),
    setClipboard: () => {}, clipboardTypes: async () => [], readClipboard: async () => null,
  }
  return { web, shell, dom, native, surface }
}

test('without a floating host the candidate box stays in the page and nothing is sent', async () => {
  const { web, shell, native, surface } = await floating(false)
  using _ = web
  const layer = new ImeLayer(web.context['document'] as Document, native, (node) => web.internals.idOf(node as never))
  layer.attach(surface)
  const panel = new ImePanel(web.context['document'] as Document, layer, { select: () => {}, page: () => {} })
  panel.show(EVENT, CARET)
  assert.equal(web.run(`document.querySelector('[data-acde-ime-panel]').parentNode === document.body`), true)
  assert.match(web.run(`document.querySelector('[data-acde-ime-panel]').getAttribute('style')`), /^position:fixed;/)
  assert.deepEqual(shell, [])
})

test('a floating host gets the box of the float, once per change, and none when the box hides', async () => {
  const { web, shell, dom, native, surface } = await floating()
  using _ = web
  const layer = new ImeLayer(web.context['document'] as Document, native, (node) => web.internals.idOf(node as never))
  layer.attach(surface)
  const panel = new ImePanel(web.context['document'] as Document, layer, { select: () => {}, page: () => {} })
  panel.show(EVENT, CARET)
  panel.show(EVENT, CARET)
  assert.equal(web.run(`document.querySelector('[data-acde-ime-panel]').parentNode.hasAttribute('data-acde-ime-float')`), true)
  assert.match(web.run(`document.querySelector('[data-acde-ime-panel]').getAttribute('style')`), /^position:absolute;/)
  panel.hide()
  assert.deepEqual(shell, [[40, 60, 120, 30], [0, 0, 0, 0]])
  const root = web.internals.idOf(web.run('document.querySelector("[data-acde-ime-float]")'))
  assert.deepEqual(dom, [[root, 40, 60, 120, 30], [root, 0, 0, 0, 0]], "the float's own subtree is drawn as the float panel")
})

function entry(native: object, surface: object, web: Awaited<ReturnType<typeof page>>) {
  const text = new TextEntry(web.internals, native as never)
  const ime: NativeIme = {
    backend: 'host', focusIn: () => {}, focusOut: () => {}, reset: () => {}, setCursorRect: () => {}, setSurroundingText: () => {},
    setCapability: () => {}, processKey: () => false, selectCandidate: () => {}, prevPage: () => {}, nextPage: () => {}, close: () => {},
  }
  text.attachIme(new InputMethod(() => ime))
  text.attach(surface as never)
  web.run(`document.getElementById('field').focus()`)
  return text
}

test('the float is dropped when the input ends', async () => {
  const { web, shell, native, surface } = await floating()
  using _ = web
  const text = entry(native, surface, web)
  text.preedit('ni')
  text.preedit('')
  assert.deepEqual(shell, [[40, 60, 120, 30], [0, 0, 0, 0]])
})

test('the float is dropped when the field loses focus or the window goes inactive', async () => {
  const { web, shell, native, surface } = await floating()
  using _ = web
  const text = entry(native, surface, web)
  text.report()
  text.preedit('ni')
  text.panel(EVENT)
  web.run(`document.getElementById('field').blur()`)
  text.report()
  assert.deepEqual(shell.at(-1), [0, 0, 0, 0])
  web.run(`document.getElementById('field').focus()`)
  text.report()
  text.panel(EVENT)
  assert.deepEqual(shell.at(-1), [40, 60, 120, 30])
  text.surfaceActive = false
  assert.deepEqual(shell.at(-1), [0, 0, 0, 0])
})

test('a click on the float picks the word under it and the wheel turns the page, without moving focus', async () => {
  const { web, native, surface } = await floating()
  using _ = web
  const picked: number[] = []
  const pages: boolean[] = []
  const layer = new ImeLayer(web.context['document'] as Document, native, (node) => web.internals.idOf(node as never))
  layer.attach(surface)
  const panel = new ImePanel(web.context['document'] as Document, layer, { select: (index) => void picked.push(index), page: (next) => void pages.push(next) })
  web.run(`document.getElementById('field').focus()`)
  panel.show({ ...EVENT, candidates: [{ label: '1', text: '你' }, { label: '2', text: '尼' }] }, CARET)
  web.run(`
    const word = document.querySelector('[data-acde-ime-panel] [data-index="1"] .ime-text')
    word.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true }))
    globalThis.__kept = !word.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true }))
    const box = document.querySelector('[data-acde-ime-panel]')
    box.dispatchEvent(new WheelEvent('wheel', { bubbles: true, cancelable: true, deltaY: 40 }))
    box.dispatchEvent(new WheelEvent('wheel', { bubbles: true, cancelable: true, deltaY: -40 }))
  `)
  assert.deepEqual(picked, [1])
  assert.deepEqual(pages, [true, false])
  assert.equal(web.run('globalThis.__kept'), true, 'the press does not take the focus')
  assert.equal(web.run(`document.activeElement.id`), 'field')
})

test('when the float content gets shorter the float rect shrinks with it', async () => {
  const { web, shell, native, surface } = await floating()
  using _ = web
  const boxes = new Map<number, typeof BOX>()
  ;(web.dom as unknown as { _box: (id: number) => typeof BOX })._box = (id) => boxes.get(id) ?? { x: 0, y: 0, width: 0, height: 0 }
  const text = entry(native, surface, web)
  const preedit = () => web.internals.idOf(web.run(`document.querySelector('[data-acde-ime-float] > span')`))
  text.preedit('nihaonihao')
  boxes.set(preedit(), { x: 50, y: 40, width: 200, height: 18 })
  text.preedit('nihaonihao ')
  boxes.set(preedit(), { x: 50, y: 40, width: 60, height: 18 })
  text.preedit('你hao')
  assert.deepEqual(shell.slice(-2), [[50, 40, 200, 18], [50, 40, 60, 18]])
})

test('a candidate box that would cross the right or bottom edge moves left or above the caret', () => {
  const at = { left: 427, below: 415, above: 388 }
  assert.deepEqual(ImePanel.fit({ width: 272, height: 37 }, at, [640, 480]), [368, 415], 'pulled left to end at the right edge')
  assert.deepEqual(ImePanel.fit({ width: 100, height: 37 }, at, [640, 480]), [427, 415], 'it fits as it is')
  assert.deepEqual(ImePanel.fit({ width: 100, height: 80 }, at, [640, 480]), [427, 308], 'above the caret when it does not fit below')
  assert.deepEqual(ImePanel.fit({ width: 900, height: 37 }, at, [640, 480]), [0, 415], 'wider than the page: from its left edge')
})

test('the float the host gets stays inside the page, with its content where the rect says', async () => {
  const { web, shell, native, surface } = await floating()
  using _ = web
  const width = web.run<number>('innerWidth')
  ;(web.dom as unknown as { _box: (id: number) => typeof BOX })._box = (id) => {
    const element = web.internals.nodeOf?.(id) as unknown as Element | undefined
    const style = element?.getAttribute?.('style') ?? ''
    const left = Number(/left:(-?[\d.]+)px/.exec(style)?.[1] ?? 0)
    const top = Number(/top:(-?[\d.]+)px/.exec(style)?.[1] ?? 0)
    return style.includes('data-acde') || !style ? { x: 0, y: 0, width: 0, height: 0 } : { x: left, y: top, width: 272, height: 37 }
  }
  const layer = new ImeLayer(web.context['document'] as Document, native, (node) => web.internals.idOf(node as never))
  layer.attach(surface)
  const panel = new ImePanel(web.context['document'] as Document, layer, { select: () => {}, page: () => {} })
  panel.show(EVENT, { x: width - 60, y: 40, width: 1, height: 18 })
  const [x, , w] = shell.at(-1)!
  assert.ok(x + w <= width, `the float ends inside the page: ${shell.at(-1)} in ${width}`)
  assert.equal(web.run(`document.querySelector('[data-acde-ime-panel]').getAttribute('style').match(/left:([\\d.]+)px/)[1]`), String(x))
})
