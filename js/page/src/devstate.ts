import { hostTime } from '@dopame/acde-dom'

interface Field {
  readonly path: string
  readonly value?: string
  readonly checked?: boolean
}

interface Scroll {
  readonly path: string
  readonly top: number
  readonly left: number
}

interface Focus {
  readonly path: string
  readonly start?: number | null
  readonly end?: number | null
}

export interface DevSnapshot {
  readonly x: number
  readonly y: number
  readonly fields: readonly Field[]
  readonly scrolls: readonly Scroll[]
  readonly focus: Focus | null
}

type Editable = HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement

export class DevState {
  static readonly RETRIES = [0, 50, 200, 600, 1500]
  static readonly MAX_SCROLL_SCAN = 3000

  static path(element: Element): string {
    const parts: string[] = []
    let node: Element | null = element
    while (node && node.localName !== 'html') {
      const id = node.getAttribute('id')
      if (id) {
        parts.unshift(`[id="${id.replace(/["\\]/g, '\\$&')}"]`)
        break
      }
      const parent: Element | null = node.parentElement
      if (!parent) break
      parts.unshift(`${node.localName}:nth-child(${[...parent.children].indexOf(node) + 1})`)
      node = parent
    }
    return parts.join(' > ')
  }

  static #ours(element: Element): boolean {
    return !element.closest('acde-frame')
  }

  static capture(win: Window): DevSnapshot {
    const doc = win.document
    const fields: Field[] = []
    for (const element of doc.querySelectorAll<Editable>('input, textarea, select')) {
      if (!DevState.#ours(element)) continue
      const type = (element.getAttribute('type') ?? '').toLowerCase()
      if (type === 'file' || type === 'hidden') continue
      const path = DevState.path(element)
      if (element.localName === 'input' && (type === 'checkbox' || type === 'radio')) fields.push({ path, checked: (element as HTMLInputElement).checked })
      else fields.push({ path, value: element.value })
    }
    const scrolls: Scroll[] = []
    const all = doc.querySelectorAll('body *')
    for (let i = 0; i < all.length && i < DevState.MAX_SCROLL_SCAN; i++) {
      const element = all[i]!
      if ((element.scrollTop || element.scrollLeft) && DevState.#ours(element)) scrolls.push({ path: DevState.path(element), top: element.scrollTop, left: element.scrollLeft })
    }
    const active = doc.activeElement
    let focus: Focus | null = null
    if (active && active !== doc.body && active !== doc.documentElement && DevState.#ours(active)) {
      const editable = active as Partial<HTMLInputElement>
      let start: number | null = null
      let end: number | null = null
      try {
        start = editable.selectionStart ?? null
        end = editable.selectionEnd ?? null
      } catch {}
      focus = { path: DevState.path(active), start, end }
    }
    return { x: win.scrollX, y: win.scrollY, fields, scrolls, focus }
  }

  static restore(win: Window, snapshot: DevSnapshot, done: (restored: number, missed: number) => void): void {
    const doc = win.document
    const fields = [...snapshot.fields]
    const scrolls = [...snapshot.scrolls]
    let focus = snapshot.focus
    let windowScroll = snapshot.x !== 0 || snapshot.y !== 0
    let restored = 0
    const fire = (element: Element, type: string) => element.dispatchEvent(new (win as unknown as { Event: typeof Event }).Event(type, { bubbles: true }))
    const field = (item: Field): boolean => {
      const element = doc.querySelector<Editable>(item.path)
      if (!element || !/^(input|textarea|select)$/.test(element.localName)) return false
      if (item.checked !== undefined) {
        const box = element as HTMLInputElement
        if (box.checked !== item.checked) box.click()
        return true
      }
      if (item.value !== undefined && element.value !== item.value) {
        const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(element), 'value')?.set
        if (setter) setter.call(element, item.value)
        else element.value = item.value
        fire(element, 'input')
        fire(element, 'change')
      }
      return true
    }
    const scroll = (item: Scroll): boolean => {
      const element = doc.querySelector(item.path)
      if (!element) return false
      element.scrollTop = item.top
      element.scrollLeft = item.left
      return element.scrollTop === item.top && element.scrollLeft === item.left
    }
    const pass = () => {
      for (const list of [fields, scrolls] as const) {
        for (let i = list.length - 1; i >= 0; i--) {
          const ok = list === fields ? field(list[i] as Field) : scroll(list[i] as Scroll)
          if (ok) {
            list.splice(i, 1)
            restored++
          }
        }
      }
      if (windowScroll) {
        win.scrollTo(snapshot.x, snapshot.y)
        if (win.scrollX === snapshot.x && win.scrollY === snapshot.y) {
          windowScroll = false
          restored++
        }
      }
      if (focus) {
        const element = doc.querySelector<HTMLElement>(focus.path)
        if (element) {
          element.focus()
          const editable = element as Partial<HTMLInputElement>
          if (focus.start != null && focus.end != null && editable.setSelectionRange) {
            try {
              editable.setSelectionRange(focus.start, focus.end)
            } catch {}
          }
          focus = null
          restored++
        }
      }
      return fields.length + scrolls.length + (windowScroll ? 1 : 0) + (focus ? 1 : 0)
    }
    let attempt = 0
    const next = () => {
      const left = pass()
      attempt++
      if (left === 0 || attempt === DevState.RETRIES.length) return done(restored, left)
      hostTime.setTimeout(next, DevState.RETRIES[attempt]! - DevState.RETRIES[attempt - 1]!)
    }
    next()
  }
}
