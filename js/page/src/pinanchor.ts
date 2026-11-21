import type { PinAnchor, WindowEdge } from '@acde/api'

export type PinAlign = 'start' | 'center' | 'end'
export type PinAdjust = 'flip' | 'slide'

export interface Anchor {
  readonly window: number
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
  readonly edge: WindowEdge
  readonly align: PinAlign
  readonly adjust: readonly PinAdjust[]
}

export interface AnchorWire extends Anchor {
  readonly visible: readonly [number, number, number, number]
}

const EDGES: readonly WindowEdge[] = ['bottom', 'top', 'right', 'left']
const ALIGNS: readonly PinAlign[] = ['start', 'center', 'end']
const ADJUSTS: readonly PinAdjust[] = ['flip', 'slide']

function number(value: unknown, name: string, least: number): number {
  if (typeof value !== 'number' || !Number.isFinite(value) || value < least) {
    throw new TypeError(`pin: anchor.${name} is a number${least === 0 ? ' of 0 or more' : ''}, not ${String(value)}`)
  }
  return value
}

function oneOf<T extends string>(value: unknown, name: string, allowed: readonly T[], fallback: T): T {
  if (value === undefined) return fallback
  if (allowed.includes(value as T)) return value as T
  throw new TypeError(`pin: anchor.${name} is one of ${allowed.join(', ')}, not ${String(value)}`)
}

export function anchorOf(given: PinAnchor): Anchor {
  if (given === null || typeof given !== 'object') throw new TypeError('pin: anchor is {window, x, y, width?, height?, edge?, align?, adjust?}')
  if (!Number.isInteger(given.window) || given.window < 0) throw new TypeError(`pin: anchor.window is a window number, not ${String(given.window)}`)
  const adjust = given.adjust ?? ADJUSTS
  if (!Array.isArray(adjust) || adjust.some((a) => !ADJUSTS.includes(a))) throw new TypeError(`pin: anchor.adjust lists ${ADJUSTS.join(' and ')}`)
  return {
    window: given.window,
    x: number(given.x, 'x', -Infinity),
    y: number(given.y, 'y', -Infinity),
    width: given.width === undefined ? 0 : number(given.width, 'width', 0),
    height: given.height === undefined ? 0 : number(given.height, 'height', 0),
    edge: oneOf(given.edge, 'edge', EDGES, 'bottom'),
    align: oneOf(given.align, 'align', ALIGNS, 'start'),
    adjust: [...new Set(adjust)],
  }
}
