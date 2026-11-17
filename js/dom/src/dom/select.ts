import { Event, fire } from '../core/event'
import { host } from '../core/host'
import type { Element } from './element'
import { disabledControl, HTMLOptionElement, HTMLSelectElement } from './forms'
import { dom, ID, wrap, inner } from './realm'

export interface SelectModifiers {
  readonly shift?: boolean
  readonly ctrl?: boolean
  readonly alt?: boolean
  readonly meta?: boolean
}

const TYPEAHEAD_MS = 1000
const CLOSED_PAGE = 10
const OUTSIDE = -2

function options(select: HTMLSelectElement): HTMLOptionElement[] {
  return [...dom.options(select[ID])].map((id) => wrap<HTMLOptionElement>(id)!)
}

function enabled(option: HTMLOptionElement | undefined): option is HTMLOptionElement {
  if (!option || option.disabled) return false
  const parent = option.parentElement
  return !(parent && dom.localName(parent[ID]) === 'optgroup' && dom.hasAttribute(parent[ID], 'disabled'))
}

function listBox(select: HTMLSelectElement): boolean {
  return select.multiple || select.size > 1
}

function notify(select: HTMLSelectElement): void {
  fire(select, new Event('input', { bubbles: true, composed: true }))
  fire(select, new Event('change', { bubbles: true }))
}

function step(list: HTMLOptionElement[], from: number, by: number): number {
  let found = -1
  const forward = by > 0
  let left = Math.abs(by)
  for (let i = from + (forward ? 1 : -1); i >= 0 && i < list.length && left > 0; i += forward ? 1 : -1) {
    if (!enabled(list[i])) continue
    found = i
    left--
  }
  return found
}

function edge(list: HTMLOptionElement[], last: boolean): number {
  const order = list.map((_, i) => i)
  if (last) order.reverse()
  return order.find((i) => enabled(list[i])) ?? -1
}

export class SelectInput {
  #press: { inPicker: boolean; moved: boolean } | null = null
  #typed = { text: '', at: 0 }
  #anchor = new WeakMap<HTMLSelectElement, number>()
  #cursor = new WeakMap<HTMLSelectElement, number>()

  get picker(): HTMLSelectElement | null {
    const id = dom.pickerState()[0]!
    if (id < 0) return null
    return wrap<HTMLSelectElement>(id) ?? null
  }

  close(): void {
    this.#press = null
    if (this.picker) dom.closePicker()
  }

  pointer(type: 'pointermove' | 'pointerdown' | 'pointerup', x: number, y: number): boolean {
    const select = this.picker
    if (!select) {
      this.#press = null
      return false
    }
    const hit = dom.pickerOptionAt(x, y)
    if (type === 'pointermove') {
      if (hit >= 0 && enabled(options(select)[hit])) dom.setPickerActive(hit)
      if (this.#press && hit !== OUTSIDE) this.#press.moved = true
      return true
    }
    if (type === 'pointerdown') {
      if (hit === OUTSIDE) {
        this.close()
        return true
      }
      this.#press = { inPicker: true, moved: false }
      return true
    }
    const press = this.#press
    this.#press = null
    if (hit === OUTSIDE) return !press || press.inPicker
    if (press && (press.inPicker || press.moved) && enabled(options(select)[hit])) this.#choose(select, hit)
    return true
  }

  wheel(x: number, y: number): boolean {
    if (!this.picker) return false
    if (dom.pickerOptionAt(x, y) !== OUTSIDE) return true
    this.close()
    return false
  }

  pressed(hit: Element, x: number, y: number, modifiers: SelectModifiers): void {
    const target = hit instanceof HTMLOptionElement ? hit.closest('select') : hit
    if (!(target instanceof HTMLSelectElement) || disabledControl(target)) return
    if (!listBox(target)) {
      if (this.picker === target) return this.close()
      if (dom.openPicker(target[ID])) this.#press = { inPicker: false, moved: false }
      return
    }
    const index = dom.listBoxOptionAt(target[ID], x, y)
    const list = options(target)
    if (index < 0 || !enabled(list[index])) return
    this.#pickInList(target, list, index, modifiers, true)
  }

  key(key: string, modifiers: SelectModifiers): boolean {
    const select = this.picker
    if (!select) return false
    const list = options(select)
    const active = dom.pickerState()[1]!
    const page = Math.max(1, dom.pickerState()[2]!)
    const move = (to: number) => {
      if (to >= 0) dom.setPickerActive(to)
    }
    switch (key) {
      case 'ArrowDown':
        if (modifiers.alt) return this.#commit(select, active)
        move(step(list, active, 1))
        return true
      case 'ArrowUp':
        if (modifiers.alt) return this.#commit(select, active)
        move(step(list, active < 0 ? list.length : active, -1))
        return true
      case 'PageDown':
        move(step(list, active, page) < 0 ? edge(list, true) : step(list, active, page))
        return true
      case 'PageUp':
        move(step(list, active < 0 ? list.length : active, -page) < 0 ? edge(list, false) : step(list, active, -page))
        return true
      case 'Home':
        move(edge(list, false))
        return true
      case 'End':
        move(edge(list, true))
        return true
      case 'Enter':
      case 'F4':
        return this.#commit(select, active)
      case 'Escape':
        this.close()
        return true
      case 'Tab':
        this.#commit(select, active)
        return false
      case ' ':
        if (this.#typing()) break
        return this.#commit(select, active)
    }
    if (key.length === 1 && !modifiers.ctrl && !modifiers.meta) {
      move(this.#typeahead(list, active, key))
      return true
    }
    return true
  }

  keyDefault(target: Element, key: string, modifiers: SelectModifiers): boolean {
    if (!(target instanceof HTMLSelectElement) || disabledControl(target)) return false
    const list = options(target)
    const current = dom.selectedIndex(target[ID])
    if (!listBox(target)) {
      if ((modifiers.alt && (key === 'ArrowDown' || key === 'ArrowUp')) || key === 'F4' || (key === ' ' && !this.#typing())) {
        if (dom.openPicker(target[ID])) this.#press = null
        return true
      }
      const to = this.#target(list, current, key, modifiers, CLOSED_PAGE)
      if (to === null) return false
      if (to >= 0 && to !== current) {
        list[to]!.selected = true
        notify(target)
      }
      return true
    }
    const cursor = target.multiple ? (this.#cursor.get(target) ?? current) : current
    if (key === ' ' && (modifiers.ctrl || modifiers.meta) && target.multiple) {
      if (cursor >= 0 && enabled(list[cursor])) this.#pickInList(target, list, cursor, { ctrl: true }, true)
      return true
    }
    const rows = Math.max(1, target.size || 4)
    const to = this.#target(list, cursor, key, modifiers, rows)
    if (to === null) return false
    if (to < 0) return true
    if (target.multiple && (modifiers.ctrl || modifiers.meta) && !modifiers.shift) this.#cursor.set(target, to)
    else this.#pickInList(target, list, to, { shift: modifiers.shift }, false)
    return true
  }

  #target(list: HTMLOptionElement[], current: number, key: string, modifiers: SelectModifiers, page: number): number | null {
    switch (key) {
      case 'ArrowDown':
      case 'ArrowRight':
        return step(list, current, 1)
      case 'ArrowUp':
      case 'ArrowLeft':
        return step(list, current < 0 ? list.length : current, -1)
      case 'PageDown':
        return step(list, current, page) < 0 ? edge(list, true) : step(list, current, page)
      case 'PageUp':
        return step(list, current < 0 ? list.length : current, -page) < 0 ? edge(list, false) : step(list, current, -page)
      case 'Home':
        return edge(list, false)
      case 'End':
        return edge(list, true)
    }
    if (key.length === 1 && !modifiers.ctrl && !modifiers.meta && !modifiers.alt) return this.#typeahead(list, current, key)
    return null
  }

  #typing(): boolean {
    return this.#typed.text !== '' && host.now() - this.#typed.at < TYPEAHEAD_MS
  }

  #typeahead(list: HTMLOptionElement[], current: number, char: string): number {
    const now = host.now()
    const text = now - this.#typed.at < TYPEAHEAD_MS ? this.#typed.text + char.toLowerCase() : char.toLowerCase()
    this.#typed = { text, at: now }
    const repeated = [...text].every((c) => c === text[0])
    const prefix = repeated ? text[0]! : text
    const start = repeated || text.length === 1 ? current + 1 : Math.max(current, 0)
    for (let k = 0; k < list.length; k++) {
      const i = (start + k) % list.length
      const option = list[i]!
      if (enabled(option) && option.label.trim().toLowerCase().startsWith(prefix)) return i
    }
    return -1
  }

  #commit(select: HTMLSelectElement, active: number): boolean {
    if (active >= 0 && enabled(options(select)[active])) this.#choose(select, active)
    else this.close()
    return true
  }

  #choose(select: HTMLSelectElement, index: number): void {
    this.close()
    const option = options(select)[index]
    if (!option || option.selected) return
    option.selected = true
    notify(select)
  }

  #pickInList(select: HTMLSelectElement, list: HTMLOptionElement[], index: number, modifiers: SelectModifiers, pointer: boolean): void {
    const before = list.map((o) => o.selected)
    if (!select.multiple) {
      list[index]!.selected = true
    } else if (modifiers.shift) {
      const anchor = this.#anchor.get(select) ?? index
      const [lo, hi] = anchor < index ? [anchor, index] : [index, anchor]
      const keep = modifiers.ctrl || modifiers.meta
      list.forEach((option, i) => {
        const inside = i >= lo && i <= hi && enabled(option)
        if (inside) option.selected = true
        else if (!keep) option.selected = false
      })
    } else if (pointer && (modifiers.ctrl || modifiers.meta)) {
      list[index]!.selected = !list[index]!.selected
      this.#anchor.set(select, index)
    } else {
      list.forEach((option, i) => (option.selected = i === index))
      this.#anchor.set(select, index)
    }
    if (!select.multiple) this.#anchor.set(select, index)
    this.#cursor.set(select, index)
    if (list.some((o, i) => o.selected !== before[i])) notify(select)
  }
}
