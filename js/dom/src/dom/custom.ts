import { DOMException } from '../core/exception'
import { report, tag } from '../core/host'
import { Ns } from '../native'
import { customAttributes, type Element, parsedHooks } from './element'
import { customHooks, HTMLElement } from './html'
import { dom, existing, ID, lifecycle, wrap, type Wrapper, inner } from './realm'
import type { Node } from './node'

interface Definition {
  readonly name: string
  readonly ctor: new () => HTMLElement
  readonly observed: Set<string>
  readonly connected?: (this: Element) => void
  readonly disconnected?: (this: Element) => void
  readonly changed?: (this: Element, name: string, old: string | null, value: string | null) => void
}

const byName = new Map<string, Definition>()
const byCtor = new Map<Function, Definition>()
const waiting = new Map<string, { promise: Promise<unknown>; resolve(ctor: unknown): void }>()
const DEFINITION = Symbol('definition')

const RESERVED = new Set(['annotation-xml', 'color-profile', 'font-face', 'font-face-src', 'font-face-uri', 'font-face-format', 'font-face-name', 'missing-glyph'])
const VALID = /^[a-z][-.0-9_a-z·À-ÖØ-öø-ͽͿ-῿‌-‍‿-⁀⁰-↏Ⰰ-⿯、-퟿豈-﷏ﷰ-�]*-[-.0-9_a-z·À-ÖØ-öø-ͽͿ-῿‌-‍‿-⁀⁰-↏Ⰰ-⿯、-퟿豈-﷏ﷰ-�]*$/

export function customName(ctor: Function): string | null {
  return byCtor.get(ctor)?.name ?? null
}

export function customClass(name: string): (new () => HTMLElement) | null {
  return byName.get(name)?.ctor ?? null
}

function definitionOf(node: object): Definition | undefined {
  return (node as { [DEFINITION]?: Definition })[DEFINITION]
}

function call(element: Element, fn: ((...args: never[]) => void) | undefined, ...args: unknown[]): void {
  if (!fn) return
  try {
    ;(fn as (...a: unknown[]) => void).apply(element, args)
  } catch (error) {
    report(error)
  }
}

lifecycle.connected.push((node: Wrapper) => {
  const def = definitionOf(node)
  if (def) call(node as unknown as Element, def.connected)
})
lifecycle.disconnected.push((node: Wrapper) => {
  const def = definitionOf(node)
  if (def) call(node as unknown as Element, def.disconnected)
})

customAttributes.before = (element, name) => {
  const def = definitionOf(element)
  return def && def.observed.has(name) ? dom.getAttribute(element[ID], name) : undefined
}
customAttributes.after = (element, name, old, value) => {
  call(element, definitionOf(element)?.changed, name, old, value)
}

function upgradeIn(root: number, names: readonly string[]): void {
  if (names.length === 0) return
  for (const id of dom.querySelectorAll(root, names.join(','))) {
    const known = existing(id)
    if (known && definitionOf(known)) continue
    const local = dom.localName(id)
    const def = byName.get(local)
    if (!def || dom.namespace(id) !== Ns.Html) continue
    if (known) upgradeExisting(known as unknown as Element, def)
    else {
      const element = wrap<Element>(id)
      if (element && dom.isConnected(id)) call(element, def.connected)
    }
  }
}

function upgradeExisting(element: Element, def: Definition): void {
  Object.setPrototypeOf(element, def.ctor.prototype)
  customHooks.upgrading = element as HTMLElement
  try {
    new def.ctor()
  } catch (error) {
    report(error)
  } finally {
    customHooks.upgrading = null
  }
  for (const name of def.observed) {
    const value = dom.getAttribute(element[ID], name)
    if (value !== null) call(element, def.changed, name, null, value)
  }
  if (inner(element, 'isConnected')) call(element, def.connected)
}

parsedHooks.push((container: Node) => {
  if (byName.size) upgradeIn(container[ID], [...byName.keys()])
})

export class CustomElementRegistry {
  define(name: string, ctor: new () => HTMLElement, options?: { extends?: string }): void {
    const n = String(name)
    if (typeof ctor !== 'function') throw new TypeError("Failed to execute 'define' on 'CustomElementRegistry': The provided value is not of type 'Function'.")
    if (!VALID.test(n) || RESERVED.has(n)) throw new DOMException(`Failed to execute 'define' on 'CustomElementRegistry': "${n}" is not a valid custom element name`, 'SyntaxError')
    if (byName.has(n)) throw new DOMException(`Failed to execute 'define' on 'CustomElementRegistry': the name "${n}" has already been used with this registry`, 'NotSupportedError')
    if (byCtor.has(ctor)) throw new DOMException("Failed to execute 'define' on 'CustomElementRegistry': this constructor has already been used with this registry", 'NotSupportedError')
    if (options?.extends) throw new DOMException('Customized built-in elements are not supported', 'NotSupportedError')
    const proto = ctor.prototype as Record<string, unknown>
    const observed = (ctor as unknown as { observedAttributes?: Iterable<string> }).observedAttributes
    const def: Definition = {
      name: n,
      ctor,
      observed: new Set(observed ? [...observed].map(String) : []),
      connected: proto['connectedCallback'] as Definition['connected'],
      disconnected: proto['disconnectedCallback'] as Definition['disconnected'],
      changed: proto['attributeChangedCallback'] as Definition['changed'],
    }
    Object.defineProperty(proto, DEFINITION, { value: def, configurable: true })
    byName.set(n, def)
    byCtor.set(ctor, def)
    upgradeIn(dom.document(), [n])
    const wait = waiting.get(n)
    if (wait) {
      waiting.delete(n)
      wait.resolve(ctor)
    }
  }

  get(name: string): (new () => HTMLElement) | undefined {
    return byName.get(String(name))?.ctor
  }

  getName(ctor: Function): string | null {
    return byCtor.get(ctor)?.name ?? null
  }

  whenDefined(name: string): Promise<unknown> {
    const n = String(name)
    if (!VALID.test(n)) return Promise.reject(new DOMException(`"${n}" is not a valid custom element name`, 'SyntaxError'))
    const def = byName.get(n)
    if (def) return Promise.resolve(def.ctor)
    let wait = waiting.get(n)
    if (!wait) {
      let resolve!: (ctor: unknown) => void
      const promise = new Promise<unknown>((r) => (resolve = r))
      wait = { promise, resolve }
      waiting.set(n, wait)
    }
    return wait.promise
  }

  upgrade(root: Node): void {
    upgradeIn(root[ID], [...byName.keys()])
  }
}
tag(CustomElementRegistry)
