import type { Modifiers, RawShellEvent, ShellEvent } from './native'

const KEYS: Record<number, readonly [string, string?]> = {
  1: ['Escape', 'Escape'], 12: ['Minus'], 13: ['Equal'], 14: ['Backspace', 'Backspace'], 15: ['Tab', 'Tab'],
  26: ['BracketLeft'], 27: ['BracketRight'], 28: ['Enter', 'Enter'], 29: ['ControlLeft', 'Control'], 39: ['Semicolon'],
  40: ['Quote'], 41: ['Backquote'], 42: ['ShiftLeft', 'Shift'], 43: ['Backslash'], 51: ['Comma'], 52: ['Period'],
  53: ['Slash'], 54: ['ShiftRight', 'Shift'], 55: ['NumpadMultiply', '*'], 56: ['AltLeft', 'Alt'], 57: ['Space'],
  58: ['CapsLock', 'CapsLock'], 87: ['F11', 'F11'], 88: ['F12', 'F12'], 96: ['NumpadEnter', 'Enter'],
  97: ['ControlRight', 'Control'], 100: ['AltRight', 'Alt'], 102: ['Home', 'Home'], 103: ['ArrowUp', 'ArrowUp'],
  104: ['PageUp', 'PageUp'], 105: ['ArrowLeft', 'ArrowLeft'], 106: ['ArrowRight', 'ArrowRight'], 107: ['End', 'End'],
  108: ['ArrowDown', 'ArrowDown'], 109: ['PageDown', 'PageDown'], 110: ['Insert', 'Insert'], 111: ['Delete', 'Delete'],
  99: ['PrintScreen', 'PrintScreen'], 125: ['MetaLeft', 'Meta'], 126: ['MetaRight', 'Meta'], 127: ['ContextMenu', 'ContextMenu'],
  71: ['Numpad7', '7'], 72: ['Numpad8', '8'], 73: ['Numpad9', '9'], 74: ['NumpadSubtract', '-'], 75: ['Numpad4', '4'], 76: ['Numpad5', '5'],
  77: ['Numpad6', '6'], 78: ['NumpadAdd', '+'], 79: ['Numpad1', '1'], 80: ['Numpad2', '2'], 81: ['Numpad3', '3'], 82: ['Numpad0', '0'],
  83: ['NumpadDecimal', '.'], 98: ['NumpadDivide', '/'],
}
for (const [row, start] of [['qwertyuiop', 16], ['asdfghjkl', 30], ['zxcvbnm', 44]] as const) {
  ;[...row].forEach((c, i) => (KEYS[start + i] = [`Key${c.toUpperCase()}`]))
}
;[...'1234567890'].forEach((c, i) => (KEYS[2 + i] = [`Digit${c}`]))
for (let i = 0; i < 10; i++) KEYS[59 + i] = [`F${i + 1}`, `F${i + 1}`]

const ALIASES: Record<string, number> = {
  ctrl: 29, control: 29, shift: 42, alt: 56, option: 56, super: 125, meta: 125, win: 125, cmd: 125, command: 125, enter: 28, return: 28, esc: 1, escape: 1, tab: 15, space: 57,
  backspace: 14, delete: 111, del: 111, up: 103, down: 108, left: 105, right: 106, home: 102, end: 107, pageup: 104, pagedown: 109,
  insert: 110, menu: 127, '-': 12, '=': 13, '[': 26, ']': 27, ';': 39, "'": 40, '`': 41, '\\': 43, ',': 51, '.': 52, '/': 53,
}

const SHIFTED: Record<string, string> = {
  '!': '1', '@': '2', '#': '3', $: '4', '%': '5', '^': '6', '&': '7', '*': '8', '(': '9', ')': '0',
  _: '-', '+': '=', '{': '[', '}': ']', '|': '\\', ':': ';', '"': "'", '<': ',', '>': '.', '?': '/', '~': '`',
}
const NAMED: Record<string, string> = { plus: '+', multiply: '*', asterisk: '*', star: '*', minus: '-', divide: '/', period: '.', comma: ',' }

export function chordCodes(chord: string): number[] | null {
  const parts = chord === '+' ? ['+'] : chord.endsWith('++') ? [...chord.slice(0, -2).split('+'), '+'] : chord.split('+')
  if (parts.some((p) => !p)) return null
  const codes: number[] = []
  for (const part of parts) {
    const name = NAMED[part.toLowerCase()] ?? part
    const base = SHIFTED[name]
    const code = keyCode(base ?? name)
    if (code === null) return null
    if (base !== undefined || (parts.length === 1 && /^[A-Z]$/.test(name))) codes.push(keyCode('shift')!)
    codes.push(code)
  }
  return [...new Set(codes)]
}

export function keyCode(name: string): number | null {
  const lower = name.toLowerCase()
  if (lower in ALIASES) return ALIASES[lower]!
  for (const [code, [key]] of Object.entries(KEYS)) {
    const k = key.toLowerCase()
    if (k === lower || k === `key${lower}` || k === `digit${lower}`) return Number(code)
  }
  return null
}

const BUTTONS: Record<number, number> = { 272: 0, 273: 2, 274: 1, 275: 3, 276: 4 }
const MODIFIER: Record<string, keyof Modifiers> = { Shift: 'shift', Control: 'ctrl', Alt: 'alt', Meta: 'meta' }

export class ShellInput {
  readonly #hitTest: (x: number, y: number) => number | null
  readonly #mods = { shift: false, ctrl: false, alt: false, meta: false }
  #scale: number

  constructor(hitTest: (x: number, y: number) => number | null, scale: number) {
    this.#hitTest = hitTest
    this.#scale = scale
  }

  normalize(raw: RawShellEvent): ShellEvent | null {
    switch (raw.type) {
      case 'frame': return { type: 'frame', time: raw.time ?? performance.now() }
      case 'close': return { type: 'close' }
      case 'panelmove': return { type: 'panelmove', id: raw.code ?? 0, x: raw.x ?? 0, y: raw.y ?? 0, width: raw.width ?? 0, height: raw.height ?? 0 }
      case 'exit': return { type: 'exit' }
      case 'resize': return { type: 'resize', width: raw.width ?? 0, height: raw.height ?? 0, scale: raw.scale ?? this.#scale }
      case 'dndenter': return { type: 'dndenter', target: this.#hitTest(raw.x ?? 0, raw.y ?? 0), x: raw.x ?? 0, y: raw.y ?? 0, mime: raw.mime ?? [] }
      case 'dndmove': return { type: 'dndmove', target: this.#hitTest(raw.x ?? 0, raw.y ?? 0), x: raw.x ?? 0, y: raw.y ?? 0 }
      case 'dnddrop': {
        const [x, y] = [raw.x ?? 0, raw.y ?? 0]
        return { type: 'dnddrop', target: this.#hitTest(x, y), x, y, data: raw.data ?? {}, files: raw.files ?? [] }
      }
      case 'dndleave': return { type: 'dndleave' }
      case 'dragend': return { type: 'dragend', dropped: raw.button === 1 }
      case 'pointerleave': return { type: 'pointerleave', modifiers: { ...this.#mods } }
      case 'pointermove':
      case 'pointerdown':
      case 'pointerup':
      case 'wheel': {
        const x = raw.x ?? 0, y = raw.y ?? 0
        const target = this.#hitTest(x, y)
        const modifiers = { ...this.#mods }
        if (raw.type === 'wheel') return { type: 'wheel', target, x, y, deltaX: raw.deltaX ?? 0, deltaY: raw.deltaY ?? 0, modifiers }
        return { type: raw.type, target, x, y, button: BUTTONS[raw.button ?? 272] ?? 0, modifiers }
      }
      case 'keydown':
      case 'keyup': {
        const [code, named] = KEYS[raw.code ?? -1] ?? [`Unidentified${raw.code}`]
        const mod = named ? MODIFIER[named] : undefined
        if (mod) this.#mods[mod] = raw.type === 'keydown'
        const char = raw.key && raw.key >= ' ' && raw.key !== '\x7f' ? raw.key : null
        const key = named && named.length > 1 ? named : (char ?? named ?? ShellInput.#fallback(code, this.#mods.shift))
        return { type: raw.type, key, code, keycode: raw.code ?? 0, modifiers: { ...this.#mods } }
      }
      default: return null
    }
  }

  set scale(scale: number) {
    this.#scale = scale
  }

  static #fallback(code: string, shift: boolean): string {
    if (code.startsWith('Key')) return shift ? code.slice(3) : code.slice(3).toLowerCase()
    if (code.startsWith('Digit')) return code.slice(5)
    return code === 'Space' ? ' ' : 'Unidentified'
  }
}
