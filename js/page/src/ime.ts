import { hostTime } from '@dopame/acde-dom'
import type { Modifiers } from './native'
import type { TextInputState } from './text'
import { Level, Log } from './log'

export interface NativeIme {
  readonly backend: 'fcitx5' | 'ibus' | 'host'
  focusIn(): void
  focusOut(): void
  reset(): void
  setCursorRect(x: number, y: number, width: number, height: number): void
  setSurroundingText(text: string, cursor: number, anchor: number): void
  setCapability(capability: { preedit?: boolean; surroundingText?: boolean; password?: boolean; clientPanel?: boolean }): void
  processKey(keysym: number, keycode: number, state: number, isRelease: boolean): boolean
  selectCandidate(index: number): void
  prevPage(): void
  nextPage(): void
  close(): void
}

export interface ImePanelEvent {
  type: 'panel'
  preedit: string
  preeditCursor: number
  auxUp: string
  auxDown: string
  candidates: { label: string; text: string }[]
  highlight: number
  layout: 'horizontal' | 'vertical' | 'default'
  hasPrev: boolean
  hasNext: boolean
}

export type ImeEvent =
  | ImePanelEvent
  | { type: 'preedit'; text: string; cursor: number }
  | { type: 'commit'; text: string }
  | { type: 'deleteSurrounding'; offset: number; length: number }
  | { type: 'forwardKey'; keysym: number; state: number; isRelease: boolean }

const KEYSYMS: Record<string, number> = {
  Backspace: 0xff08, Tab: 0xff09, Enter: 0xff0d, Escape: 0xff1b, Delete: 0xffff, Home: 0xff50, ArrowLeft: 0xff51,
  ArrowUp: 0xff52, ArrowRight: 0xff53, ArrowDown: 0xff54, PageUp: 0xff55, PageDown: 0xff56, End: 0xff57, Insert: 0xff63,
  Shift: 0xffe1, Control: 0xffe3, CapsLock: 0xffe5, Alt: 0xffe9, Meta: 0xffeb, ContextMenu: 0xff67,
  F1: 0xffbe, F2: 0xffbf, F3: 0xffc0, F4: 0xffc1, F5: 0xffc2, F6: 0xffc3, F7: 0xffc4, F8: 0xffc5, F9: 0xffc6, F10: 0xffc7, F11: 0xffc8, F12: 0xffc9,
}
const NAMES = Object.fromEntries(Object.entries(KEYSYMS).map(([name, sym]) => [sym, name])) as Record<number, string>

export class InputMethod {
  static readonly RETRY_MS = 2000
  readonly #open: () => NativeIme
  #ime: NativeIme | null = null
  #failedAt = -Infinity
  #focused = false
  #sent: TextInputState | null = null

  constructor(open: () => NativeIme) {
    this.#open = open
  }

  static area(place: (rect: TextInputState['rect']) => void): InputMethod {
    const none = () => undefined
    return new InputMethod(() => ({
      backend: 'host',
      focusIn: none,
      focusOut: none,
      reset: none,
      setCursorRect: (x, y, width, height) => place({ x, y, width, height }),
      setSurroundingText: none,
      setCapability: none,
      processKey: () => false,
      selectCandidate: none,
      prevPage: none,
      nextPage: none,
      close: none,
    }))
  }

  focus(state: TextInputState): void {
    this.#guard((ime) => {
      const sent = this.#focused ? this.#sent : null
      if (sent?.password !== state.password) ime.setCapability({ preedit: true, surroundingText: true, password: state.password, clientPanel: true })
      if (!this.#focused) {
        ime.focusIn()
        this.#focused = true
      }
      if (sent?.text !== state.text || sent.cursor !== state.cursor || sent.anchor !== state.anchor) ime.setSurroundingText(state.text, state.cursor, state.anchor)
      const rect = state.rect, was = sent?.rect
      if (!was || was.x !== rect.x || was.y !== rect.y || was.width !== rect.width || was.height !== rect.height) ime.setCursorRect(rect.x, rect.y, rect.width, rect.height)
      this.#sent = state
    })
  }

  blur(): void {
    if (!this.#focused) return
    this.#focused = false
    this.#sent = null
    if (this.#ime) this.#guard((ime) => ime.focusOut())
  }

  key(key: string, keycode: number, modifiers: Modifiers | undefined, release: boolean): boolean {
    if (!this.#focused) return false
    return this.#guard((ime) => ime.processKey(InputMethod.keysym(key), keycode, InputMethod.state(modifiers), release)) ?? false
  }

  #native(): NativeIme | null {
    if (this.#ime || hostTime.now() - this.#failedAt < InputMethod.RETRY_MS) return this.#ime
    try {
      this.#ime = this.#open()
    } catch (error) {
      if (this.#failedAt === -Infinity) Log.write(Level.Warn, 'acde_host', `input method unavailable (trying again): ${(error as Error).message}`)
      this.#failedAt = hostTime.now()
    }
    return this.#ime
  }

  #guard<T>(call: (ime: NativeIme) => T): T | undefined {
    const ime = this.#native()
    if (!ime) return undefined
    try {
      return call(ime)
    } catch (error) {
      this.#ime = null
      this.#focused = false
      this.#sent = null
      this.#failedAt = hostTime.now()
      Log.write(Level.Warn, 'acde_host', `input method dropped: ${(error as Error).message}`)
      return undefined
    }
  }

  select(index: number): void {
    if (this.#focused) this.#guard((ime) => ime.selectCandidate(index))
  }

  page(next: boolean): void {
    if (this.#focused) this.#guard((ime) => (next ? ime.nextPage() : ime.prevPage()))
  }

  close(): void {
    try {
      this.blur()
      this.#ime?.close()
      this.#ime = null
    } catch (error) {
      Log.write(Level.Warn, 'acde_host', `closing the input method: ${(error as Error).message}`)
    }
  }

  static keysym(key: string): number {
    const named = KEYSYMS[key]
    if (named) return named
    const code = key.codePointAt(0) ?? 0
    return key.length > 0 && [...key].length === 1 ? (code < 0x100 ? code : 0x0100_0000 + code) : 0
  }

  static key(keysym: number): string {
    if (NAMES[keysym]) return NAMES[keysym]
    if (keysym >= 0x0100_0000) return String.fromCodePoint(keysym - 0x0100_0000)
    return keysym >= 0x20 && keysym < 0x100 ? String.fromCharCode(keysym) : 'Unidentified'
  }

  static state(modifiers: Modifiers | undefined): number {
    return (modifiers?.shift ? 1 : 0) | (modifiers?.ctrl ? 4 : 0) | (modifiers?.alt ? 8 : 0) | (modifiers?.meta ? 64 : 0)
  }

  static modifiers(state: number): Modifiers {
    return { shift: !!(state & 1), ctrl: !!(state & 4), alt: !!(state & 8), meta: !!(state & 64) }
  }
}
