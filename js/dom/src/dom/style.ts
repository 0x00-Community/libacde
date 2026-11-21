import { DOMException, guard } from '../core/exception'
import { constants, getters, listClass, tag } from '../core/host'
import { setAttribute, type Element } from './element'
import { computedStyleOf } from './layout'
import { changed, dom, ID, version } from './realm'

type Declarations = Map<string, { value: string; important: boolean }>

const INLINE = Symbol('inlineStyle')

const propertyNames = new Map<string, string>()

export function propertyName(key: string): string {
  let name = propertyNames.get(key)
  if (name !== undefined) return name
  if (key.startsWith('--')) name = key
  else if (key === 'cssFloat') name = 'float'
  else if (key.includes('-')) name = key.toLowerCase()
  else {
    const dashed = key.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)
    name = /^(webkit|moz|ms|o)-/.test(dashed) ? `-${dashed}` : dashed
  }
  if (propertyNames.size > 4096) propertyNames.clear()
  propertyNames.set(key, name)
  return name
}

function propertyKey(name: string): string {
  return name.startsWith('--') ? name : name.toLowerCase()
}

function serialize(declarations: Declarations): string {
  return [...declarations].map(([name, { value, important }]) => `${name}: ${value}${important ? ' !important' : ''};`).join(' ')
}

function parseDeclarations(text: string): Declarations {
  const declarations: Declarations = new Map()
  let depth = 0, quote = '', start = 0
  const take = (end: number) => {
    const part = text.slice(start, end)
    start = end + 1
    const colon = part.indexOf(':')
    if (colon < 0) return
    const name = part.slice(0, colon).trim()
    let value = part.slice(colon + 1).trim()
    const important = /!\s*important$/i.test(value)
    if (important) value = value.replace(/!\s*important$/i, '').trim()
    if (name && value) {
      const key = propertyKey(name)
      declarations.delete(key)
      declarations.set(key, { value, important })
    }
  }
  for (let i = 0; i < text.length; i++) {
    const c = text[i]!
    if (quote) {
      if (c === '\\') i++
      else if (c === quote) quote = ''
    } else if (c === '"' || c === "'") quote = c
    else if (c === '(' || c === '[') depth++
    else if (c === ')' || c === ']') depth = Math.max(0, depth - 1)
    else if (c === ';' && depth === 0) take(i)
  }
  take(text.length)
  return declarations
}

const handler: ProxyHandler<CSSStyleDeclaration> = {
  get(target, key) {
    if (typeof key === 'symbol' || key in target) return Reflect.get(target, key)
    if (/^\d+$/.test(key)) return target.item(Number(key))
    return target.getPropertyValue(propertyName(key))
  },
  set(target, key, value) {
    if (typeof key === 'symbol' || key in target) return Reflect.set(target, key, value)
    target.setProperty(propertyName(key), value === null || value === undefined ? '' : String(value))
    return true
  },
}

interface DeclarationState {
  readonly element: Element | null
  readonly pseudo: string
  readonly computed: boolean
  text: string | null
  textVersion: number
  parsed: Declarations
  names: string[] | null
  namesVersion: number
}

const STATE = Symbol('state')

export function declarationsOf(style: CSSStyleDeclaration): Declarations {
  const s = style[STATE]
  if (!s.element || s.textVersion === version) return s.parsed
  const text = dom.getAttribute(s.element[ID], 'style')
  s.textVersion = version
  if (text === s.text) return s.parsed
  s.text = text
  s.parsed = parseDeclarations(text ?? '')
  return s.parsed
}

export function writeDeclarations(style: CSSStyleDeclaration, declarations: Declarations): void {
  const s = style[STATE]
  const text = serialize(declarations)
  if (!s.element) {
    s.parsed = declarations
    return
  }
  if (text === (s.text ?? '')) return
  s.text = text === '' ? null : text
  s.parsed = declarations
  setAttribute(s.element, 'style', s.text)
  s.textVersion = version
}

function writable(style: CSSStyleDeclaration): void {
  if (style[STATE].computed) throw new DOMException("Failed to execute 'setProperty' on 'CSSStyleDeclaration': These styles are computed, and therefore read-only.", 'NoModificationAllowedError')
}

function nameList(style: CSSStyleDeclaration): string[] {
  const s = style[STATE]
  if (!s.computed) return [...declarationsOf(style).keys()]
  if (s.namesVersion !== version || !s.names) {
    const all = dom.computedStyleAll(s.element![ID], s.pseudo)
    const names: string[] = []
    for (let i = 0; i < all.length; i += 2) names.push(all[i]!)
    s.names = names
    s.namesVersion = version
  }
  return s.names
}

export class CSSStyleDeclaration {
  readonly [STATE]: DeclarationState

  constructor(element: Element | null = null, computed = false, pseudo = '') {
    this[STATE] = { element, pseudo, computed, text: null, textVersion: -1, parsed: new Map(), names: null, namesVersion: -1 }
  }

  get cssText(): string {
    const s = this[STATE]
    if (s.computed) return ''
    return s.element ? (dom.getAttribute(s.element[ID], 'style') ?? '') : serialize(s.parsed)
  }
  set cssText(text: string) {
    writable(this)
    const s = this[STATE]
    if (s.element) setAttribute(s.element, 'style', String(text))
    else s.parsed = parseDeclarations(String(text))
  }

  get length(): number {
    return nameList(this).length
  }

  item(index: number): string {
    return nameList(this)[index] ?? ''
  }

  getPropertyValue(name: string): string {
    const key = propertyKey(String(name))
    const s = this[STATE]
    if (s.computed) return computedStyleOf(s.element![ID], s.pseudo, key)
    return declarationsOf(this).get(key)?.value ?? ''
  }

  getPropertyPriority(name: string): string {
    if (this[STATE].computed) return ''
    return declarationsOf(this).get(propertyKey(String(name)))?.important ? 'important' : ''
  }

  setProperty(name: string, value: string | null, priority = ''): void {
    writable(this)
    const key = propertyKey(String(name))
    const declarations = new Map(declarationsOf(this))
    const text = value === null || value === undefined ? '' : String(value).trim()
    if (text === '') declarations.delete(key)
    else {
      const important = String(priority).toLowerCase() === 'important'
      if (priority && !important) return
      const old = declarations.get(key)
      if (old && old.value === text && old.important === important) return
      declarations.set(key, { value: text, important })
    }
    writeDeclarations(this, declarations)
  }

  removeProperty(name: string): string {
    writable(this)
    const old = this.getPropertyValue(name)
    this.setProperty(name, '')
    return old
  }

  get cssFloat(): string {
    return this.getPropertyValue('float')
  }
  set cssFloat(value: string) {
    this.setProperty('float', value)
  }

  *[Symbol.iterator](): IterableIterator<string> {
    yield* nameList(this)
  }
}
getters(CSSStyleDeclaration.prototype, { parentRule: null })
tag(CSSStyleDeclaration)

export function inlineStyle(element: Element): CSSStyleDeclaration {
  const holder = element as unknown as { [INLINE]?: CSSStyleDeclaration }
  return (holder[INLINE] ??= new Proxy(new CSSStyleDeclaration(element), handler))
}

export function computedStyle(element: Element, pseudo: string): CSSStyleDeclaration {
  return new Proxy(new CSSStyleDeclaration(element, true, pseudo), handler)
}

function freeStyle(text: string): CSSStyleDeclaration {
  const style = new CSSStyleDeclaration(null)
  style[STATE].parsed = parseDeclarations(text)
  return new Proxy(style, handler)
}

const TEXT = Symbol('text')
const DETACH = Symbol('detach')
const COUNT = Symbol('count')
const AT = Symbol('at')

interface RuleOwner {
  [COUNT](): number
  [AT](index: number): CSSRule
}

class RuleCache {
  objects: (CSSRule | undefined)[] = []

  at(index: number, text: string, make: () => CSSRule): CSSRule {
    const held = this.objects[index]
    if (held && held[TEXT] === text) return held
    const made = make()
    this.objects[index] = made
    return made
  }

  insert(index: number): void {
    this.objects.splice(index, 0, undefined)
  }

  remove(index: number): void {
    this.objects.splice(index, 1)[0]?.[DETACH]()
  }

  clear(): void {
    for (const rule of this.objects.splice(0)) rule?.[DETACH]()
  }
}

export class CSSRule {
  [TEXT]: string
  #sheet: CSSStyleSheet | null
  #parent: CSSGroupingRule | null

  constructor(text: string, sheet: CSSStyleSheet | null, parent: CSSGroupingRule | null = null) {
    this[TEXT] = text
    this.#sheet = sheet
    this.#parent = parent
  }
  get cssText(): string {
    return this[TEXT]
  }
  get parentStyleSheet(): CSSStyleSheet | null {
    return this.#sheet
  }
  get parentRule(): CSSGroupingRule | null {
    return this.#parent
  }
  get type(): number {
    const text = this[TEXT].trimStart()
    if (text.startsWith('@media')) return 4
    if (text.startsWith('@font-face')) return 5
    if (text.startsWith('@keyframes')) return 7
    if (text.startsWith('@import')) return 3
    if (text.startsWith('@supports')) return 12
    return text.startsWith('@') ? 0 : 1
  }
  get selectorText(): string {
    return this.type === 1 ? this[TEXT].slice(0, this[TEXT].indexOf('{')).trim() : ''
  }
  get style(): CSSStyleDeclaration {
    const open = this[TEXT].indexOf('{')
    return freeStyle(open < 0 ? '' : this[TEXT].slice(open + 1, this[TEXT].lastIndexOf('}')))
  }
  [DETACH](): void {
    this.#sheet = null
    this.#parent = null
  }
}
constants(CSSRule, { STYLE_RULE: 1, IMPORT_RULE: 3, MEDIA_RULE: 4, FONT_FACE_RULE: 5, KEYFRAMES_RULE: 7, SUPPORTS_RULE: 12 })
tag(CSSRule)
export class CSSStyleRule extends CSSRule {}
tag(CSSStyleRule)

function parts(text: string): { prelude: string; inner: string[] } {
  const open = text.indexOf('{'), close = text.lastIndexOf('}')
  if (open < 0 || close <= open) return { prelude: text.trim(), inner: [] }
  return { prelude: text.slice(0, open).trim(), inner: splitRules(text.slice(open + 1, close)) }
}

function joined(prelude: string, inner: readonly string[]): string {
  return inner.length === 0 ? `${prelude} {\n}` : `${prelude} {\n${inner.map((rule) => `  ${rule}`).join('\n')}\n}`
}

let rewrite: (rule: CSSRule, text: string) => void

export class CSSGroupingRule extends CSSRule implements RuleOwner {
  readonly #cache = new RuleCache()
  #list: CSSRuleList | null = null

  get cssRules(): CSSRuleList {
    return (this.#list ??= new Proxy(new CSSRuleList(this), ruleIndex))
  }

  [COUNT](): number {
    return parts(this[TEXT]).inner.length
  }

  [AT](index: number): CSSRule {
    const text = parts(this[TEXT]).inner[index] ?? ''
    return this.#cache.at(index, text, () => ruleOf(text, this.parentStyleSheet, this))
  }

  insertRule(rule: string, index = 0): number {
    const what = `Failed to execute 'insertRule' on '${this[Symbol.toStringTag as never] ?? 'CSSGroupingRule'}'`
    const text = String(rule).trim()
    const at = Number(index) >>> 0
    const { prelude, inner } = parts(this[TEXT])
    if (!text || splitRules(text).length !== 1) throw new DOMException(`${what}: Failed to parse the rule '${rule}'.`, 'SyntaxError')
    if (at > inner.length) throw new DOMException(`${what}: The index provided (${index}) is larger than the maximum index (${inner.length}).`, 'IndexSizeError')
    const before = this[TEXT]
    inner.splice(at, 0, text)
    rewrite(this, joined(prelude, inner))
    if (parts(this[TEXT]).inner.length !== inner.length) {
      rewrite(this, before)
      throw new DOMException(`${what}: Failed to parse the rule '${rule}'.`, 'SyntaxError')
    }
    this.#cache.insert(at)
    return at
  }

  deleteRule(index: number): void {
    const what = `Failed to execute 'deleteRule' on '${this[Symbol.toStringTag as never] ?? 'CSSGroupingRule'}'`
    const at = Number(index) >>> 0
    const { prelude, inner } = parts(this[TEXT])
    if (at >= inner.length) throw new DOMException(`${what}: The index provided (${index}) is larger than the maximum index (${inner.length - 1}).`, 'IndexSizeError')
    inner.splice(at, 1)
    rewrite(this, joined(prelude, inner))
    this.#cache.remove(at)
  }
}
tag(CSSGroupingRule)

export class CSSConditionRule extends CSSGroupingRule {
  get conditionText(): string {
    return parts(this[TEXT]).prelude.replace(/^@(media|supports)\b/i, '').trim()
  }
}
tag(CSSConditionRule)

export class CSSMediaRule extends CSSConditionRule {
  get media(): { mediaText: string; length: number } {
    const text = this.conditionText
    return { mediaText: text, length: text ? text.split(',').length : 0 }
  }
}
tag(CSSMediaRule)

export class CSSSupportsRule extends CSSConditionRule {}
tag(CSSSupportsRule)

function splitRules(text: string): string[] {
  const rules: string[] = []
  let depth = 0, start = 0, quote = ''
  for (let i = 0; i < text.length; i++) {
    const c = text[i]!
    if (quote) {
      if (c === '\\') i++
      else if (c === quote) quote = ''
      continue
    }
    if (c === '/' && text[i + 1] === '*') {
      const end = text.indexOf('*/', i + 2)
      i = end < 0 ? text.length : end + 1
      continue
    }
    if (c === '"' || c === "'") quote = c
    else if (c === '{') depth++
    else if (c === '}') {
      depth--
      if (depth === 0) {
        rules.push(text.slice(start, i + 1).trim())
        start = i + 1
      }
    } else if (c === ';' && depth === 0) {
      const rule = text.slice(start, i + 1).trim()
      if (rule) rules.push(rule)
      start = i + 1
    }
  }
  return rules.filter(Boolean)
}

const SHEET = Symbol('sheet')

export class CSSRuleList {
  readonly [SHEET]: RuleOwner

  constructor(owner: RuleOwner) {
    this[SHEET] = owner
  }

  get length(): number {
    return this[SHEET][COUNT]()
  }

  item(index: number): CSSRule | null {
    const i = Number(index) >>> 0
    return i < this[SHEET][COUNT]() ? this[SHEET][AT](i) : null
  }

  *[Symbol.iterator](): IterableIterator<CSSRule> {
    const n = this.length
    for (let i = 0; i < n; i++) yield this[SHEET][AT](i)
  }
}
tag(CSSRuleList)

const ruleIndex: ProxyHandler<CSSRuleList> = {
  get(target, key) {
    if (typeof key === 'string' && key.length > 0 && key.charCodeAt(0) >= 48 && key.charCodeAt(0) <= 57 && String(Number(key)) === key) return target.item(Number(key)) ?? undefined
    return Reflect.get(target, key)
  },
  has(target, key) {
    if (typeof key === 'string' && key.length > 0 && key.charCodeAt(0) >= 48 && key.charCodeAt(0) <= 57 && String(Number(key)) === key) return Number(key) < target.length
    return Reflect.has(target, key)
  },
}

const NO_SERIAL = -2

let ruleCount: (sheet: CSSStyleSheet) => number
export let fontFaceTexts: (sheet: CSSStyleSheet) => string[]

function ruleOf(text: string, sheet: CSSStyleSheet | null, parent: CSSGroupingRule | null): CSSRule {
  const start = text.trimStart()
  if (/^@media\b/i.test(start)) return new CSSMediaRule(text, sheet, parent)
  if (/^@supports\b/i.test(start)) return new CSSSupportsRule(text, sheet, parent)
  return start.startsWith('@') ? new CSSRule(text, sheet, parent) : new CSSStyleRule(text, sheet, parent)
}

export class CSSStyleSheet implements RuleOwner {
  #rules: string[] = []
  readonly #cache = new RuleCache()
  readonly #owner: Element | null
  readonly #serial: number = NO_SERIAL
  readonly #href: string | null = null
  readonly #opaque: boolean = false
  #list: CSSRuleList | null = null
  #disabled = false

  constructor(options?: { media?: string; disabled?: boolean }, owner: Element | null = null) {
    this.#owner = owner
    if (!owner) this.#disabled = !!options?.disabled
    if (!owner || dom.localName(owner[ID]) !== 'link') return
    this.#serial = dom.sheetSerial(owner[ID])
    this.#href = dom.sheetUrl(owner[ID])
    this.#opaque = dom.sheetOpaque(owner[ID])
  }

  get disabled(): boolean {
    return this.#owner ? dom.sheetDisabled(this.#owner[ID]) : this.#disabled
  }
  set disabled(value: boolean) {
    if (!this.#owner) return void (this.#disabled = !!value)
    dom.setSheetDisabled(this.#owner[ID], !!value)
    changed()
  }

  get ownerNode(): Element | null {
    return this.#owner
  }
  get href(): string | null {
    return this.#href
  }
  get media(): { mediaText: string; length: number } {
    const text = (this.#owner ? dom.getAttribute(this.#owner[ID], 'media') : null) ?? ''
    return { mediaText: text, length: text.trim() ? text.split(',').length : 0 }
  }

  #open(access: string): void {
    if (this.#opaque) throw new DOMException(access, 'SecurityError')
  }

  #constructed(member: string): void {
    if (this.#owner) throw new DOMException(`Failed to execute '${member}' on 'CSSStyleSheet': Can't call ${member} on non-constructed CSSStyleSheets.`, 'NotAllowedError')
  }

  #native(): number {
    if (!this.#owner) return -1
    if (this.#serial !== NO_SERIAL && (this.#opaque || dom.sheetSerial(this.#owner[ID]) !== this.#serial)) return -1
    return dom.sheetRuleCount(this.#owner[ID])
  }

  #text(): string[] {
    if (this.#owner && this.#serial === NO_SERIAL) this.#rules = splitRules(dom.textContent(this.#owner[ID]))
    return this.#rules
  }

  static {
    ruleCount = (sheet) => {
      const n = sheet.#native()
      return n >= 0 ? n : sheet.#text().length
    }
    rewrite = (rule, text) => {
      const parent = rule.parentRule
      if (parent) {
        const { prelude, inner } = parts(parent[TEXT])
        const at = Array.from({ length: inner.length }, (_, i) => i).find((i) => parent[AT](i) === rule)
        if (at === undefined) return void (rule[TEXT] = text)
        inner[at] = text
        rewrite(parent, joined(prelude, inner))
        rule[TEXT] = parts(parent[TEXT]).inner[at] ?? text
        return
      }
      const sheet = rule.parentStyleSheet
      const at = sheet ? sheet.#cache.objects.indexOf(rule) : -1
      const current = sheet && at >= 0 ? (sheet.#native() >= 0 ? dom.sheetRule(sheet.#owner![ID], at) : sheet.#text()[at]) : undefined
      if (!sheet || current !== rule[TEXT]) return void (rule[TEXT] = text)
      if (sheet.#native() >= 0) {
        const owner = sheet.#owner![ID]
        guard(() => {
          dom.sheetDeleteRule(owner, at)
          dom.sheetInsertRule(owner, text, at)
        })
        changed()
        rule[TEXT] = dom.sheetRule(owner, at)
        return
      }
      sheet.#text()[at] = text
      sheet.#flush()
      rule[TEXT] = text
    }
    fontFaceTexts = (sheet) => {
      const owner = sheet.#owner
      const live = owner !== null && sheet.#serial !== NO_SERIAL && dom.sheetSerial(owner[ID]) === sheet.#serial
      const texts = live ? Array.from({ length: dom.sheetRuleCount(owner[ID]) }, (_, i) => dom.sheetRule(owner[ID], i)) : sheet.#text()
      return texts.filter((text) => /^\s*@font-face\b/i.test(text))
    }
  }

  [COUNT](): number {
    return ruleCount(this)
  }

  [AT](index: number): CSSRule {
    const text = this.#native() >= 0 ? dom.sheetRule(this.#owner![ID], index) : (this.#text()[index] ?? '')
    return this.#cache.at(index, text, () => ruleOf(text, this, null))
  }

  get cssRules(): CSSRuleList {
    this.#open("Failed to read the 'cssRules' property from 'CSSStyleSheet': Cannot access rules")
    return (this.#list ??= new Proxy(new CSSRuleList(this), ruleIndex))
  }

  get rules(): CSSRuleList {
    this.#open("Failed to read the 'rules' property from 'CSSStyleSheet': Cannot access rules")
    return (this.#list ??= new Proxy(new CSSRuleList(this), ruleIndex))
  }

  insertRule(rule: string, index = 0): number {
    this.#open("Failed to execute 'insertRule' on 'CSSStyleSheet': Cannot access StyleSheet to insertRule")
    const text = String(rule)
    const at = Number(index) >>> 0
    if (this.#native() >= 0) {
      const inserted = guard(() => dom.sheetInsertRule(this.#owner![ID], text, at))
      changed()
      this.#cache.insert(inserted)
      return inserted
    }
    const rules = this.#text()
    const trimmed = text.trim()
    if (!trimmed || splitRules(trimmed).length !== 1) throw new DOMException(`Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to parse the rule '${rule}'.`, 'SyntaxError')
    if (at > rules.length) throw new DOMException(`Failed to execute 'insertRule' on 'CSSStyleSheet': The index provided (${index}) is larger than the maximum index (${rules.length}).`, 'IndexSizeError')
    rules.splice(at, 0, trimmed)
    this.#flush()
    this.#cache.insert(at)
    return at
  }

  addRule(selector: string, style: string, index?: number): number {
    this.insertRule(`${selector} { ${style} }`, index ?? ruleCount(this))
    return -1
  }

  deleteRule(index: number): void {
    this.#open("Failed to execute 'deleteRule' on 'CSSStyleSheet': Cannot access StyleSheet to deleteRule")
    const at = Number(index) >>> 0
    if (this.#native() >= 0) {
      guard(() => dom.sheetDeleteRule(this.#owner![ID], at))
      changed()
      this.#cache.remove(at)
      return
    }
    const rules = this.#text()
    if (at >= rules.length) throw new DOMException(`Failed to execute 'deleteRule' on 'CSSStyleSheet': The index provided (${index}) is larger than the maximum index (${rules.length - 1}).`, 'IndexSizeError')
    rules.splice(at, 1)
    this.#flush()
    this.#cache.remove(at)
  }

  removeRule(index = 0): void {
    this.deleteRule(index)
  }

  replaceSync(text: string): void {
    this.#constructed('replaceSync')
    this.#rules = splitRules(String(text))
    this.#cache.clear()
  }

  async replace(text: string): Promise<CSSStyleSheet> {
    this.#constructed('replace')
    this.#rules = splitRules(String(text))
    this.#cache.clear()
    return this
  }

  #flush(): void {
    if (this.#owner && this.#serial === NO_SERIAL) (this.#owner as unknown as { textContent: string }).textContent = this.#rules.join('\n')
  }
}
getters(CSSStyleSheet.prototype, { type: 'text/css', title: null, parentStyleSheet: null, ownerRule: null })
tag(CSSStyleSheet)

export const StyleSheetList = listClass<CSSStyleSheet>('StyleSheetList')
export type StyleSheetList = InstanceType<typeof StyleSheetList>

export const CSS = {
  escape(value: string): string {
    const text = String(value)
    let out = ''
    for (let i = 0; i < text.length; i++) {
      const c = text.charCodeAt(i)
      if (c === 0) out += '�'
      else if ((c >= 1 && c <= 31) || c === 127 || (i === 0 && c >= 48 && c <= 57) || (i === 1 && c >= 48 && c <= 57 && text.charCodeAt(0) === 45)) out += `\\${c.toString(16)} `
      else if (i === 0 && c === 45 && text.length === 1) out += `\\${text[i]}`
      else if (c >= 128 || c === 45 || c === 95 || (c >= 48 && c <= 57) || (c >= 65 && c <= 90) || (c >= 97 && c <= 122)) out += text[i]
      else out += `\\${text[i]}`
    }
    return out
  },
  supports(...args: readonly unknown[]): boolean {
    if (args.length === 0) throw new TypeError("Failed to execute 'supports' on 'CSS': 1 argument required, but only 0 present.")
    if (args.length === 1) return dom.cssSupports(String(args[0]))
    return dom.cssSupportsDeclaration(String(args[0]), String(args[1]))
  },
}
