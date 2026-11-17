import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { Frame } from '../src/frame.ts'

describe('Frame.desktop', () => {
  test('setDoubleClick hands each declared action to the runtime', () => {
    const calls: string[] = []
    const desktop = Frame.desktop({ setDoubleClick: (action) => void calls.push(action) })
    desktop.setDoubleClick('minimize')
    desktop.setDoubleClick('none')
    desktop.setDoubleClick('fill')
    assert.deepEqual(calls, ['minimize', 'none', 'fill'])
  })

  test('setFrameTheme hands the checked theme to the runtime as numbers', () => {
    const calls: number[][] = []
    const desktop = Frame.desktop({ setFrameTheme: (...args) => void calls.push(args) })
    desktop.setFrameTheme({ active: { background: '#000' }, buttonSide: 'left' })
    desktop.setFrameTheme(null)
    const none = (n: number) => Array<number>(n).fill(-1)
    assert.deepEqual(calls, [
      [0x000000ff, ...none(15), 0, 1, 0],
      [...none(16), 0, 0, 0],
    ])
  })

  test('a bad theme is a TypeError and changes nothing', () => {
    const calls: number[][] = []
    const desktop = Frame.desktop({ setFrameTheme: (...args) => void calls.push(args) })
    assert.throws(() => desktop.setFrameTheme({ active: { background: 'blue' } }), (e: Error) => e instanceof TypeError && /active.background/.test(e.message))
    assert.deepEqual(calls, [])
  })

  test('keys this runtime does not know are left out, not refused', () => {
    const calls: number[][] = []
    const warned: string[] = []
    const desktop = Frame.desktop({ setFrameTheme: (...args) => void calls.push(args) }, (line) => void warned.push(line))
    desktop.setFrameTheme({ shadow: true, active: { background: '#000' } } as never)
    assert.deepEqual(warned, ['setFrameTheme: ignored shadow (not known to this runtime)'])
    assert.equal(calls.length, 1)
    assert.equal(calls[0]![0], 0x000000ff)
  })

  test('an unknown action is a TypeError and changes nothing', () => {
    const calls: string[] = []
    const desktop = Frame.desktop({ setDoubleClick: (action) => void calls.push(action) })
    assert.throws(() => desktop.setDoubleClick('maximize' as never), TypeError)
    assert.throws(() => desktop.setDoubleClick(undefined as never), TypeError)
    assert.deepEqual(calls, [])
  })
})

describe('Frame.darkPage', () => {
  const page = (computed: string, meta: string | null) => ({
    document: {
      documentElement: {} as Element,
      head: null as Element | null,
      querySelector: () => (meta === null ? null : ({ getAttribute: () => meta } as unknown as Element)),
    },
    getComputedStyle: () => ({ getPropertyValue: () => computed }),
    MutationObserver: class {
      observe(): void {}
      disconnect(): void {}
    },
  })

  test('a page supports dark when its root color-scheme or its color-scheme meta names dark', () => {
    assert.equal(Frame.darkPage(page('light dark', null)), true)
    assert.equal(Frame.darkPage(page('dark', null)), true)
    assert.equal(Frame.darkPage(page('normal', 'light dark')), true)
    assert.equal(Frame.darkPage(page('only light', null)), false)
    assert.equal(Frame.darkPage(page('normal', null)), false)
    assert.equal(Frame.darkPage(page('', 'darker')), false)
  })

  test('followScheme tells the surface at once and again only when the answer changes', () => {
    let computed = 'normal'
    let changed = () => {}
    const tells: boolean[] = []
    const watched = {
      ...page('', null),
      getComputedStyle: () => ({ getPropertyValue: () => computed }),
      MutationObserver: class {
        constructor(callback: () => void) {
          changed = callback
        }
        observe(): void {}
        disconnect(): void {}
      },
    }
    Frame.followScheme(watched, { setPageDark: (dark) => void tells.push(dark) }, (run) => run())
    changed()
    computed = 'dark'
    changed()
    changed()
    assert.deepEqual(tells, [false, true])
  })

  test('the frame is dark for a page that is dark now: dark only, or light and dark in a dark scheme', () => {
    assert.equal(Frame.darkFrame(page('dark', null), false), true)
    assert.equal(Frame.darkFrame(page('only dark', null), false), true)
    assert.equal(Frame.darkFrame(page('light dark', null), false), false)
    assert.equal(Frame.darkFrame(page('light dark', null), true), true)
    assert.equal(Frame.darkFrame(page('normal', 'dark light'), true), true)
    assert.equal(Frame.darkFrame(page('normal', null), true), false, 'a page that names no scheme is light')
    assert.equal(Frame.darkFrame(page('only light', null), true), false)
  })

  test('followScheme follows prefers-color-scheme for a page that supports both', () => {
    let dark = false
    let flip = () => {}
    const tells: boolean[] = []
    const watched = {
      ...page('light dark', null),
      matchMedia: (query: string) => {
        assert.equal(query, '(prefers-color-scheme: dark)')
        return {
          get matches() {
            return dark
          },
          addEventListener: (type: 'change', listener: () => void) => void (flip = listener),
        }
      },
    }
    Frame.followScheme(watched, { setPageDark: (value) => void tells.push(value) }, (run) => run())
    dark = true
    flip()
    dark = false
    flip()
    assert.deepEqual(tells, [false, true, false])
  })

  test('followScheme watches the root, the head and its scheme parts, never a subtree', () => {
    const text = { nodeType: 3 }
    const style = { localName: 'style', childNodes: [text] } as unknown as Element
    const meta = { localName: 'meta', childNodes: [] } as unknown as Element
    const root = {} as Element
    const head = { children: [meta, style] } as unknown as Element
    const watched: [unknown, MutationObserverInit][] = []
    let styled = 0
    const document = { documentElement: root, head, querySelector: () => null }
    const page = {
      document,
      getComputedStyle: () => {
        styled += 1
        return { getPropertyValue: () => 'normal' }
      },
      MutationObserver: class {
        observe(target: unknown, options: MutationObserverInit): void {
          watched.push([target, options])
        }
        disconnect(): void {}
      },
    }
    Frame.followScheme(page, { setPageDark: () => {} }, (run) => run())
    const filter = ['name', 'content', 'rel', 'href', 'media', 'disabled']
    assert.deepEqual(watched, [
      [document, { childList: true }],
      [root, { attributes: true, childList: true }],
      [head, { childList: true }],
      [meta, { attributes: true, attributeFilter: filter, childList: false }],
      [style, { attributes: true, attributeFilter: filter, childList: true }],
      [text, { characterData: true }],
    ])
    assert.ok(watched.every(([, options]) => !options.subtree), 'a subtree observer makes every DOM change walk its ancestors')
    assert.equal(styled, 1, 'the computed style is read once, not for every change in the body')
  })

  test("the root's color-scheme wins over the meta, as in browsers", () => {
    assert.equal(Frame.darkFrame(page('dark', 'light dark'), false), true, 'an app that switched itself to dark')
    assert.equal(Frame.darkFrame(page('light', 'light dark'), true), false, 'an app that switched itself to light')
    assert.equal(Frame.darkFrame(page('normal', 'dark'), false), true, 'the meta when the root says nothing')
    assert.equal(Frame.darkFrame(page('', 'light dark'), true), true)
  })
})
