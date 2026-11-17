import { DOMException } from '../core/exception'
import { Event, fire } from '../core/event'
import { getters, host, queueTask, tag } from '../core/host'
import { File } from '../core/blob'
import { NONE } from '../native'
import { legacy } from './legacy'
import { invokePopover, popoverTarget } from './popover'
import { ACTIVATION, activation, FileList, SubmitEvent } from './events'
import { ATTRIBUTE_CHANGED, flags, LOCAL, setAttribute, tokenList, type Element, type DOMTokenList } from './element'
import { env, resolveUrl } from './env'
import { focusable, setFocus } from './focus'
import { createHtml, HTMLElement, register } from './html'
import { acceptFilters, readPicked } from './picker'
import { collectionShape, cssString, DocumentFragment, HTMLCollection, listShape, liveCollection, liveIds, mixin, namedBy, namesOf, NodeList, staticNodeList, type Node } from './node'
import { bool, long, string, url } from './reflect'
import { documentId, dom, exotic, ID, version, wrap, inner } from './realm'

const SELECTION = Symbol('selection')
const CUSTOM_VALIDITY = Symbol('customValidity')
const INDETERMINATE = Symbol('indeterminate')

type Selection = { start: number; end: number; direction: 'forward' | 'backward' | 'none' }
type Field = HTMLInputElement | HTMLTextAreaElement

const VALUE_MODE = new Set(['text', 'search', 'url', 'tel', 'email', 'password', 'number', 'range', 'color', 'date', 'month', 'week', 'time', 'datetime-local'])
const DEFAULT_MODE = new Set(['hidden', 'submit', 'image', 'reset', 'button'])
const SELECTABLE = new Set(['text', 'search', 'url', 'tel', 'password'])
const SELECT_ALL = new Set([...SELECTABLE, 'email', 'number'])
const TYPES = ['hidden', 'text', 'search', 'tel', 'url', 'email', 'password', 'date', 'month', 'week', 'time', 'datetime-local', 'number', 'range', 'color', 'checkbox', 'radio', 'file', 'submit', 'image', 'reset', 'button']

function formOwnerId(id: number): number {
  const attr = dom.localName(id) === 'img' ? null : dom.getAttribute(id, 'form')
  if (attr !== null) {
    const found = dom.getElementById(attr)
    return found !== NONE && dom.localName(found) === 'form' ? found : NONE
  }
  const parent = dom.parent(id)
  return parent === NONE ? NONE : dom.closest(parent, 'form')
}

function formOwner(element: Element): HTMLFormElement | null {
  return wrap<HTMLFormElement>(formOwnerId(element[ID]))
}

function labelsOf(element: Element): NodeList {
  const id = element[ID]
  const list: Node[] = []
  const root = dom.ancestors(id).at(-1) ?? id
  for (const label of dom.querySelectorAll(root, 'label')) {
    const control = wrap<HTMLLabelElement>(label)!.control
    if (control === element) list.push(wrap<Node>(label)!)
  }
  return staticNodeList(list)
}

export function disabledControl(element: Element): boolean {
  const id = element[ID]
  return dom.hasAttribute(id, 'disabled') || dom.closest(id, 'fieldset[disabled] *') !== NONE
}

const VALIDITY = ['valueMissing', 'typeMismatch', 'patternMismatch', 'tooShort', 'rangeUnderflow', 'rangeOverflow'] as const

let validityOwner: (state: ValidityState) => Element

export class ValidityState {
  declare readonly valueMissing: boolean
  declare readonly typeMismatch: boolean
  declare readonly patternMismatch: boolean
  declare readonly tooShort: boolean
  declare readonly rangeUnderflow: boolean
  declare readonly rangeOverflow: boolean
  readonly #owner: Element

  constructor(owner: Element) {
    this.#owner = owner
  }

  static {
    validityOwner = (state) => state.#owner
  }
  get customError(): boolean {
    return !!(this.#owner as unknown as { [CUSTOM_VALIDITY]?: string })[CUSTOM_VALIDITY]
  }
  get valid(): boolean {
    const v = validity(this.#owner)
    return !(VALIDITY.some((key) => v[key]) || this.customError)
  }
}
getters(ValidityState.prototype, { tooLong: false, stepMismatch: false, badInput: false })
for (const key of VALIDITY) {
  Object.defineProperty(ValidityState.prototype, key, {
    get(this: ValidityState) {
      return validity(validityOwner(this))[key]
    },
    configurable: true,
  })
}
tag(ValidityState)

function validity(element: Element): Record<(typeof VALIDITY)[number], boolean> {
  const result = { valueMissing: false, typeMismatch: false, patternMismatch: false, tooShort: false, rangeUnderflow: false, rangeOverflow: false }
  const id = element[ID]
  if (!willValidate(element)) return result
  const required = dom.hasAttribute(id, 'required')
  const local = element[LOCAL]
  if (local === 'select') {
    result.valueMissing = required && (element as unknown as HTMLSelectElement).value === ''
    return result
  }
  const value = (element as unknown as { value: string }).value
  if (local === 'input') {
    const type = (element as unknown as HTMLInputElement).type
    if (type === 'checkbox') result.valueMissing = required && !dom.checked(id)
    else if (type === 'radio') {
      const name = dom.getAttribute(id, 'name')
      result.valueMissing = required && !(name ? radioGroup(element as unknown as HTMLInputElement).some((r) => dom.checked(r)) : dom.checked(id))
    } else result.valueMissing = required && value === ''
    if (value !== '') {
      if (type === 'email') result.typeMismatch = !value.split(dom.hasAttribute(id, 'multiple') ? ',' : /(?!)/).every((v) => /^[^\s@]+@[^\s@]+$/.test(v.trim()))
      if (type === 'url') result.typeMismatch = !/^[a-zA-Z][a-zA-Z0-9+.-]*:\S*$/.test(value)
      const pattern = dom.getAttribute(id, 'pattern')
      if (pattern !== null) {
        try {
          result.patternMismatch = !new RegExp(`^(?:${pattern})$`, 'v').test(value)
        } catch {
        }
      }
      const min = parseInt(dom.getAttribute(id, 'minlength') ?? '', 10)
      if (!Number.isNaN(min)) result.tooShort = value.length < min
      if (type === 'number' || type === 'range') {
        const n = parseFloat(value), lo = parseFloat(dom.getAttribute(id, 'min') ?? ''), hi = parseFloat(dom.getAttribute(id, 'max') ?? '')
        result.rangeUnderflow = !Number.isNaN(lo) && n < lo
        result.rangeOverflow = !Number.isNaN(hi) && n > hi
      }
    }
  } else if (local === 'textarea') {
    result.valueMissing = required && value === ''
    const min = parseInt(dom.getAttribute(id, 'minlength') ?? '', 10)
    if (!Number.isNaN(min) && value !== '') result.tooShort = value.length < min
  }
  return result
}

function willValidate(element: Element): boolean {
  const id = element[ID]
  if (disabledControl(element) || dom.closest(id, 'datalist') !== NONE) return false
  const local = element[LOCAL]
  if (local === 'input') {
    const type = (element as unknown as HTMLInputElement).type
    return type !== 'hidden' && type !== 'reset' && type !== 'button' && !dom.hasAttribute(id, 'readonly')
  }
  if (local === 'button') return (element as unknown as HTMLButtonElement).type === 'submit'
  if (local === 'textarea') return !dom.hasAttribute(id, 'readonly')
  return local === 'select'
}

class ConstraintValidation {
  get validity(): ValidityState {
    return new ValidityState(this as unknown as Element)
  }
  get willValidate(): boolean {
    return willValidate(this as unknown as Element)
  }
  get validationMessage(): string {
    const custom = (this as { [CUSTOM_VALIDITY]?: string })[CUSTOM_VALIDITY]
    if (custom) return custom
    const v = validity(this as unknown as Element)
    return v.valueMissing ? 'Please fill out this field.' : v.typeMismatch ? 'Please enter a valid value.' : v.patternMismatch ? 'Please match the requested format.' : v.tooShort ? 'Please lengthen this text.' : v.rangeUnderflow || v.rangeOverflow ? 'Value out of range.' : ''
  }
  setCustomValidity(this: Element, message: string): void {
    ;(this as unknown as { [CUSTOM_VALIDITY]?: string })[CUSTOM_VALIDITY] = String(message)
  }
  checkValidity(this: Element): boolean {
    if (!willValidate(this) || new ValidityState(this).valid) return true
    fire(this, new Event('invalid', { cancelable: true }))
    return false
  }
  reportValidity(this: Element): boolean {
    return (this as unknown as ConstraintValidation).checkValidity.call(this)
  }
  get form(): HTMLFormElement | null {
    return formOwner(this as unknown as Element)
  }
  get labels(): NodeList {
    return labelsOf(this as unknown as Element)
  }
}

export function fieldSelection(field: Field): Selection {
  const holder = field as unknown as { [SELECTION]?: Selection }
  const length = field.value.length
  let s = holder[SELECTION]
  if (!s) {
    s = holder[SELECTION] = { start: length, end: length, direction: 'none' }
    dom.setSelectionRange(field[ID], length, length)
  }
  if (s.end > length) s.end = length
  if (s.start > s.end) s.start = s.end
  return s
}

function setSelection(field: Field, start: number, end: number, direction: string | undefined): void {
  const length = field.value.length
  const e = Math.min(Math.max(0, Math.trunc(end) || 0), length)
  let s = Math.min(Math.max(0, Math.trunc(start) || 0), length)
  if (s > e) s = e
  ;(field as unknown as { [SELECTION]?: Selection })[SELECTION] = { start: s, end: e, direction: direction === 'forward' || direction === 'backward' ? direction : 'none' }
  dom.setSelectionRange(field[ID], s, e)
}

export type TextField = Field

export function setFieldSelection(field: Field, start: number, end: number): void {
  setSelection(field, start, end, 'none')
}

export function replaceFieldText(field: Field, from: number, to: number, text: string): void {
  const value = field.value
  const insert = field[LOCAL] === 'textarea' ? text.replace(/\r\n?/g, '\n') : text.replace(/[\r\n]/g, '')
  const proto = field[LOCAL] === 'textarea' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype
  Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(field, value.slice(0, from) + insert + value.slice(to))
  setSelection(field, from + insert.length, from + insert.length, 'none')
}

function valueChanged(field: Field): void {
  const length = field.value.length
  ;(field as unknown as { [SELECTION]?: Selection })[SELECTION] = { start: length, end: length, direction: 'none' }
}

function currentSelection(field: unknown): Selection | null {
  return selectable(field as Field) ? fieldSelection(field as Field) : null
}

function changeableSelection(field: unknown): Selection {
  const s = currentSelection(field)
  if (!s) throw new DOMException(`The input element's type ('${(field as HTMLInputElement).type}') does not support selection.`, 'InvalidStateError')
  return s
}

class TextSelection {
  get selectionStart(): number | null {
    return currentSelection(this)?.start ?? null
  }
  set selectionStart(value: number) {
    const s = changeableSelection(this)
    setSelection(this as unknown as Field, value, Math.max(value, s.end), s.direction)
  }
  get selectionEnd(): number | null {
    return currentSelection(this)?.end ?? null
  }
  set selectionEnd(value: number) {
    const s = changeableSelection(this)
    setSelection(this as unknown as Field, Math.min(s.start, value), value, s.direction)
  }
  get selectionDirection(): string | null {
    return currentSelection(this)?.direction ?? null
  }
  set selectionDirection(value: string) {
    const s = fieldSelection(this as unknown as Field)
    setSelection(this as unknown as Field, s.start, s.end, value)
  }
  setSelectionRange(this: Field, start: number, end: number, direction?: string): void {
    if (!selectable(this)) throw new DOMException(`Failed to execute 'setSelectionRange' on 'HTMLInputElement': The input element's type ('${(this as HTMLInputElement).type}') does not support selection.`, 'InvalidStateError')
    setSelection(this, Number(start), Number(end), direction)
  }
  select(this: Field): void {
    if (this[LOCAL] !== 'textarea' && !SELECT_ALL.has((this as HTMLInputElement).type)) return
    setSelection(this, 0, this.value.length, 'none')
    queueTask(() => fire(this, new Event('select', { bubbles: true })))
  }
  setRangeText(this: Field, replacement: string, start?: number, end?: number, mode = 'preserve'): void {
    if (!selectable(this)) throw new DOMException("Failed to execute 'setRangeText': The input element's type does not support selection.", 'InvalidStateError')
    const s = fieldSelection(this)
    const value = this.value
    const from = Math.min(start ?? s.start, value.length), to = Math.min(end ?? s.end, value.length)
    if (from > to) throw new DOMException("Failed to execute 'setRangeText': The provided start value is larger than the end value.", 'IndexSizeError')
    const text = String(replacement)
    const oldStart = s.start, oldEnd = s.end
    dom.setValue(this[ID], value.slice(0, from) + text + value.slice(to))
    const newEnd = from + text.length
    if (mode === 'select') setSelection(this, from, newEnd, 'none')
    else if (mode === 'start') setSelection(this, from, from, 'none')
    else if (mode === 'end') setSelection(this, newEnd, newEnd, 'none')
    else {
      const delta = text.length - (to - from)
      const shift = (p: number) => (p > to ? p + delta : p > from ? from : p)
      setSelection(this, shift(oldStart), oldEnd > to ? oldEnd + delta : oldEnd > from ? newEnd : oldEnd, s.direction)
    }
  }
}

function selectable(field: Field): boolean {
  return field[LOCAL] === 'textarea' || SELECTABLE.has((field as HTMLInputElement).type)
}

function inputType(id: number): string {
  const raw = dom.getAttribute(id, 'type')
  if (raw === null) return 'text'
  const lower = raw.toLowerCase()
  return TYPES.includes(lower) ? lower : 'text'
}

function sanitize(type: string, value: string): string {
  switch (type) {
    case 'text':
    case 'search':
    case 'tel':
    case 'password': return value.replace(/[\r\n]/g, '')
    case 'url':
    case 'email': return value.replace(/[\r\n]/g, '').trim()
    case 'number': return value === '' || Number.isFinite(Number(value)) ? value : ''
    case 'color': return /^#[0-9a-f]{6}$/i.test(value) ? value.toLowerCase() : '#000000'
    default: return value
  }
}

function radioGroup(input: HTMLInputElement): number[] {
  const id = input[ID]
  const name = dom.getAttribute(id, 'name')
  if (!name) return [id]
  const form = formOwner(input)
  const root = form ? form[ID] : (dom.ancestors(id).at(-1) ?? id)
  const all = dom.querySelectorAll(root, `input[name=${cssString(name)}]`)
  const out: number[] = []
  for (const other of all) {
    if (inputType(other) !== 'radio') continue
    if (!form && dom.closest(other, 'form') !== NONE) continue
    if (form && formOwner(wrap<Element>(other)!) !== form) continue
    out.push(other)
  }
  if (!out.includes(id)) out.push(id)
  return out
}

function setChecked(input: HTMLInputElement, checked: boolean): void {
  const id = input[ID]
  if (checked && input.type === 'radio') for (const other of radioGroup(input)) if (other !== id && dom.checked(other)) dom.setChecked(other, false)
  dom.setChecked(id, checked)
}

const FILES = Symbol('files')

function filesOf(input: HTMLInputElement): File[] {
  return (input as unknown as { [FILES]?: { files: File[] } })[FILES]?.files ?? []
}

function setFiles(input: HTMLInputElement, files: File[]): void {
  ;(input as unknown as { [FILES]?: { files: File[]; list: unknown } })[FILES] = { files, list: null }
}

const picking = new WeakSet<object>()

function fireChange(target: Element): void {
  fire(target, new Event('input', { bubbles: true, composed: true }))
  fire(target, new Event('change', { bubbles: true }))
}

function pickFiles(input: HTMLInputElement): void {
  if (picking.has(input)) return
  const chooser = env.files
  const cancel = () => fire(input, new Event('cancel', { bubbles: true }))
  if (!chooser) return void host.setTimer(0, cancel)
  picking.add(input)
  const accept = dom.getAttribute(input[ID], 'accept') ?? ''
  void chooser
    .open({ multiple: dom.hasAttribute(input[ID], 'multiple'), directory: false, filters: acceptFilters(accept) })
    .then(async (picked) => {
      if (!picked || picked.length === 0) return cancel()
      setFiles(input, await Promise.all(picked.map(readPicked)))
      fireChange(input)
    })
    .catch((error: unknown) => {
      host.print(1, `acde-dom: file chooser: ${(error as Error)?.message ?? String(error)}`)
      cancel()
    })
    .finally(() => picking.delete(input))
}

export class HTMLInputElement extends HTMLElement {
  get type(): string {
    return inputType(this[ID])
  }
  set type(value: string) {
    setAttribute(this, 'type', String(value))
  }

  get value(): string {
    const type = this.type
    if (VALUE_MODE.has(type)) return dom.value(this[ID])
    if (DEFAULT_MODE.has(type)) return dom.getAttribute(this[ID], 'value') ?? ''
    if (type === 'checkbox' || type === 'radio') return dom.getAttribute(this[ID], 'value') ?? 'on'
    const first = type === 'file' ? filesOf(this)[0] : undefined
    return first ? `C:\\fakepath\\${first.name}` : ''
  }
  set value(value: string) {
    const type = this.type
    const text = value === null ? '' : String(value)
    if (VALUE_MODE.has(type)) {
      const next = sanitize(type, text)
      if (next === dom.value(this[ID])) {
        dom.setValue(this[ID], next)
        return
      }
      dom.setValue(this[ID], next)
      valueChanged(this)
    } else if (type === 'file') {
      if (text !== '') throw new DOMException("Failed to set the 'value' property on 'HTMLInputElement': This input element accepts a filename, which may only be programmatically set to the empty string.", 'InvalidStateError')
      setFiles(this, [])
    } else setAttribute(this, 'value', text)
  }

  get defaultValue(): string {
    return dom.getAttribute(this[ID], 'value') ?? ''
  }
  set defaultValue(value: string) {
    setAttribute(this, 'value', String(value))
  }

  get checked(): boolean {
    return dom.checked(this[ID])
  }
  set checked(value: boolean) {
    setChecked(this, !!value)
  }

  get defaultChecked(): boolean {
    return dom.hasAttribute(this[ID], 'checked')
  }
  set defaultChecked(value: boolean) {
    setAttribute(this, 'checked', value ? '' : null)
  }

  get indeterminate(): boolean {
    return !!(this as unknown as { [INDETERMINATE]?: boolean })[INDETERMINATE]
  }
  set indeterminate(value: boolean) {
    ;(this as unknown as { [INDETERMINATE]?: boolean })[INDETERMINATE] = !!value
  }

  get valueAsNumber(): number {
    const type = this.type
    const value = this.value
    if (type === 'number' || type === 'range') return value === '' ? NaN : Number(value)
    if (type === 'date' || type === 'datetime-local' || type === 'month' || type === 'week') return value ? Date.parse(value) : NaN
    if (type === 'time') {
      const m = /^(\d\d):(\d\d)(?::(\d\d(?:\.\d+)?))?$/.exec(value)
      return m ? (Number(m[1]) * 3600 + Number(m[2]) * 60 + Number(m[3] ?? 0)) * 1000 : NaN
    }
    return NaN
  }
  set valueAsNumber(value: number) {
    if (this.type === 'number' || this.type === 'range') this.value = Number.isFinite(value) ? String(value) : ''
    else throw new DOMException("Failed to set the 'valueAsNumber' property on 'HTMLInputElement': This input element does not support Number values.", 'InvalidStateError')
  }

  get valueAsDate(): Date | null {
    const n = this.valueAsNumber
    return (this.type === 'date' || this.type === 'datetime-local') && Number.isFinite(n) ? new Date(n) : null
  }
  set valueAsDate(value: Date | null) {
    this.value = value ? value.toISOString().slice(0, this.type === 'date' ? 10 : 16) : ''
  }

  get files(): unknown {
    if (this.type !== 'file') return null
    const holder = this as unknown as { [FILES]?: { files: File[]; list: unknown } }
    const held = (holder[FILES] ??= { files: [], list: null })
    return (held.list ??= new FileList(held.files.slice()))
  }
  set files(value: unknown) {
    if (this.type !== 'file' || !(value instanceof FileList)) return
    const files: File[] = []
    for (let i = 0; i < value.length; i++) files.push(value.item(i)!)
    setFiles(this, files)
  }

  get list(): Element | null {
    const attr = dom.getAttribute(this[ID], 'list')
    if (!attr) return null
    const found = dom.getElementById(attr)
    return found !== NONE && dom.localName(found) === 'datalist' ? wrap<Element>(found) : null
  }

  stepUp(n = 1): void {
    this.#step(n)
  }
  stepDown(n = 1): void {
    this.#step(-n)
  }
  #step(n: number): void {
    if (this.type !== 'number' && this.type !== 'range') throw new DOMException("Failed to execute 'stepUp' on 'HTMLInputElement': This form element is not steppable.", 'InvalidStateError')
    const step = parseFloat(dom.getAttribute(this[ID], 'step') ?? '1') || 1
    let next = (Number(this.value) || 0) + n * step
    const min = parseFloat(dom.getAttribute(this[ID], 'min') ?? ''), max = parseFloat(dom.getAttribute(this[ID], 'max') ?? '')
    if (!Number.isNaN(min)) next = Math.max(min, next)
    if (!Number.isNaN(max)) next = Math.min(max, next)
    this.value = String(next)
  }

  showPicker(): void {
    if (disabledControl(this) || dom.hasAttribute(this[ID], 'readonly')) throw new DOMException("Failed to execute 'showPicker' on 'HTMLInputElement': HTMLInputElement::showPicker() cannot be used on immutable controls.", 'InvalidStateError')
    if (this.type === 'file') pickFiles(this)
  }

  [ACTIVATION](): { run(): void; cancel(): void } | null {
    if (disabledControl(this)) return null
    const type = this.type
    if (type === 'file') return activation(() => pickFiles(this))
    if ((type === 'button' || ((type === 'submit' || type === 'reset' || type === 'image') && !this.form)) && popoverTarget(this as never)) {
      return activation(() => invokePopover(this as never))
    }
    if (type === 'checkbox') {
      const was = this.checked, wasIndeterminate = this.indeterminate
      setChecked(this, !was)
      this.indeterminate = false
      return {
        run: () => this.isConnected && fireChange(this),
        cancel: () => {
          setChecked(this, was)
          this.indeterminate = wasIndeterminate
        },
      }
    }
    if (type === 'radio') {
      const was = this.checked
      const previous = was ? NONE : (radioGroup(this).find((id) => id !== this[ID] && dom.checked(id)) ?? NONE)
      setChecked(this, true)
      return {
        run: () => !was && this.isConnected && fireChange(this),
        cancel: () => {
          if (was) return
          setChecked(this, false)
          if (previous !== NONE) dom.setChecked(previous, true)
        },
      }
    }
    if (type === 'submit' || type === 'image') return activation(() => submitFrom(this))
    if (type === 'reset') return activation(() => resetFrom(this))
    return null
  }

  override [ATTRIBUTE_CHANGED](name: string, value: string | null): void {
    super[ATTRIBUTE_CHANGED](name, value)
    if (name === 'type') {
      const holder = this as unknown as { [SELECTION]?: Selection }
      holder[SELECTION] = undefined
    }
  }
}
for (const name of ['name', 'placeholder', 'min', 'max', 'step', 'pattern', 'autocomplete', 'accept', 'alt', 'dirName', 'formAction', 'formEnctype', 'formMethod', 'formTarget', 'useMap', 'align']) string(HTMLInputElement.prototype, name, name === 'dirName' ? 'dirname' : name.toLowerCase())
for (const name of ['disabled', 'required', 'readOnly', 'multiple', 'formNoValidate', 'webkitdirectory']) bool(HTMLInputElement.prototype, name, name.toLowerCase())
long(HTMLInputElement.prototype, 'maxLength', -1, 'maxlength', 0)
long(HTMLInputElement.prototype, 'minLength', -1, 'minlength', 0)
long(HTMLInputElement.prototype, 'size', 20, 'size', 1)
long(HTMLInputElement.prototype, 'width', 0)
long(HTMLInputElement.prototype, 'height', 0)
url(HTMLInputElement.prototype, 'src')
mixin(HTMLInputElement, ConstraintValidation)
mixin(HTMLInputElement, TextSelection)
register(HTMLInputElement, 'input')
export interface HTMLInputElement extends ConstraintValidation, TextSelection {}

export class HTMLTextAreaElement extends HTMLElement {
  get value(): string {
    return dom.value(this[ID])
  }
  set value(value: string) {
    const text = (value === null ? '' : String(value)).replace(/\r\n?/g, '\n')
    if (text === dom.value(this[ID])) return
    dom.setValue(this[ID], text)
    valueChanged(this)
  }
  get defaultValue(): string {
    return dom.textContent(this[ID])
  }
  set defaultValue(value: string) {
    this.textContent = String(value)
  }
  get textLength(): number {
    return this.value.length
  }
}
getters(HTMLTextAreaElement.prototype, { type: 'textarea' })
for (const name of ['name', 'placeholder', 'autocomplete', 'wrap', 'dirName']) string(HTMLTextAreaElement.prototype, name, name.toLowerCase())
for (const name of ['disabled', 'required', 'readOnly']) bool(HTMLTextAreaElement.prototype, name, name.toLowerCase())
long(HTMLTextAreaElement.prototype, 'maxLength', -1, 'maxlength', 0)
long(HTMLTextAreaElement.prototype, 'minLength', -1, 'minlength', 0)
long(HTMLTextAreaElement.prototype, 'rows', 2, 'rows', 1)
long(HTMLTextAreaElement.prototype, 'cols', 20, 'cols', 1)
mixin(HTMLTextAreaElement, ConstraintValidation)
mixin(HTMLTextAreaElement, TextSelection)
register(HTMLTextAreaElement, 'textarea')
export interface HTMLTextAreaElement extends ConstraintValidation, TextSelection {}

const SELECT = Symbol('select')

export class HTMLOptionsCollection extends HTMLCollection {
  [SELECT]: HTMLSelectElement | null = null

  get selectedIndex(): number {
    return this[SELECT] ? this[SELECT].selectedIndex : -1
  }
  set selectedIndex(value: number) {
    if (this[SELECT]) this[SELECT].selectedIndex = value
  }

  add(element: Element, before?: Element | number | null): void {
    this[SELECT]?.add(element, before)
  }

  remove(index: number): void {
    this[SELECT]?.remove(index)
  }

  override get length(): number {
    return liveIds(this).length
  }
  override set length(value: number) {
    if (this[SELECT]) setOptionCount(this[SELECT], value)
  }
}
tag(HTMLOptionsCollection)
listShape(HTMLOptionsCollection, collectionShape<HTMLOptionsCollection>({
  setItem: (list, index, value) => {
    if (list[SELECT]) setOption(list[SELECT], index, value)
  },
}))

const OPTIONS = Symbol('options')

function optionIds(select: number): Uint32Array {
  return dom.options(select)
}

function setOptionCount(select: HTMLSelectElement, value: number): void {
  const ids = optionIds(select[ID])
  const count = Number(value) >>> 0
  if (count > ids.length) {
    if (count > 100000) return
    const fragment = new DocumentFragment()
    for (let i = ids.length; i < count; i++) fragment.appendChild(createHtml('option'))
    select.appendChild(fragment)
    return
  }
  for (let i = ids.length - 1; i >= count; i--) wrap<Element>(ids[i]!)!.remove()
}

function setOption(select: HTMLSelectElement, index: number, value: unknown): void {
  if (value === null || value === undefined) return select.remove(index)
  if (!(value instanceof HTMLOptionElement)) throw new TypeError("Failed to set an indexed property on 'HTMLSelectElement': parameter 2 is not of type 'HTMLOptionElement'.")
  const ids = optionIds(select[ID])
  if (index > ids.length) setOptionCount(select, index)
  if (index >= ids.length) select.appendChild(value)
  else wrap<Element>(ids[index]!)!.replaceWith(value)
}

const selectShape = legacy<HTMLSelectElement>({
  identity: true,
  length: (select) => optionIds(select[ID]).length,
  item: (select, index) => wrap<Element>(optionIds(select[ID])[index] ?? NONE),
  setItem: setOption,
})

export class HTMLSelectElement extends HTMLElement {
  constructor() {
    exotic(selectShape)
    super()
  }

  get type(): string {
    return dom.hasAttribute(this[ID], 'multiple') ? 'select-multiple' : 'select-one'
  }

  get options(): HTMLOptionsCollection {
    const holder = this as unknown as { [OPTIONS]?: HTMLOptionsCollection }
    if (!holder[OPTIONS]) {
      const id = this[ID]
      const list = liveCollection(() => optionIds(id), HTMLOptionsCollection)
      list[SELECT] = this
      holder[OPTIONS] = list
    }
    return holder[OPTIONS]
  }

  get length(): number {
    return optionIds(this[ID]).length
  }
  set length(value: number) {
    setOptionCount(this, value)
  }

  item(index: number): Element | null {
    return wrap<Element>(optionIds(this[ID])[Number(index) >>> 0] ?? NONE)
  }

  namedItem(name: string): Element | null {
    return this.options.namedItem(name)
  }

  get selectedIndex(): number {
    return dom.selectedIndex(this[ID])
  }
  set selectedIndex(value: number) {
    const index = Math.trunc(Number(value))
    dom.setSelectedIndex(this[ID], Number.isFinite(index) ? index : -1)
  }

  get value(): string {
    const index = this.selectedIndex
    if (index < 0) return ''
    return (this.item(index) as HTMLOptionElement | null)?.value ?? ''
  }
  set value(value: string) {
    const text = String(value)
    this.selectedIndex = optionIds(this[ID]).findIndex((id) => wrap<HTMLOptionElement>(id)!.value === text)
  }

  get selectedOptions(): HTMLCollection {
    const id = this[ID]
    return liveCollection(() => optionIds(id).filter((o) => dom.optionSelected(o)))
  }

  add(element: Element, before?: Element | number | null): void {
    const ref = typeof before === 'number' ? this.item(before) : (before ?? null)
    const parent = ref ? ref.parentNode! : this
    parent.insertBefore(element, ref)
  }

  override remove(index?: number): void {
    if (index === undefined) return super.remove()
    ;(this.item(index) as Element | null)?.remove()
  }

  showPicker(): void {
    if (disabledControl(this)) throw new DOMException("Failed to execute 'showPicker' on 'HTMLSelectElement': HTMLSelectElement::showPicker() cannot be used on immutable controls.", 'InvalidStateError')
    if (!this.isConnected || this.type === 'select-multiple' || this.size > 1) return
    dom.openPicker(this[ID])
  }
}
for (const name of ['name', 'autocomplete']) string(HTMLSelectElement.prototype, name)
for (const name of ['disabled', 'required', 'multiple']) bool(HTMLSelectElement.prototype, name)
long(HTMLSelectElement.prototype, 'size', 0, 'size', 0)
mixin(HTMLSelectElement, ConstraintValidation)
register(HTMLSelectElement, 'select')
export interface HTMLSelectElement extends ConstraintValidation {
  name: string
  autocomplete: string
  disabled: boolean
  required: boolean
  multiple: boolean
  size: number
}

function ownerSelect(option: number): number {
  const parent = dom.parent(option)
  if (parent === NONE) return NONE
  if (dom.localName(parent) === 'select') return parent
  if (dom.localName(parent) === 'optgroup') {
    const grand = dom.parent(parent)
    return grand !== NONE && dom.localName(grand) === 'select' ? grand : NONE
  }
  return NONE
}

export class HTMLOptionElement extends HTMLElement {
  get value(): string {
    return dom.getAttribute(this[ID], 'value') ?? this.text
  }
  set value(value: string) {
    setAttribute(this, 'value', String(value))
  }

  get text(): string {
    return dom.textContent(this[ID]).replace(/[\t\n\f\r ]+/g, ' ').trim()
  }
  set text(value: string) {
    this.textContent = String(value)
  }

  get label(): string {
    return dom.getAttribute(this[ID], 'label') ?? this.text
  }
  set label(value: string) {
    setAttribute(this, 'label', String(value))
  }

  get index(): number {
    const select = ownerSelect(this[ID])
    if (select === NONE) return 0
    return optionIds(select).indexOf(this[ID])
  }

  get selected(): boolean {
    return dom.optionSelected(this[ID])
  }
  set selected(value: boolean) {
    dom.setOptionSelected(this[ID], !!value)
  }

  get defaultSelected(): boolean {
    return dom.hasAttribute(this[ID], 'selected')
  }
  set defaultSelected(value: boolean) {
    setAttribute(this, 'selected', value ? '' : null)
  }

  get form(): HTMLFormElement | null {
    const select = ownerSelect(this[ID])
    return select === NONE ? null : formOwner(wrap<Element>(select)!)
  }
}
bool(HTMLOptionElement.prototype, 'disabled')
register(HTMLOptionElement, 'option')
export interface HTMLOptionElement {
  disabled: boolean
}

export class HTMLOptGroupElement extends HTMLElement {}
bool(HTMLOptGroupElement.prototype, 'disabled')
string(HTMLOptGroupElement.prototype, 'label')
register(HTMLOptGroupElement, 'optgroup')

export class HTMLButtonElement extends HTMLElement {
  get type(): string {
    const raw = dom.getAttribute(this[ID], 'type')?.toLowerCase()
    return raw === 'button' || raw === 'reset' ? raw : 'submit'
  }
  set type(value: string) {
    setAttribute(this, 'type', String(value))
  }

  [ACTIVATION](): { run(): void; cancel(): void } | null {
    if (disabledControl(this)) return null
    const type = this.type
    if (type === 'submit' && this.form) return activation(() => submitFrom(this))
    if (type === 'reset' && this.form) return activation(() => resetFrom(this))
    if (popoverTarget(this as never)) return activation(() => invokePopover(this as never))
    if (type === 'submit') return activation(() => submitFrom(this))
    if (type === 'reset') return activation(() => resetFrom(this))
    return activation(() => {})
  }
}
for (const name of ['name', 'value', 'formAction', 'formEnctype', 'formMethod', 'formTarget', 'popoverTargetAction', 'command']) string(HTMLButtonElement.prototype, name, name.toLowerCase())
string(HTMLInputElement.prototype, 'popoverTargetAction', 'popovertargetaction')
for (const proto of [HTMLButtonElement.prototype, HTMLInputElement.prototype]) {
  Object.defineProperty(proto, 'popoverTargetElement', {
    configurable: true,
    enumerable: true,
    get(this: HTMLElement) {
      return popoverTarget(this as never)
    },
    set(this: HTMLElement, element: Element | null) {
      if (element === null) setAttribute(this, 'popovertarget', null)
      else setAttribute(this, 'popovertarget', element.id)
    },
  })
}
for (const name of ['disabled', 'formNoValidate']) bool(HTMLButtonElement.prototype, name, name.toLowerCase())
mixin(HTMLButtonElement, ConstraintValidation)
register(HTMLButtonElement, 'button')
export interface HTMLButtonElement extends ConstraintValidation {}

const LISTED = 'button, fieldset, input, object, output, select, textarea'
const FORM_LISTED = 'button, fieldset, input:not([type="image" i]), object, output, select, textarea'

function radio(id: number): boolean {
  return dom.localName(id) === 'input' && dom.getAttribute(id, 'type')?.toLowerCase() === 'radio'
}

export class RadioNodeList extends NodeList {
  get value(): string {
    const ids = liveIds(this)
    for (let i = 0; i < ids.length; i++) if (radio(ids[i]!) && dom.checked(ids[i]!)) return dom.getAttribute(ids[i]!, 'value') ?? 'on'
    return ''
  }
  set value(value: string) {
    const text = String(value)
    const ids = liveIds(this)
    for (let i = 0; i < ids.length; i++) {
      if (!radio(ids[i]!)) continue
      const own = dom.getAttribute(ids[i]!, 'value')
      if (own === text || (own === null && text === 'on')) {
        wrap<HTMLInputElement>(ids[i]!)!.checked = true
        return
      }
    }
  }
}
tag(RadioNodeList)

export class HTMLFormControlsCollection extends HTMLCollection {
  override namedItem(name: string): Element | null {
    const key = String(name)
    if (!key) return null
    const ids = liveIds(this)
    let first = NONE
    for (let i = 0; i < ids.length; i++) {
      if (!namedBy(ids[i]!, key)) continue
      if (first !== NONE) return liveCollection(() => liveIds(this).filter((id) => namedBy(id, key)), RadioNodeList) as unknown as Element
      first = ids[i]!
    }
    return wrap<Element>(first)
  }
}
tag(HTMLFormControlsCollection)
listShape(HTMLFormControlsCollection, collectionShape())

const ELEMENTS = Symbol('elements')
const NAMED = Symbol('named')
const PAST = Symbol('pastNames')

function owned(form: number, selector: string): Uint32Array {
  const inside = dom.querySelectorAll(form, selector)
  if (!flags.formAttributes) return inside
  const own = dom.getAttribute(form, 'id')
  const result: number[] = []
  for (const id of inside) if (!dom.hasAttribute(id, 'form') || dom.getAttribute(id, 'form') === own) result.push(id)
  if (own) {
    let outside = false
    for (const id of dom.querySelectorAll(documentId, `[form=${cssString(own)}]`)) {
      if (result.includes(id) || !dom.matches(id, selector)) continue
      result.push(id)
      outside = true
    }
    if (outside) result.sort((a, b) => (a === b ? 0 : dom.compareDocumentPosition(a, b) & 4 ? -1 : 1))
  }
  return Uint32Array.from(result)
}

function formControls(form: number): Uint32Array {
  return owned(form, FORM_LISTED)
}

type Named = Element | RadioNodeList

function candidates(form: number, key: string): Uint32Array {
  const quoted = cssString(key)
  const found = owned(form, `:is(${FORM_LISTED}, img):is([id=${quoted}], [name=${quoted}])`)
  if (found.length === 0) return found
  const listed = found.filter((id) => dom.localName(id) !== 'img')
  return listed.length ? listed : found
}

function formNamed(form: HTMLFormElement, key: string): Named | undefined {
  if (!key) return undefined
  let cache = form[NAMED]
  if (!cache) cache = form[NAMED] = { version, found: new Map() }
  else if (cache.version !== version) {
    cache.version = version
    cache.found.clear()
    const past = form[PAST]
    if (past) for (const [name, element] of past) if (formOwnerId(element[ID]) !== form[ID]) past.delete(name)
  }
  let found = cache.found.get(key)
  if (found === undefined) {
    found = formLookup(form, key)
    cache.found.set(key, found)
  }
  return found ?? undefined
}

function formLookup(form: HTMLFormElement, key: string): Named | null {
  const id = form[ID]
  const ids = candidates(id, key)
  if (ids.length === 0) return form[PAST]?.get(key) ?? null
  if (ids.length > 1) return liveCollection(() => candidates(id, key), RadioNodeList)
  const element = wrap<Element>(ids[0]!)!
  ;(form[PAST] ??= new Map()).set(key, element)
  return element
}

function formNames(form: HTMLFormElement): string[] {
  const names = namesOf(owned(form[ID], `${FORM_LISTED}, img`))
  const past = form[PAST]
  return past ? names.concat([...past.keys()]) : names
}

function elementsOf(form: HTMLFormElement): HTMLFormControlsCollection {
  const id = form[ID]
  return (form[ELEMENTS] ??= liveCollection(() => formControls(id), HTMLFormControlsCollection))
}

const formShape = legacy<HTMLFormElement>({
  identity: true,
  override: true,
  length: (form) => liveIds(elementsOf(form)).length,
  item: (form, index) => {
    const ids = liveIds(elementsOf(form))
    return index < ids.length ? wrap<Element>(ids[index]!) : null
  },
  named: formNamed,
  names: formNames,
})

function formMethod(form: number): string {
  const raw = dom.getAttribute(form, 'method')?.toLowerCase()
  return raw === 'post' || raw === 'dialog' ? raw : 'get'
}

function formValid(form: number): boolean {
  let valid = true
  for (const id of formControls(form)) {
    const control = wrap<Element>(id)! as unknown as ConstraintValidation & Element
    if (typeof control.checkValidity === 'function' && !control.checkValidity.call(control)) valid = false
  }
  return valid
}

function submitForm(form: HTMLFormElement, submitter: Element | null): void {
  const id = form[ID]
  if (submitter && formOwnerId(submitter[ID]) !== id) throw new DOMException("Failed to execute 'requestSubmit' on 'HTMLFormElement': The specified element is not owned by this form element.", 'NotFoundError')
  const noValidate = dom.hasAttribute(id, 'novalidate') || (submitter ? dom.hasAttribute(submitter[ID], 'formnovalidate') : false)
  if (!noValidate && !formValid(id)) return
  const event = new SubmitEvent('submit', { bubbles: true, cancelable: true, submitter })
  if (!fire(form, event)) return
  if (formMethod(id) === 'dialog') {
    const dialog = dom.closest(id, 'dialog')
    if (dialog !== NONE) (wrap<HTMLElement>(dialog) as unknown as { close(v?: string): void }).close(submitter ? ((submitter as unknown as { value?: string }).value ?? undefined) : undefined)
  }
}

function resetForm(form: HTMLFormElement): void {
  if (!fire(form, new Event('reset', { bubbles: true, cancelable: true }))) return
  for (const id of formControls(form[ID])) {
    const control = wrap<Element>(id)!
    const local = control[LOCAL]
    if (local === 'input') {
      const input = control as unknown as HTMLInputElement
      dom.setValue(id, dom.getAttribute(id, 'value') ?? '')
      dom.setChecked(id, dom.hasAttribute(id, 'checked'))
      input.indeterminate = false
    } else if (local === 'textarea') dom.setValue(id, dom.textContent(id))
    else if (local === 'select') dom.resetSelect(id)
  }
}

function submitFrom(submitter: Element): void {
  const form = formOwner(submitter)
  if (form) submitForm(form, submitter)
}

function resetFrom(control: Element): void {
  const form = formOwner(control)
  if (form) resetForm(form)
}

export function submitImplicitly(field: Element): void {
  const form = formOwner(field)
  if (form) submitForm(form, null)
}

export class HTMLFormElement extends HTMLElement {
  [ELEMENTS]?: HTMLFormControlsCollection;
  [NAMED]?: { version: number; found: Map<string, Named | null> };
  [PAST]?: Map<string, Element>

  constructor() {
    exotic(formShape)
    super()
  }

  get elements(): HTMLFormControlsCollection {
    return elementsOf(this)
  }

  get length(): number {
    return liveIds(elementsOf(this)).length
  }

  get relList(): DOMTokenList {
    return tokenList(this, Symbol.for('acde.relList'), 'rel', ['noreferrer', 'noopener', 'opener'])
  }

  get action(): string {
    const raw = dom.getAttribute(this[ID], 'action')
    return raw === null || raw === '' ? resolveUrl('') : resolveUrl(raw)
  }
  set action(value: string) {
    setAttribute(this, 'action', String(value))
  }

  get method(): string {
    return formMethod(this[ID])
  }
  set method(value: string) {
    setAttribute(this, 'method', String(value))
  }

  submit(): void {}

  requestSubmit(submitter?: Element | null): void {
    submitForm(this, submitter ?? null)
  }

  reset(): void {
    resetForm(this)
  }

  checkValidity(): boolean {
    return formValid(this[ID])
  }

  reportValidity(): boolean {
    return formValid(this[ID])
  }
}
for (const name of ['name', 'target', 'acceptCharset', 'autocomplete', 'rel']) string(HTMLFormElement.prototype, name, name === 'acceptCharset' ? 'accept-charset' : name.toLowerCase())
for (const name of ['enctype', 'encoding']) string(HTMLFormElement.prototype, name, 'enctype')
bool(HTMLFormElement.prototype, 'noValidate', 'novalidate')
register(HTMLFormElement, 'form')

const LABELABLE = 'button, input:not([type="hidden" i]), meter, output, progress, select, textarea'

export class HTMLLabelElement extends HTMLElement {
  get htmlFor(): string {
    return dom.getAttribute(this[ID], 'for') ?? ''
  }
  set htmlFor(value: string) {
    setAttribute(this, 'for', String(value))
  }

  get control(): HTMLElement | null {
    const target = dom.getAttribute(this[ID], 'for')
    if (target !== null) {
      const id = dom.getElementById(target)
      return id !== NONE && dom.matches(id, LABELABLE) && dom.ancestors(id).at(-1) === dom.ancestors(this[ID]).at(-1) ? wrap<HTMLElement>(id) : null
    }
    return wrap<HTMLElement>(dom.querySelector(this[ID], LABELABLE))
  }

  get form(): HTMLFormElement | null {
    const control = this.control
    return control ? formOwner(control) : null
  }

  [ACTIVATION](event: Event): { run(): void; cancel(): void } | null {
    const control = this.control
    const target = event.target as Node | null
    if (!control || (target && inner(control, 'contains').call(control, target))) return null
    if (target && target !== this && (target as unknown as Element)[ID] !== undefined && dom.closest((target as unknown as Element)[ID], 'a[href], button, input, select, textarea, label') !== this[ID]) return null
    return activation(() => {
      if (focusable(control[ID])) setFocus(control)
      if (control[LOCAL] !== 'textarea' && !(control[LOCAL] === 'input' && SELECTABLE.has((control as unknown as HTMLInputElement).type))) inner(control, 'click').call(control)
    })
  }
}
register(HTMLLabelElement, 'label')

export class HTMLFieldSetElement extends HTMLElement {
  get elements(): HTMLCollection {
    const id = this[ID]
    const holder = this as unknown as { [ELEMENTS]?: HTMLCollection }
    return (holder[ELEMENTS] ??= liveCollection(() => dom.querySelectorAll(id, LISTED)))
  }
}
getters(HTMLFieldSetElement.prototype, { type: 'fieldset' })
bool(HTMLFieldSetElement.prototype, 'disabled')
string(HTMLFieldSetElement.prototype, 'name')
mixin(HTMLFieldSetElement, ConstraintValidation)
register(HTMLFieldSetElement, 'fieldset')

export class HTMLLegendElement extends HTMLElement {
  get form(): HTMLFormElement | null {
    const fieldset = dom.parent(this[ID])
    return fieldset !== NONE && dom.localName(fieldset) === 'fieldset' ? formOwner(wrap<Element>(fieldset)!) : null
  }
}
string(HTMLLegendElement.prototype, 'align')
register(HTMLLegendElement, 'legend')

export class HTMLOutputElement extends HTMLElement {
  #default: string | null = null
  get value(): string {
    return dom.textContent(this[ID])
  }
  set value(value: string) {
    this.#default ??= dom.textContent(this[ID])
    this.textContent = String(value)
  }
  get defaultValue(): string {
    return this.#default ?? dom.textContent(this[ID])
  }
  set defaultValue(value: string) {
    if (this.#default === null) this.textContent = String(value)
    else this.#default = String(value)
  }
  get htmlFor(): DOMTokenList {
    return tokenList(this, Symbol.for('acde.htmlFor'), 'for')
  }
}
getters(HTMLOutputElement.prototype, { type: 'output' })
string(HTMLOutputElement.prototype, 'name')
mixin(HTMLOutputElement, ConstraintValidation)
register(HTMLOutputElement, 'output')

function numberSetters(cls: { prototype: object }, names: readonly string[]): void {
  for (const name of names) {
    const get = Object.getOwnPropertyDescriptor(cls.prototype, name)!.get
    Object.defineProperty(cls.prototype, name, { get, set(this: Element, v: unknown) { setAttribute(this, name, String(Number(v))) }, configurable: true })
  }
}

export class HTMLProgressElement extends HTMLElement {
  get value(): number {
    const v = parseFloat(dom.getAttribute(this[ID], 'value') ?? '')
    return Number.isFinite(v) && v > 0 ? Math.min(v, this.max) : 0
  }
  get max(): number {
    const v = parseFloat(dom.getAttribute(this[ID], 'max') ?? '')
    return Number.isFinite(v) && v > 0 ? v : 1
  }
  get position(): number {
    return dom.hasAttribute(this[ID], 'value') ? this.value / this.max : -1
  }
}
numberSetters(HTMLProgressElement, ['value', 'max'])
mixin(HTMLProgressElement, ConstraintValidation, ['labels'])
register(HTMLProgressElement, 'progress')

export class HTMLMeterElement extends HTMLElement {
  #num(name: string, fallback: number): number {
    const v = parseFloat(dom.getAttribute(this[ID], name) ?? '')
    return Number.isFinite(v) ? v : fallback
  }
  get min(): number {
    return this.#num('min', 0)
  }
  get max(): number {
    return Math.max(this.min, this.#num('max', 1))
  }
  get value(): number {
    return Math.min(Math.max(this.#num('value', 0), this.min), this.max)
  }
  get low(): number {
    return Math.min(Math.max(this.#num('low', this.min), this.min), this.max)
  }
  get high(): number {
    return Math.min(Math.max(this.#num('high', this.max), this.low), this.max)
  }
  get optimum(): number {
    return Math.min(Math.max(this.#num('optimum', (this.min + this.max) / 2), this.min), this.max)
  }
}
numberSetters(HTMLMeterElement, ['min', 'max', 'value', 'low', 'high', 'optimum'])
mixin(HTMLMeterElement, ConstraintValidation, ['labels'])
register(HTMLMeterElement, 'meter')

type Entry = [string, string | File]

export class FormData {
  #entries: Entry[] = []

  constructor(form?: HTMLFormElement, submitter?: Element | null) {
    if (form === undefined) return
    if (!(form instanceof HTMLFormElement)) throw new TypeError("Failed to construct 'FormData': parameter 1 is not of type 'HTMLFormElement'.")
    for (const id of formControls(form[ID])) {
      const control = wrap<Element>(id)!
      if (disabledControl(control) || dom.closest(id, 'datalist') !== NONE) continue
      const name = dom.getAttribute(id, 'name')
      const local = control[LOCAL]
      if (local === 'button') {
        if (control === submitter && name) this.#entries.push([name, (control as unknown as { value: string }).value])
        continue
      }
      if (local === 'input') {
        const input = control as unknown as HTMLInputElement
        const type = input.type
        if ((type === 'submit' || type === 'image') && control !== submitter) continue
        if (type === 'reset' || type === 'button') continue
        if ((type === 'checkbox' || type === 'radio') && !input.checked) continue
        if (!name) continue
        if (type === 'file') {
          const files = filesOf(input)
          if (files.length === 0) this.#entries.push([name, new File([], '', { type: 'application/octet-stream' })])
          for (const file of files) this.#entries.push([name, file])
          continue
        }
        this.#entries.push([name, input.value])
      } else if (local === 'select') {
        if (!name) continue
        for (const option of optionIds(id)) {
          const o = wrap<HTMLOptionElement>(option)!
          if (dom.optionSelected(option) && !dom.hasAttribute(option, 'disabled')) this.#entries.push([name, o.value])
        }
      } else if (local === 'textarea') {
        if (name) this.#entries.push([name, (control as unknown as HTMLTextAreaElement).value.replace(/\r?\n/g, '\r\n')])
      }
    }
  }

  static #value(value: unknown, filename?: string): string | File {
    if (value instanceof File) return filename === undefined ? value : new File([value], filename, { type: value.type, lastModified: value.lastModified })
    if (value !== null && typeof value === 'object' && 'arrayBuffer' in (value as object) && 'size' in (value as object)) return new File([value as never], filename ?? 'blob', { type: (value as { type?: string }).type })
    return String(value)
  }

  append(name: string, value: unknown, filename?: string): void {
    this.#entries.push([String(name), FormData.#value(value, filename)])
  }

  delete(name: string): void {
    const n = String(name)
    this.#entries = this.#entries.filter(([k]) => k !== n)
  }

  get(name: string): string | File | null {
    const n = String(name)
    return this.#entries.find(([k]) => k === n)?.[1] ?? null
  }

  getAll(name: string): (string | File)[] {
    const n = String(name)
    return this.#entries.filter(([k]) => k === n).map(([, v]) => v)
  }

  has(name: string): boolean {
    const n = String(name)
    return this.#entries.some(([k]) => k === n)
  }

  set(name: string, value: unknown, filename?: string): void {
    const n = String(name)
    const v = FormData.#value(value, filename)
    const at = this.#entries.findIndex(([k]) => k === n)
    if (at < 0) this.#entries.push([n, v])
    else {
      this.#entries[at] = [n, v]
      this.#entries = this.#entries.filter(([k], i) => k !== n || i === at)
    }
  }

  forEach(callback: (value: string | File, key: string, parent: FormData) => void, thisArg?: unknown): void {
    for (const [k, v] of this.#entries.slice()) callback.call(thisArg, v, k, this)
  }

  *entries(): IterableIterator<[string, string | File]> {
    for (const [k, v] of this.#entries) yield [k, v]
  }

  *keys(): IterableIterator<string> {
    for (const [k] of this.#entries) yield k
  }

  *values(): IterableIterator<string | File> {
    for (const [, v] of this.#entries) yield v
  }

  [Symbol.iterator](): IterableIterator<[string, string | File]> {
    return this.entries()
  }
}
tag(FormData)
