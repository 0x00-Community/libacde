import { fire } from '../core/event'
import { NONE, NodeType } from '../native'
import { LOCAL, noteMarkup, type Element } from './element'
import { InputEvent } from './events'
import { EDITING_HOST } from './focus'
import { fieldSelection, replaceFieldText, setFieldSelection, submitImplicitly, type TextField } from './forms'
import * as history from './history'
import type { HTMLElement } from './html'
import { Text } from './node'
import { getSelection, type Range } from './range'
import { dom, ID, wrap, inner } from './realm'

const TEXT_TYPES = new Set(['text', 'search', 'url', 'email', 'password', 'tel', 'number'])

export function textField(target: Element): TextField | null {
  if (target[LOCAL] === 'textarea') return target as unknown as TextField
  if (target[LOCAL] !== 'input') return null
  return TEXT_TYPES.has((target as unknown as { type: string }).type) ? (target as unknown as TextField) : null
}

export function editable(target: Element): boolean {
  return !!inner((target as HTMLElement), 'isContentEditable') || dom.closest(target[ID], EDITING_HOST) !== NONE
}

export function selectedText(target: Element): string {
  const field = textField(target)
  if (!field) return getSelection().toString()
  const { start, end } = fieldSelection(field)
  return field.value.slice(start, end)
}

function dataOf(inputType: string, text: string): string | null {
  return inputType === 'insertLineBreak' || inputType === 'insertParagraph' || !text ? null : text
}

function beforeInput(target: Element, inputType: string, data: string | null): boolean {
  const composing = inputType === 'insertCompositionText'
  return fire(target, new InputEvent('beforeinput', { bubbles: true, cancelable: !composing, composed: true, inputType, data, isComposing: composing })) || composing
}

function afterInput(target: Element, inputType: string, data: string | null): void {
  fire(target, new InputEvent('input', { bubbles: true, composed: true, inputType, data, isComposing: inputType === 'insertCompositionText' }))
}

function replaceField(field: TextField, text: string, from: number, to: number, inputType: string, before: boolean): void {
  const data = dataOf(inputType, text)
  if (before && !beforeInput(field, inputType, data)) return
  history.edit(field, inputType, () => replaceFieldText(field, from, to, text))
  afterInput(field, inputType, data)
}

function editRange(target: Element, inputType: string, data: string | null, before: boolean, change: (range: Range) => void): boolean {
  if (!editable(target)) return false
  const selection = getSelection()
  if (selection.rangeCount === 0) return false
  if (before && !beforeInput(target, inputType, data)) return true
  change(selection.getRangeAt(0))
  afterInput(target, inputType, data)
  return true
}

export function insertText(target: Element, text: string, inputType = 'insertText', before = true): boolean {
  const field = textField(target)
  if (field) {
    if (field[LOCAL] === 'input' && (inputType === 'insertLineBreak' || inputType === 'insertParagraph')) return false
    const { start, end } = fieldSelection(field)
    replaceField(field, text, start, end, inputType, before)
    return true
  }
  return editRange(target, inputType, dataOf(inputType, text), before, (range) => {
    const selection = getSelection()
    range.deleteContents()
    const container = range.startContainer
    if (!text) return void selection.collapse(container, range.startOffset)
    if (inner(container, 'nodeType') === NodeType.Text) {
      const at = range.startOffset
      ;(container as Text).insertData(at, text)
      selection.collapse(container, at + text.length)
    } else {
      const node = new Text(text)
      range.insertNode(node)
      selection.collapse(node, text.length)
    }
  })
}

export function insertHTML(target: Element, html: string, before = true): boolean {
  if (textField(target)) return insertText(target, html, 'insertText', before)
  return editRange(target, 'insertHTML', html, before, (range) => {
    range.deleteContents()
    noteMarkup(html)
    const fragment = range.createContextualFragment(html)
    const last = fragment.lastChild
    range.insertNode(fragment)
    const parent = last && inner(last, 'parentNode')
    if (last && parent) getSelection().collapse(parent, dom.childNodes(parent[ID]).indexOf(last[ID]) + 1)
  })
}

export function deleteText(target: Element, forward: boolean, before = true): boolean {
  const inputType = forward ? 'deleteContentForward' : 'deleteContentBackward'
  const field = textField(target)
  if (field) {
    const { start, end } = fieldSelection(field)
    const length = field.value.length
    if (start !== end) replaceField(field, '', start, end, inputType, before)
    else if (forward ? end < length : start > 0) replaceField(field, '', forward ? start : start - 1, forward ? start + 1 : start, inputType, before)
    return true
  }
  if (!editable(target)) return false
  const selection = getSelection()
  if (selection.rangeCount === 0) return false
  const range = selection.getRangeAt(0)
  if (!range.collapsed) return editRange(target, inputType, null, before, (r) => {
    r.deleteContents()
    selection.collapse(r.startContainer, r.startOffset)
  })
  const node = range.startContainer
  if (inner(node, 'nodeType') !== NodeType.Text) return true
  const from = forward ? range.startOffset : range.startOffset - 1
  if (from < 0 || from >= (node as Text).length) return true
  return editRange(target, inputType, null, before, () => {
    ;(node as Text).deleteData(from, 1)
    selection.collapse(node, from)
  })
}

export function deleteSurrounding(target: Element, offset: number, length: number): void {
  const field = textField(target)
  if (field) {
    const cursor = fieldSelection(field).end
    const from = Math.max(0, cursor + offset)
    return replaceField(field, '', from, Math.min(field.value.length, from + length), 'deleteContent', true)
  }
  const selection = getSelection()
  const node = selection.focusNode
  if (!selection.isCollapsed || (node && inner(node, 'nodeType')) !== NodeType.Text) return
  const from = Math.max(0, selection.focusOffset + offset), to = Math.min((node as Text).length, from + length)
  editRange(target, 'deleteContent', null, true, () => {
    ;(node as Text).deleteData(from, to - from)
    selection.collapse(node, from)
  })
}

export function selectAll(target: Element): boolean {
  const field = textField(target)
  if (field) {
    ;(field as unknown as { select(): void }).select()
    return true
  }
  if (!editable(target)) return false
  getSelection().selectAllChildren(wrap<Element>(dom.closest(target[ID], '[contenteditable]:not([contenteditable="false" i])')) ?? target)
  return true
}

export function undo(target: Element, before = true): boolean {
  const field = textField(target)
  return !!field && history.undo(field, before)
}

export function redo(target: Element, before = true): boolean {
  const field = textField(target)
  return !!field && history.redo(field, before)
}

export function canUndo(target: Element): boolean {
  const field = textField(target)
  return !!field && history.canUndo(field)
}

export function canRedo(target: Element): boolean {
  const field = textField(target)
  return !!field && history.canRedo(field)
}

export function editKey(target: Element, key: string, ctrl: boolean, shift = false): void {
  const field = textField(target)
  if (!field && !editable(target)) return
  if (key.length === 1 && !ctrl) return void insertText(target, key)
  if (key === 'Backspace' || key === 'Delete') return void deleteText(target, key === 'Delete')
  if (key === 'a' && ctrl) return void selectAll(target)
  if (!field) return
  const lower = key.toLowerCase()
  if (ctrl && lower === 'z') return void (shift ? redo(target) : undo(target))
  if (ctrl && lower === 'y' && !shift) return void redo(target)
  const { start, end } = fieldSelection(field)
  const length = field.value.length
  const caret = (at: number) => setFieldSelection(field, at, at)
  if (key === 'ArrowLeft') return caret(start !== end ? start : Math.max(0, start - 1))
  if (key === 'ArrowRight') return caret(start !== end ? end : Math.min(length, end + 1))
  if (key === 'Home') return caret(0)
  if (key === 'End') return caret(length)
  if (key === 'Enter') {
    if (field[LOCAL] === 'textarea') return void insertText(target, '\n', 'insertLineBreak')
    submitImplicitly(field)
  }
}
