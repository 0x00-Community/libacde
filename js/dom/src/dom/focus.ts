import { fire } from '../core/event'
import { NONE, NodeType } from '../native'
import { FocusEvent } from './events'
import type { Element } from './element'
import { metricsOf } from './layout'
import { CONNECTED, dom, ID, lifecycle, wrap, type Wrapper } from './realm'

export const focus = {
  id: NONE,
  revision: 0,
  modality: 'pointer' as 'pointer' | 'keyboard',
  windowActive: true,
}

const ALWAYS = new Set(['button', 'select', 'textarea', 'iframe', 'summary'])
const TEXT_TYPES = new Set(['', 'text', 'search', 'url', 'email', 'password', 'tel', 'number', 'date', 'datetime-local', 'month', 'time', 'week'])

export const EDITING_HOST = '[contenteditable=""],[contenteditable="true" i],[contenteditable="plaintext-only" i]'

export function focusable(id: number): boolean {
  if (dom.nodeType(id) !== NodeType.Element) return false
  const local = dom.localName(id)
  let candidate = dom.hasAttribute(id, 'tabindex')
  if (!candidate) {
    if (ALWAYS.has(local)) candidate = true
    else if (local === 'input') candidate = dom.getAttribute(id, 'type')?.toLowerCase() !== 'hidden'
    else if (local === 'a' || local === 'area') candidate = dom.hasAttribute(id, 'href')
    else if ((local === 'audio' || local === 'video') && dom.hasAttribute(id, 'controls')) candidate = true
    else candidate = editable(id)
  }
  if (!candidate) return false
  if ((local === 'button' || local === 'input' || local === 'select' || local === 'textarea') && (dom.hasAttribute(id, 'disabled') || dom.closest(id, 'fieldset[disabled] *') !== NONE)) return false
  if (dom.closest(id, '[inert]') !== NONE) return false
  return metricsOf(id) !== null || local === 'body' || local === 'html'
}

function editable(id: number): boolean {
  const value = dom.getAttribute(id, 'contenteditable')
  return value !== null && (value === '' || value.toLowerCase() === 'true' || value.toLowerCase() === 'plaintext-only')
}

function isTextField(id: number): boolean {
  const local = dom.localName(id)
  if (local === 'textarea') return true
  if (local === 'input') return TEXT_TYPES.has((dom.getAttribute(id, 'type') ?? '').toLowerCase())
  return dom.closest(id, EDITING_HOST) !== NONE
}

export function focusedElement(): Element | null {
  return focus.id === NONE ? null : wrap<Element>(focus.id)
}

export function setFocus(element: Element | null, visible?: boolean): void {
  const next = element ? element[ID] : NONE
  const previous = focus.id
  if (next === previous) {
    if (next !== NONE) dom.setFocus(next, visible ?? (focus.modality === 'keyboard' || isTextField(next)))
    return
  }
  const old = previous === NONE ? null : wrap<Element>(previous)
  const picker = dom.pickerState()[0]!
  if (picker >= 0 && picker !== next) dom.closePicker()
  focus.id = next
  focus.revision++
  dom.setFocus(next, next === NONE ? false : (visible ?? (focus.modality === 'keyboard' || isTextField(next))))
  if (old?.[CONNECTED]) {
    fire(old, new FocusEvent('blur', { relatedTarget: element }))
    fire(old, new FocusEvent('focusout', { bubbles: true, relatedTarget: element }))
  }
  if (focus.id !== next || !element) return
  fire(element, new FocusEvent('focus', { relatedTarget: old }))
  fire(element, new FocusEvent('focusin', { bubbles: true, relatedTarget: old }))
}

lifecycle.disconnected.push((node: Wrapper) => {
  if (node[ID] === focus.id) {
    focus.id = NONE
    focus.revision++
    dom.setFocus(NONE, false)
  }
})
