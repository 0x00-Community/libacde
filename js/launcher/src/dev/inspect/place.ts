import type { InspectArg, InspectWindow } from '@acde/api'
import type { ActResult } from '../answers'
import type { InspectHost, PlaceChange, ShellWindow } from './types'

type Rect = readonly [number, number, number, number]

export class Placing {
  static readonly SETTLE_MS = 2000
  static readonly POLL_MS = 50

  static point(arg: InspectArg): { x: number; y: number } | null {
    const m = typeof arg === 'string' ? /^\s*(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)\s*$/.exec(arg) : null
    return m ? { x: Number(m[1]), y: Number(m[2]) } : null
  }

  static size(arg: InspectArg): { width: number; height: number } | null {
    const m = typeof arg === 'string' ? /^\s*(\d+(?:\.\d+)?)\s*[x×]\s*(\d+(?:\.\d+)?)\s*$/i.exec(arg) : null
    const size = m ? { width: Number(m[1]), height: Number(m[2]) } : null
    return size && size.width > 0 && size.height > 0 ? size : null
  }

  static unminimizable(row: ShellWindow | undefined): string | null {
    if (!row || row.minimize !== false) return null
    if ((row.window ?? row.id) === 0) return 'the desktop stays behind every window'
    if (row.mode === 'fullscreen') return "it fills the host window, which only the host window's own controls minimize here"
    if (row.mode === 'pinned') return 'a pinned window stays where it is'
    return 'a standalone app has no Dock to bring it back'
  }

  static refused(window: InspectWindow, action: string): string | null {
    if (window.kind === 'desktop') return `the desktop cannot ${action}`
    if (window.mode === 'fullscreen') return `${window.ref} fills the host window (a single-app session), so it cannot ${action}`
    if (window.mode === 'pinned') return `${window.ref} is a pinned panel, which its desktop places`
    if (window.hidden) return `${window.ref} is minimized; act ${window.ref} restore first`
    const state = window.state ?? 'normal'
    if (state !== 'normal') return `${window.ref} is ${state}; only a normal window can ${action}: act ${window.ref} restore first`
    return null
  }

  constructor(
    readonly host: InspectHost,
    readonly windows: () => readonly InspectWindow[],
    readonly focus: (window: InspectWindow) => void,
  ) {}

  fill(window: InspectWindow, state: 'filled' | 'fullscreen'): ActResult {
    if (window.kind === 'desktop') return { ok: false, text: `the desktop cannot be made ${state}` }
    if (window.mode === 'fullscreen') return { ok: false, text: `${window.ref} already fills the host window` }
    if (!this.host.shell.setState) return { ok: false, text: `this session cannot make ${window.ref} ${state}` }
    if (window.hidden) this.host.hide(window.id, false)
    this.host.shell.setState(window.id, state)
    this.focus(window)
    return { ok: true, text: `${window.ref} is ${state}` }
  }

  minimize(window: InspectWindow): ActResult {
    if (window.kind === 'desktop') return { ok: false, text: 'the desktop cannot be minimized' }
    const reason = Placing.unminimizable(this.host.shell.windows().find((row) => (row.window ?? row.id) === window.id))
    if (reason) return { ok: false, text: `${window.ref} cannot be minimized: ${reason}` }
    if (this.host.shell.setState) this.host.shell.setState(window.id, 'minimized')
    else this.host.hide(window.id, true)
    return { ok: true, text: `${window.ref} is minimized` }
  }

  raise(window: InspectWindow): ActResult {
    if (window.kind === 'desktop') return { ok: false, text: 'the desktop stays behind every window; it cannot be raised' }
    if (window.hidden) return { ok: false, text: `${window.ref} is minimized; act ${window.ref} restore first` }
    this.focus(window)
    return { ok: true, text: `${window.ref} is in front` }
  }

  async move(window: InspectWindow, arg: InspectArg): Promise<ActResult> {
    const to = Placing.point(arg)
    if (!to) return { ok: false, text: `move needs where to: screen x,y like "100,80", not ${JSON.stringify(arg)}` }
    return this.#place(window, 'move', { x: to.x, y: to.y })
  }

  async resize(window: InspectWindow, arg: InspectArg): Promise<ActResult> {
    const size = Placing.size(arg)
    if (!size) return { ok: false, text: `resize needs a size: WxH like "800x600", not ${JSON.stringify(arg)}` }
    if (window.mode === 'fixed') return { ok: false, text: `${window.ref} has a fixed size; act ${window.ref} move places it` }
    const [x, y] = window.rect
    return this.#place(window, 'resize', { x, y, ...size })
  }

  async #place(window: InspectWindow, action: 'move' | 'resize', change: PlaceChange): Promise<ActResult> {
    const refused = Placing.refused(window, action)
    if (refused) return { ok: false, text: refused }
    const place = this.host.shell.place
    if (!place) return { ok: false, text: `this session cannot ${action} ${window.ref}` }
    const answered = await place(window.id, change)
    const same = (a: number, b: number | undefined) => b === undefined || Math.round(a) === Math.round(b)
    const wanted = (rect: Rect) => same(rect[2], change.width) && same(rect[3], change.height)
    const rect = wanted(answered) ? answered : await this.#settled(window.id, wanted, answered)
    const shown = `${Math.round(rect[0])},${Math.round(rect[1])} ${Math.round(rect[2])}x${Math.round(rect[3])}`
    if (!wanted(rect)) return { ok: true, text: `${window.ref} is at ${shown}; the app has not taken the size ${change.width}x${change.height} yet` }
    return { ok: true, text: `${window.ref} is at ${shown}` }
  }

  async #settled(id: number, wanted: (rect: Rect) => boolean, last: Rect): Promise<Rect> {
    const started = Date.now()
    let rect = last
    while (Date.now() - started < Placing.SETTLE_MS) {
      await new Promise((resolve) => setTimeout(resolve, Placing.POLL_MS))
      rect = this.windows().find((w) => w.id === id)?.rect ?? rect
      if (wanted(rect)) return rect
    }
    return rect
  }
}
