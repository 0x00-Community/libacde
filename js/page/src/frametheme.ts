import type { FrameTheme } from '@dopame/acde-api'

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
