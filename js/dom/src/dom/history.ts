import { fire } from '../core/event'
import { InputEvent } from './events'
import { fieldSelection, replaceFieldText, setFieldSelection, type TextField } from './forms'

type Span = readonly [number, number]

interface Step {
  readonly type: string
  readonly before: string
  readonly from: Span
  after: string
  to: Span
  open: boolean
}

interface Stacks {
  readonly undo: Step[]
  readonly redo: Step[]
}

const DEPTH = 1000
const TYPING = new Set(['insertText', 'insertLineBreak'])
const histories = new WeakMap<TextField, Stacks>()

function joins(last: Step, type: string, before: string, from: Span): boolean {
  if (!last.open || last.after !== before || last.to[0] !== from[0] || last.to[1] !== from[1]) return false
  if (TYPING.has(last.type)) return TYPING.has(type) && from[0] === from[1]
  return last.type === type && (type === 'deleteContentBackward' || type === 'deleteContentForward') && from[0] === from[1]
}

function live(field: TextField): Stacks | null {
  const stacks = histories.get(field)
  if (!stacks) return null
  const expected = stacks.undo.at(-1)?.after ?? stacks.redo.at(-1)?.before
  if (expected === field.value) return stacks
  histories.delete(field)
  return null
}

function span(field: TextField): Span {
  const { start, end } = fieldSelection(field)
  return [start, end]
}

export function edit(field: TextField, type: string, change: () => void): void {
  const known = live(field)
  const before = field.value
  const from = span(field)
  change()
  const after = field.value
  if (after === before) return
  const to = span(field)
  const stacks = known ?? { undo: [], redo: [] }
  stacks.redo.length = 0
  const last = stacks.undo.at(-1)
  if (last && joins(last, type, before, from)) {
    last.after = after
    last.to = to
  } else {
    stacks.undo.push({ type, before, from, after, to, open: true })
    if (stacks.undo.length > DEPTH) stacks.undo.shift()
  }
  histories.set(field, stacks)
}

export function canUndo(field: TextField): boolean {
  return !!live(field)?.undo.length
}

export function canRedo(field: TextField): boolean {
  return !!live(field)?.redo.length
}

function travel(field: TextField, back: boolean, before: boolean): boolean {
  const stacks = live(field)
  const source = back ? stacks?.undo : stacks?.redo
  const step = source?.at(-1)
  if (!stacks || !source || !step) return false
  const inputType = back ? 'historyUndo' : 'historyRedo'
  if (before && !fire(field, new InputEvent('beforeinput', { bubbles: true, cancelable: true, composed: true, inputType, data: null }))) return true
  if (live(field) !== stacks || source.at(-1) !== step) return true
  source.pop()
  step.open = false
  const [value, [start, end]] = back ? [step.before, step.from] : [step.after, step.to]
  replaceFieldText(field, 0, field.value.length, value)
  setFieldSelection(field, start, end)
  ;(back ? stacks.redo : stacks.undo).push(step)
  fire(field, new InputEvent('input', { bubbles: true, composed: true, inputType, data: null }))
  return true
}

export function undo(field: TextField, before = true): boolean {
  return travel(field, true, before)
}

export function redo(field: TextField, before = true): boolean {
  return travel(field, false, before)
}
