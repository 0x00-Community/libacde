import type { FrameTheme } from '@acde/api'

export interface FrameThemeWire {
  readonly colors: readonly number[]
  readonly style: number
  readonly side: number
  readonly align: number
  readonly ignored: readonly string[]
}

export class FrameThemes {
  static readonly KEYS = ['active', 'inactive', 'light', 'buttonStyle', 'buttonSide', 'titleAlign'] as const
  static readonly STATES = ['active', 'inactive'] as const
  static readonly COLORS = ['background', 'title', 'line', 'buttons'] as const
  static readonly #CHOICES = {
    buttonStyle: ['dots', 'glyphs'],
    buttonSide: ['left', 'right'],
    titleAlign: ['center', 'start'],
  } as const
  static readonly #HEX = /^#([0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/i
  static readonly #RGB = /^rgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*(?:,\s*(\d*\.?\d+)\s*)?\)$/i

  static color(text: unknown): number | null {
    if (typeof text !== 'string') return null
    const hex = FrameThemes.#HEX.exec(text.trim())
    if (hex) {
      const digits = hex[1]!.length <= 4 ? [...hex[1]!].map((d) => d + d).join('') : hex[1]!
      return Number.parseInt(digits.length === 6 ? `${digits}ff` : digits, 16)
    }
    const rgb = FrameThemes.#RGB.exec(text.trim())
    if (!rgb) return null
    const [r, g, b] = [rgb[1], rgb[2], rgb[3]].map(Number) as [number, number, number]
    const a = rgb[4] === undefined ? 1 : Number(rgb[4])
    if ([r, g, b].some((c) => c > 255) || a > 1) return null
    return ((r << 24) | (g << 16) | (b << 8) | Math.round(a * 255)) >>> 0
  }

  static parse(raw: unknown): FrameThemeWire | string {
    if (raw === null || raw === undefined) return { colors: Array(16).fill(-1), style: 0, side: 0, align: 0, ignored: [] }
    if (typeof raw !== 'object' || Array.isArray(raw)) return 'a frame theme is an object (or null for the runtime look)'
    const theme = raw as Record<string, unknown>
    const ignored = Object.keys(theme).filter((key) => !(FrameThemes.KEYS as readonly string[]).includes(key))
    const colors: number[] = []
    const light = theme['light']
    const notObject = typeof light !== 'object' || light === null || Array.isArray(light)
    if (light !== undefined && notObject) return 'light is an object with active and inactive colors'
    const lightStates = (light ?? {}) as Record<string, unknown>
    ignored.push(...Object.keys(lightStates).filter((key) => !(FrameThemes.STATES as readonly string[]).includes(key)).map((key) => `light.${key}`))
    for (const [prefix, states] of [['', theme], ['light.', lightStates]] as const) {
      for (const state of FrameThemes.STATES) {
        const found = FrameThemes.#colors(`${prefix}${state}`, states[state], ignored)
        if (typeof found === 'string') return found
        colors.push(...found)
      }
    }
    const codes: number[] = []
    for (const key of ['buttonStyle', 'buttonSide', 'titleAlign'] as const) {
      const value = theme[key]
      const choices: readonly unknown[] = FrameThemes.#CHOICES[key]
      if (value !== undefined && !choices.includes(value)) return `${key} is one of ${choices.join(', ')}`
      codes.push(value === undefined ? 0 : choices.indexOf(value) + 1)
    }
    const [style, side, align] = codes as [number, number, number]
    return { colors, style, side, align, ignored }
  }

  static #colors(state: string, raw: unknown, ignored: string[]): number[] | string {
    if (raw === undefined) return Array(4).fill(-1)
    if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return `${state} is an object of colors (${FrameThemes.COLORS.join(', ')})`
    const colors = raw as Record<string, unknown>
    ignored.push(...Object.keys(colors).filter((key) => !(FrameThemes.COLORS as readonly string[]).includes(key)).map((key) => `${state}.${key}`))
    const out: number[] = []
    for (const key of FrameThemes.COLORS) {
      if (colors[key] === undefined) {
        out.push(-1)
        continue
      }
      const color = FrameThemes.color(colors[key])
      if (color === null) return `${state}.${key} is not a color (#rgb, #rgba, #rrggbb, #rrggbbaa, rgb() or rgba()): ${JSON.stringify(colors[key])}`
      out.push(color)
    }
    return out
  }
}

export interface WindowLookWire {
  readonly numbers: readonly number[]
  readonly ignored: readonly string[]
}

type Fields = Record<string, unknown>

const LOOK_KEYS = ['corners', 'shadows'] as const
const LOOK_CORNERS = ['radius', 'shape'] as const
const LOOK_STATES = ['focused', 'unfocused'] as const
const LOOK_LAYER = ['x', 'y', 'blur', 'spread', 'color'] as const
const LOOK_MOST_LAYERS = 4
const LOOK_MOST_RADIUS = 256
const LOOK_ROUND = 1
const LOOK_SHAPES: ReadonlyMap<string, number> = new Map([
  ['round', LOOK_ROUND],
  ['squircle', 2],
])
const LOOK_SUPERELLIPSE = /^superellipse\(\s*(-?(?:\d+\.?\d*|\.\d+))\s*\)$/
const LOOK_K = [1, 3] as const

export function cornerShape(what: string, raw: unknown): number | string {
  if (raw === undefined) return LOOK_ROUND
  const named = typeof raw === 'string' ? LOOK_SHAPES.get(raw.trim()) : undefined
  if (named !== undefined) return named
  const k = typeof raw === 'string' ? LOOK_SUPERELLIPSE.exec(raw.trim())?.[1] : undefined
  const value = k === undefined ? Number.NaN : Number(k)
  if (value >= LOOK_K[0] && value <= LOOK_K[1]) return value
  return `${what} is round, squircle or superellipse(k) with k from ${LOOK_K[0]} to ${LOOK_K[1]}: ${JSON.stringify(raw)}`
}

export class WindowLookReader {
  readonly #ignored: string[] = []

  static parse(raw: unknown): WindowLookWire | string {
    if (raw === null || raw === undefined) return { numbers: [], ignored: [] }
    return new WindowLookReader().#look(raw)
  }

  #look(raw: unknown): WindowLookWire | string {
    const look = this.#object('a window look', raw, LOOK_KEYS, '')
    if (typeof look === 'string') return `${look} (or null for square windows without shadows)`
    const corners = this.#corners(look['corners'])
    if (typeof corners === 'string') return corners
    const shadows = this.#object('shadows', look['shadows'], LOOK_STATES, 'shadows.')
    if (typeof shadows === 'string') return shadows
    const numbers = [...corners]
    for (const state of LOOK_STATES) {
      const layers = this.#layers(`shadows.${state}`, shadows[state])
      if (typeof layers === 'string') return layers
      numbers.push(layers.length / LOOK_LAYER.length, ...layers)
    }
    return { numbers, ignored: this.#ignored }
  }

  #object(what: string, raw: unknown, keys: readonly string[], prefix: string): Fields | string {
    if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return `${what} is an object with ${keys.join(', ')}`
    const fields = raw as Fields
    this.#ignored.push(...Object.keys(fields).filter((key) => !keys.includes(key)).map((key) => `${prefix}${key}`))
    return fields
  }

  #number(what: string, raw: unknown, low = -Infinity, high = Infinity): number | string {
    if (typeof raw !== 'number' || !Number.isFinite(raw) || raw < low || raw > high) {
      const range = high !== Infinity ? `a number from ${low} to ${high}` : low !== -Infinity ? `a number of at least ${low}` : 'a finite number'
      return `${what} is ${range}: ${JSON.stringify(raw)}`
    }
    return raw
  }

  #corners(raw: unknown): number[] | string {
    const corners = this.#object('corners', raw, LOOK_CORNERS, 'corners.')
    if (typeof corners === 'string') return corners
    const radius = this.#number('corners.radius', corners['radius'], 0, LOOK_MOST_RADIUS)
    if (typeof radius === 'string') return radius
    const shape = cornerShape('corners.shape', corners['shape'])
    if (typeof shape === 'string') return shape
    return [radius, shape]
  }

  #layers(what: string, raw: unknown): number[] | string {
    if (!Array.isArray(raw)) return `${what} is an array of shadow layers in CSS order`
    if (raw.length > LOOK_MOST_LAYERS) return `${what} has at most ${LOOK_MOST_LAYERS} layers, not ${raw.length}`
    const numbers: number[] = []
    for (const [index, item] of raw.entries()) {
      const at = `${what}[${index}]`
      const layer = this.#object(at, item, LOOK_LAYER, `${at}.`)
      if (typeof layer === 'string') return layer
      for (const key of ['x', 'y', 'blur', 'spread'] as const) {
        const value = this.#number(`${at}.${key}`, layer[key], key === 'blur' ? 0 : -Infinity)
        if (typeof value === 'string') return value
        numbers.push(value)
      }
      const color = FrameThemes.color(layer['color'])
      if (color === null) return `${at}.color is not a color (#rrggbbaa, #rrggbb, #rgb, #rgba, rgb() or rgba()): ${JSON.stringify(layer['color'])}`
      numbers.push(color)
    }
    return numbers
  }
}
