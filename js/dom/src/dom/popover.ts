import { DOMException } from '../core/exception'
import { fire, type EventTarget } from '../core/event'
import { queueTask } from '../core/host'
import { ToggleEvent } from './events'
import { dom, ID, wrap } from './realm'

type Popover = EventTarget & { readonly [ID]: number; readonly isConnected: boolean }
type Kind = 'auto' | 'manual' | 'hint'

const open = new Set<Popover>()
const autos: Popover[] = []
const modals: Popover[] = []

function kind(el: Popover): Kind | null {
  const value = dom.getAttribute(el[ID], 'popover')
  if (value === null) return null
  const v = value.toLowerCase()
  return v === '' || v === 'auto' ? 'auto' : v === 'hint' ? 'hint' : 'manual'
}

function contains(outer: Popover, inner: Popover): boolean {
  return outer === inner || dom.ancestors(inner[ID]).includes(outer[ID])
}

function check(el: Popover, method: string, opening: boolean): Kind {
  const k = kind(el)
  if (!k) {
    throw new DOMException(
      `Failed to execute '${method}' on 'HTMLElement': Not supported on elements that do not have a valid value for the 'popover' attribute.`,
      'NotSupportedError',
    )
  }
  if (opening && !el.isConnected) {
    throw new DOMException(`Failed to execute '${method}' on 'HTMLElement': Invalid on disconnected popover elements.`, 'InvalidStateError')
  }
  if (opening && dom.localName(el[ID]) === 'dialog' && dom.hasAttribute(el[ID], 'open')) {
    throw new DOMException(`Failed to execute '${method}' on 'HTMLElement': The dialog is already open as a dialog.`, 'InvalidStateError')
  }
  return k
}

function toggled(el: Popover, opened: boolean): void {
  const [oldState, newState] = opened ? ['closed', 'open'] : ['open', 'closed']
  queueTask(() => fire(el, new ToggleEvent('toggle', { oldState, newState })))
}

function closeAbove(keep: Popover | null): void {
  while (autos.length) {
    const top = autos[autos.length - 1]!
    if (keep && contains(top, keep)) return
    hidePopover(top)
  }
}

export function isPopoverOpen(el: Popover): boolean {
  return open.has(el)
}

export function showPopover(el: Popover): void {
  const k = check(el, 'showPopover', true)
  if (open.has(el)) return
  const before = new ToggleEvent('beforetoggle', { oldState: 'closed', newState: 'open', cancelable: true })
  if (!fire(el, before) || open.has(el) || kind(el) !== k || !el.isConnected) return
  if (k !== 'manual') closeAbove(el)
  open.add(el)
  if (k !== 'manual') autos.push(el)
  dom.setOpenState(el[ID], false, true)
  toggled(el, true)
}

export function hidePopover(el: Popover): void {
  check(el, 'hidePopover', false)
  if (!open.has(el)) return
  const at = autos.indexOf(el)
  if (at >= 0) while (autos.length > at + 1) hidePopover(autos[autos.length - 1]!)
  if (!open.has(el)) return
  fire(el, new ToggleEvent('beforetoggle', { oldState: 'open', newState: 'closed' }))
  if (!open.delete(el)) return
  const i = autos.indexOf(el)
  if (i >= 0) autos.splice(i, 1)
  dom.setOpenState(el[ID], false, false)
  toggled(el, false)
}

export function togglePopover(el: Popover, force?: boolean | { force?: boolean }): boolean {
  const wanted = typeof force === 'object' && force !== null ? force.force : force
  const show = wanted === undefined ? !open.has(el) : Boolean(wanted)
  if (show) showPopover(el)
  else hidePopover(el)
  return open.has(el)
}

export function popoverTarget(invoker: Popover): Popover | null {
  const id = dom.getAttribute(invoker[ID], 'popovertarget')
  if (!id) return null
  const target = dom.getElementById(id)
  const el = target >= 0 ? wrap<Popover>(target) : null
  return el && kind(el) ? el : null
}

export function invokePopover(invoker: Popover): boolean {
  const target = popoverTarget(invoker)
  if (!target) return false
  const action = dom.getAttribute(invoker[ID], 'popovertargetaction')?.toLowerCase()
  if (action === 'show') {
    if (!open.has(target)) showPopover(target)
  } else if (action === 'hide') {
    if (open.has(target)) hidePopover(target)
  } else togglePopover(target)
  return true
}

export function setModal(el: Popover, modal: boolean): void {
  const at = modals.indexOf(el)
  if (modal && at < 0) modals.push(el)
  if (!modal && at >= 0) modals.splice(at, 1)
  dom.setOpenState(el[ID], true, modal)
}

export function lightDismiss(target: Popover | null): void {
  if (!autos.length) return
  let keep: Popover | null = null
  for (let i = autos.length - 1; i >= 0 && target; i--) {
    const popover = autos[i]!
    if (contains(popover, target) || (popoverTarget(target) === popover)) {
      keep = popover
      break
    }
  }
  closeAbove(keep)
}

export function escape(): boolean {
  const top = autos[autos.length - 1]
  if (top) {
    hidePopover(top)
    return true
  }
  const dialog = modals[modals.length - 1] as (Popover & { requestClose(): void }) | undefined
  if (dialog) {
    dialog.requestClose()
    return true
  }
  return false
}
