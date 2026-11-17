import { DOMException } from '../core/exception'
import { getters, tag } from '../core/host'
import { Ns } from '../native'
import type { Element } from './element'
import { LOCAL, NS } from './element'
import { SHADOW_ROOT } from './events'
import { focusedElement } from './focus'
import { DocumentFragment } from './node'
import { CONNECTED, ID } from './realm'
import { getSelection, type Selection } from './range'

const HOST = Symbol('host')
const MODE = Symbol('mode')
const DELEGATES_FOCUS = Symbol('delegatesFocus')
const VALID_HOSTS = new Set(['article', 'aside', 'blockquote', 'body', 'div', 'footer', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'header', 'main', 'nav', 'p', 'section', 'span'])

export class ShadowRoot extends DocumentFragment {
  declare [HOST]: Element
  declare [MODE]: string
  declare [DELEGATES_FOCUS]: boolean

  get host(): Element {
    return this[HOST]
  }
  get mode(): string {
    return this[MODE]
  }
  get delegatesFocus(): boolean {
    return this[DELEGATES_FOCUS]
  }
  override get parentNode(): null {
    return null
  }
  override get nodeName(): string {
    return '#document-fragment'
  }

  get activeElement(): Element | null {
    const focused = focusedElement()
    return focused && focused !== this[HOST] && this[HOST].contains(focused) ? focused : null
  }

  get innerHTML(): string {
    return this[HOST].innerHTML
  }
  set innerHTML(html: string) {
    this[HOST].innerHTML = html
  }

  getSelection(): Selection {
    return getSelection()
  }

  elementFromPoint(x: number, y: number): Element | null {
    return (this[HOST].ownerDocument as unknown as { elementFromPoint(x: number, y: number): Element | null }).elementFromPoint(x, y)
  }

  elementsFromPoint(x: number, y: number): Element[] {
    return (this[HOST].ownerDocument as unknown as { elementsFromPoint(x: number, y: number): Element[] }).elementsFromPoint(x, y)
  }

  getAnimations(): unknown[] {
    return []
  }
}
getters(ShadowRoot.prototype, { slotAssignment: 'named', clonable: false, serializable: false, styleSheets: () => [] })
Object.defineProperty(ShadowRoot.prototype, CONNECTED, {
  get(this: ShadowRoot) {
    return this[HOST][CONNECTED]
  },
  set() {},
  configurable: true,
})
tag(ShadowRoot)

export function attachShadow(host: Element, init: { mode?: string; delegatesFocus?: boolean }): ShadowRoot {
  const holder = host as unknown as { [SHADOW_ROOT]?: ShadowRoot }
  if (host[NS] !== Ns.Html || (!VALID_HOSTS.has(host[LOCAL]) && !host[LOCAL].includes('-'))) throw new DOMException(`Failed to execute 'attachShadow' on 'Element': This element does not support attachShadow`, 'NotSupportedError')
  if (holder[SHADOW_ROOT]) throw new DOMException("Failed to execute 'attachShadow' on 'Element': Shadow root cannot be created on a host which already hosts a shadow tree.", 'NotSupportedError')
  if (init?.mode !== 'open' && init?.mode !== 'closed') throw new TypeError("Failed to execute 'attachShadow' on 'Element': The provided value must be 'open' or 'closed'.")
  const root = Object.create(ShadowRoot.prototype) as ShadowRoot
  ;(root as unknown as { [ID]: number })[ID] = host[ID]
  root[HOST] = host
  root[MODE] = init.mode
  root[DELEGATES_FOCUS] = !!init.delegatesFocus
  holder[SHADOW_ROOT] = root
  return root
}

export function shadowRootOf(host: Element): ShadowRoot | null {
  const root = (host as unknown as { [SHADOW_ROOT]?: ShadowRoot })[SHADOW_ROOT]
  return root && root[MODE] === 'open' ? root : null
}

