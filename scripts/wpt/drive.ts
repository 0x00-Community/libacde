import type { Acde } from 'acdev/driver'

type InputEvent =
  | { type: 'move'; x: number; y: number }
  | { type: 'down' | 'up'; x: number; y: number; button?: 'left' | 'right' | 'middle' }
  | { type: 'wheel'; x: number; y: number; dx?: number; dy?: number }
  | { type: 'keydown' | 'keyup'; key: string }
  | { type: 'text'; text: string }
  | { type: 'wait'; ms: number }

export type DriverRequest =
  | { id: number; kind: 'click'; x: number; y: number }
  | { id: number; kind: 'keys'; keys: string }
  | { id: number; kind: 'actions'; actions: Source[] }

type Action = { type: string; duration?: number; x?: number; y?: number; origin?: string; button?: number; value?: string; deltaX?: number; deltaY?: number }
type Source = { type: 'pointer' | 'key' | 'wheel' | 'none'; id: string; parameters?: { pointerType?: string }; actions: Action[] }

const SPECIAL: Record<string, string> = {
  '': 'Backspace', '': 'Tab', '': 'Enter', '': 'Enter', '': 'ShiftLeft', '': 'ControlLeft',
  '': 'AltLeft', '': 'Escape', '': 'Space', '': 'PageUp', '': 'PageDown', '': 'End',
  '': 'Home', '': 'ArrowLeft', '': 'ArrowUp', '': 'ArrowRight', '': 'ArrowDown', '': 'Insert',
  '': 'Delete', '': ';', '': '=', '': 'NumpadMultiply', '': 'NumpadAdd', '': 'NumpadSubtract',
  '': 'NumpadDecimal', '': 'NumpadDivide', '': 'MetaLeft', '': 'ShiftRight', '': 'ControlRight',
  '': 'AltRight', '': 'MetaRight',
}
for (let i = 0; i < 10; i++) SPECIAL[String.fromCharCode(0xe01a + i)] = `Numpad${i}`
for (let i = 0; i < 12; i++) SPECIAL[String.fromCharCode(0xe031 + i)] = `F${i + 1}`
const MODIFIERS = new Set(['ShiftLeft', 'ShiftRight', 'ControlLeft', 'ControlRight', 'AltLeft', 'AltRight', 'MetaLeft', 'MetaRight'])
const SHIFTED: Record<string, string> = {
  '!': '1', '@': '2', '#': '3', $: '4', '%': '5', '^': '6', '&': '7', '*': '8', '(': '9', ')': '0',
  _: '-', '+': '=', '{': '[', '}': ']', '|': '\\', ':': ';', '"': "'", '<': ',', '>': '.', '?': '/', '~': '`',
}
const PLAIN = new Set([..."abcdefghijklmnopqrstuvwxyz0123456789-=[];'`\\,./"])

function keyOf(char: string): { key: string; shift: boolean } | { text: string } {
  const special = SPECIAL[char]
  if (special) return { key: special, shift: false }
  if (char === ' ') return { key: 'Space', shift: false }
  if (char === '\n') return { key: 'Enter', shift: false }
  if (PLAIN.has(char)) return { key: char, shift: false }
  if (/^[A-Z]$/.test(char)) return { key: char.toLowerCase(), shift: true }
  if (SHIFTED[char]) return { key: SHIFTED[char]!, shift: true }
  return { text: char }
}

function keys(text: string): InputEvent[] {
  const events: InputEvent[] = []
  const held: string[] = []
  for (const char of text) {
    if (char === '') {
      for (const k of held.splice(0).reverse()) events.push({ type: 'keyup', key: k })
      continue
    }
    const k = keyOf(char)
    if ('text' in k) {
      events.push({ type: 'text', text: k.text })
      continue
    }
    if (MODIFIERS.has(k.key)) {
      const at = held.indexOf(k.key)
      if (at >= 0) {
        held.splice(at, 1)
        events.push({ type: 'keyup', key: k.key })
      } else {
        held.push(k.key)
        events.push({ type: 'keydown', key: k.key })
      }
      continue
    }
    const shift = k.shift && !held.some((h) => h.startsWith('Shift'))
    if (shift) events.push({ type: 'keydown', key: 'ShiftLeft' })
    events.push({ type: 'keydown', key: k.key }, { type: 'keyup', key: k.key })
    if (shift) events.push({ type: 'keyup', key: 'ShiftLeft' })
  }
  for (const k of held.reverse()) events.push({ type: 'keyup', key: k })
  return events
}

const BUTTON = ['left', 'middle', 'right'] as const

function actions(sources: Source[]): InputEvent[] {
  const events: InputEvent[] = []
  const pointer = { x: 0, y: 0 }
  for (const s of sources)
    if (s.type === 'pointer' && s.parameters?.pointerType && s.parameters.pointerType !== 'mouse')
      throw new Error(`pointerType ${s.parameters.pointerType} is not supported (mouse only)`)
  const ticks = Math.max(0, ...sources.map((s) => s.actions.length))
  for (let i = 0; i < ticks; i++) {
    let wait = 0
    for (const s of sources) {
      const a = s.actions[i]
      if (!a) continue
      wait = Math.max(wait, a.duration ?? 0)
      switch (a.type) {
        case 'pause':
          break
        case 'pointerMove': {
          const x = a.origin === 'pointer' ? pointer.x + (a.x ?? 0) : (a.x ?? 0)
          const y = a.origin === 'pointer' ? pointer.y + (a.y ?? 0) : (a.y ?? 0)
          pointer.x = Math.round(x)
          pointer.y = Math.round(y)
          events.push({ type: 'move', x: pointer.x, y: pointer.y })
          break
        }
        case 'pointerDown':
        case 'pointerUp': {
          const button = BUTTON[a.button ?? 0]
          if (!button) throw new Error(`pointer button ${a.button} is not supported`)
          events.push({ type: a.type === 'pointerDown' ? 'down' : 'up', x: pointer.x, y: pointer.y, button })
          break
        }
        case 'keyDown':
        case 'keyUp': {
          const k = keyOf(a.value ?? '')
          if ('text' in k) {
            if (a.type === 'keyDown') events.push({ type: 'text', text: k.text })
          } else {
            if (k.shift && a.type === 'keyDown') events.push({ type: 'keydown', key: 'ShiftLeft' })
            events.push({ type: a.type === 'keyDown' ? 'keydown' : 'keyup', key: k.key })
            if (k.shift && a.type === 'keyDown') events.push({ type: 'keyup', key: 'ShiftLeft' })
          }
          break
        }
        case 'scroll':
          events.push({ type: 'wheel', x: Math.round(a.x ?? 0), y: Math.round(a.y ?? 0), dx: a.deltaX ?? 0, dy: a.deltaY ?? 0 })
          break
        default:
          throw new Error(`action ${a.type} is not supported`)
      }
    }
    for (let left = Math.min(wait, 5000); left > 0; left -= 1000) events.push({ type: 'wait', ms: Math.min(left, 1000) })
  }
  return events
}

export async function perform(acde: Acde, web: string, req: DriverRequest): Promise<string | null> {
  let events: InputEvent[]
  try {
    events =
      req.kind === 'click'
        ? [{ type: 'move', x: Math.round(req.x), y: Math.round(req.y) }, { type: 'down', x: Math.round(req.x), y: Math.round(req.y) }, { type: 'up', x: Math.round(req.x), y: Math.round(req.y) }]
        : req.kind === 'keys'
          ? keys(req.keys)
          : actions(req.actions)
  } catch (e) {
    return String((e as Error)?.message ?? e)
  }
  if (!events.length) return null
  try {
    const result = await acde.input(web, events as never)
    return result.ok ? null : `input failed: ${result.text}`
  } catch (e) {
    return `input failed: ${String((e as Error)?.message ?? e).slice(0, 300)}`
  }
}
