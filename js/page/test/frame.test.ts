import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { Frame } from '../src/frame.ts'

const PRESETS: Record<string, { wire: number; duration: number }> = { standard: { wire: 0, duration: 514 }, reduced: { wire: 1, duration: 150 } }
const motion = (preset?: string) => {
  const found = PRESETS[preset ?? 'standard']
  return found ? { preset: preset ?? 'standard', ...found, easing: 'linear(0, 1)' } : null
}

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

  test('setWindowLook hands the checked look to the runtime as numbers, null as none', () => {
    const calls: number[][] = []
    const desktop = Frame.desktop({ setWindowLook: (...args) => void calls.push(args), motion })
    const layer = { x: 0, y: 8, blur: 24, spread: -4, color: '#00000040' }
    desktop.setWindowLook({ corners: { radius: 12, shape: 'superellipse(2.5)' }, shadows: { focused: [layer], unfocused: [] } })
    desktop.setWindowLook({ corners: { radius: 12 }, shadows: { focused: [], unfocused: [] }, motion: 'reduced' })
    desktop.setWindowLook(null)
    assert.deepEqual(calls, [[12, 2.5, 1, 0, 8, 24, -4, 0x00000040, 0, 0], [12, 1, 0, 0, 1], []])
  })

  test('setWindowLook passes the desktop titlebar height last, within 24 to 64 px, and runtime frames follow it', () => {
    const calls: number[][] = []
    const desktop = Frame.desktop({ setWindowLook: (...args) => void calls.push(args), motion })
    desktop.setWindowLook({ titlebarHeight: 40 })
    assert.deepEqual(calls, [[0, 1, 0, 0, 0, 40]])
    assert.throws(() => desktop.setWindowLook({ titlebarHeight: 80 }), /titlebarHeight is a number from 24 to 64/)
    assert.equal(Frame.height({ frame: true }, 40), 40)
    assert.equal(Frame.height({ frame: true }), Frame.HEIGHT)
    assert.equal(Frame.height({ frame: false }, 40), 0)
    assert.deepEqual(Frame.windowSize({ frame: true, size: [800, 600] } as never, 40), [800, 640])
  })

  test('setWindowLook passes the window controls and title metrics after the titlebar, -1 where the runtime keeps its own', () => {
    const calls: number[][] = []
    const desktop = Frame.desktop({ setWindowLook: (...args) => void calls.push(args), motion })
    desktop.setWindowLook({ controls: { inset: 12, size: 12, gap: 8 }, title: { weight: 590 } })
    assert.deepEqual(calls, [[0, 1, 0, 0, 0, 0, 12, 12, 8, -1, -1, 590]])
    assert.throws(() => desktop.setWindowLook({ controls: { size: 40 } }), /controls.size is a number from 6 to 32/)
  })

  test('setWindowLook changes only the keys it is given; null goes back to square, no shadows, standard', () => {
    const calls: number[][] = []
    const desktop = Frame.desktop({ setWindowLook: (...args) => void calls.push(args), motion })
    desktop.setWindowLook({ motion: 'reduced' })
    desktop.setWindowLook({ corners: { radius: 12, shape: 'squircle' } })
    desktop.setWindowLook({ shadows: { focused: [{ x: 0, y: 2, blur: 4, spread: 0, color: '#000000ff' }], unfocused: [] } })
    desktop.setWindowLook({ motion: 'standard' })
    desktop.setWindowLook(null)
    desktop.setWindowLook({ motion: 'reduced' })
    assert.deepEqual(calls, [
      [0, 1, 0, 0, 1],
      [12, 2, 0, 0, 1],
      [12, 2, 1, 0, 2, 4, 0, 0x000000ff, 0, 1],
      [12, 2, 1, 0, 2, 4, 0, 0x000000ff, 0, 0],
      [],
      [0, 1, 0, 0, 1],
    ])
  })

  test('motion() reads a preset from the runtime, by default the one of the last window look', () => {
    const desktop = Frame.desktop({ setWindowLook: () => {}, motion })
    assert.deepEqual(desktop.motion(), { preset: 'standard', duration: 514, easing: 'linear(0, 1)' })
    desktop.setWindowLook({ corners: { radius: 0 }, shadows: { focused: [], unfocused: [] }, motion: 'reduced' })
    assert.deepEqual(desktop.motion(), { preset: 'reduced', duration: 150, easing: 'linear(0, 1)' })
    assert.equal(desktop.motion('standard').preset, 'standard')
    desktop.setWindowLook(null)
    assert.equal(desktop.motion().preset, 'standard')
    assert.throws(() => desktop.motion('fast' as never), TypeError)
  })

  test('a bad window look is a TypeError and changes nothing', () => {
    const calls: number[][] = []
    const desktop = Frame.desktop({ setWindowLook: (...args) => void calls.push(args), motion })
    const corners = { radius: 12, shape: 'squircle' as const }
    const layer = { x: 0, y: 0, blur: 4, spread: 0, color: '#000000ff' }
    const bad = (look: unknown, pattern: RegExp) =>
      assert.throws(() => desktop.setWindowLook(look as never), (e: Error) => e instanceof TypeError && pattern.test(e.message))
    bad({ corners: { radius: 12, shape: 'superellipse(4)' }, shadows: { focused: [], unfocused: [] } }, /corners\.shape/)
    bad({ corners, shadows: { focused: [layer, layer, layer, layer, layer], unfocused: [] } }, /at most 4/)
    bad({ corners, shadows: { focused: [{ ...layer, color: 'black' }], unfocused: [] } }, /focused\[0\]\.color/)
    bad({ corners, shadows: { focused: [{ ...layer, blur: -1 }], unfocused: [] } }, /blur/)
    bad({ corners, shadows: { focused: [], unfocused: [] }, motion: 'fast' }, /motion/)
    assert.deepEqual(calls, [])
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

describe('Frame.grabs', () => {
  test('the desktop asks the surface for a pointer or keyboard grab and gives it back', () => {
    const calls: string[] = []
    const grabs = Frame.grabs({ requestGrab: (kind) => void calls.push(`on ${kind}`), releaseGrab: (kind) => void calls.push(`off ${kind}`) })
    grabs.requestGrab('keyboard')
    grabs.releaseGrab('keyboard')
    assert.deepEqual(calls, ['on keyboard', 'off keyboard'])
    assert.throws(() => grabs.requestGrab('mouse' as never), /kind is 'pointer' or 'keyboard'/)
  })
})
