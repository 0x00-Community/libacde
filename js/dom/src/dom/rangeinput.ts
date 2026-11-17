import { Event, fire } from '../core/event'
import type { Element } from './element'
import { disabledControl, HTMLInputElement } from './forms'
import { rectOf } from './layout'
import { dom, ID } from './realm'

const THUMB = 8

function rangeOf(target: Element): HTMLInputElement | null {
  return target instanceof HTMLInputElement && target.type === 'range' && !disabledControl(target) ? target : null
}

function bounds(input: HTMLInputElement): { min: number; max: number } {
  const number = (name: string, fallback: number) => {
    const value = Number.parseFloat(dom.getAttribute(input[ID], name) ?? '')
    return Number.isFinite(value) ? value : fallback
  }
  const min = number('min', 0)
  return { min, max: Math.max(min, number('max', 100)) }
}

const VALUE = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!

function setTo(input: HTMLInputElement, value: number): boolean {
  const before = VALUE.get!.call(input) as string
  VALUE.set!.call(input, String(value))
  if (VALUE.get!.call(input) === before) return false
  fire(input, new Event('input', { bubbles: true, composed: true }))
  return true
}

export class RangeInput {
  #active: HTMLInputElement | null = null
  #start = ''

  get dragging(): boolean {
    return this.#active !== null
  }

  down(target: Element, x: number): void {
    const input = rangeOf(target)
    if (!input) return
    this.#active = input
    this.#start = input.value
    this.move(x)
  }

  move(x: number): void {
    const input = this.#active
    if (!input) return
    const r = rectOf(input[ID])
    const width = Math.max(1, r[2]! - 2 * THUMB)
    const fraction = Math.min(1, Math.max(0, (x - r[0]! - THUMB) / width))
    const { min, max } = bounds(input)
    setTo(input, min + fraction * (max - min))
  }

  up(): void {
    const input = this.#active
    this.#active = null
    if (input && input.value !== this.#start) fire(input, new Event('change', { bubbles: true }))
  }

  key(target: Element, key: string): boolean {
    const input = rangeOf(target)
    if (!input) return false
    const { min, max } = bounds(input)
    const raw = dom.getAttribute(input[ID], 'step')
    const parsed = Number.parseFloat(raw ?? '')
    const step = raw?.trim().toLowerCase() === 'any' ? (max - min) / 100 : Number.isFinite(parsed) && parsed > 0 ? parsed : 1
    const now = input.valueAsNumber
    const page = Math.max(step, (max - min) / 10)
    const next: Record<string, number> = { ArrowRight: now + step, ArrowUp: now + step, ArrowLeft: now - step, ArrowDown: now - step, PageUp: now + page, PageDown: now - page, Home: min, End: max }
    const value = next[key]
    if (value === undefined) return false
    if (setTo(input, Math.min(max, Math.max(min, value)))) fire(input, new Event('change', { bubbles: true }))
    return true
  }
}
