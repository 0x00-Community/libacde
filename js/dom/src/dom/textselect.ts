import { Event, fire } from '../core/event'
import { NodeType } from '../native'
import type { Element } from './element'
import type { Modifiers } from './input'
import { computedStyleOf } from './layout'
import type { Node } from './node'
import { getSelection, Range } from './range'
import { dom, ID, inner, wrap } from './realm'

const NOT_SELECTING = 'input, textarea, select, button, option, label, [contenteditable]:not([contenteditable="false" i]), a[href], img, [draggable="true" i]'
const INLINE = new Set(['inline', 'inline-block', 'inline-flex', 'inline-grid', 'contents', 'ruby', 'ruby-text', 'none'])

type Granularity = 'character' | 'word' | 'paragraph'

interface Point {
  readonly node: Node
  readonly offset: number
}

let words: Intl.Segmenter | null = null

function caretAt(x: number, y: number): Point | null {
  const at = dom.caretFromPoint(x, y)
  const node = at ? wrap<Node>(at.node) : null
  return node ? { node, offset: at!.offset } : null
}

function selectable(element: Element): boolean {
  return computedStyleOf(element[ID], '', 'user-select') !== 'none'
}

function elementOf(node: Node): Element | null {
  return inner(node, 'nodeType') === NodeType.Element ? (node as Element) : inner(node, 'parentElement')
}

function word(point: Point): Range {
  const range = new Range()
  range.setStart(point.node, point.offset)
  range.collapse(true)
  if (inner(point.node, 'nodeType') !== NodeType.Text) return range
  const text = (point.node as unknown as { data: string }).data
  if (!text) return range
  words ??= new Intl.Segmenter(undefined, { granularity: 'word' })
  const at = Math.min(point.offset, text.length - 1)
  const segment = words.segment(text).containing(at)
  if (!segment) return range
  range.setStart(point.node, segment.index)
  range.setEnd(point.node, segment.index + segment.segment.length)
  return range
}

function paragraph(point: Point): Range {
  let block = elementOf(point.node)
  while (block && inner(block, 'parentElement') && INLINE.has(computedStyleOf(block[ID], '', 'display'))) block = inner(block, 'parentElement')
  const range = new Range()
  if (block) range.selectNodeContents(block)
  else range.setStart(point.node, point.offset)
  return range
}

function expand(point: Point, granularity: Granularity): Range {
  if (granularity === 'word') return word(point)
  if (granularity === 'paragraph') return paragraph(point)
  const range = new Range()
  range.setStart(point.node, point.offset)
  return range
}

function before(a: Point, b: Point): boolean {
  if (a.node === b.node) return a.offset <= b.offset
  return (dom.compareDocumentPosition(a.node[ID], b.node[ID]) & 4) !== 0
}

export class PointerSelection {
  #anchor: Range | null = null
  #granularity: Granularity = 'character'
  #dragging = false

  get dragging(): boolean {
    return this.#dragging
  }

  down(target: Element, x: number, y: number, count: number, modifiers: Modifiers): void {
    this.#dragging = false
    if (inner(target, 'closest').call(target, NOT_SELECTING) || !selectable(target)) return
    const point = caretAt(x, y)
    const selection = getSelection()
    if (!point) {
      selection.removeAllRanges()
      return
    }
    if (!fire(point.node, new Event('selectstart', { bubbles: true, cancelable: true }))) return
    this.#granularity = count >= 3 ? 'paragraph' : count === 2 ? 'word' : 'character'
    if (modifiers.shift && selection.rangeCount > 0 && this.#granularity === 'character') {
      selection.extend(point.node, point.offset)
      const anchor = new Range()
      anchor.setStart(selection.anchorNode!, selection.anchorOffset)
      this.#anchor = anchor
    } else {
      this.#anchor = expand(point, this.#granularity)
      selection.setBaseAndExtent(this.#anchor.startContainer, this.#anchor.startOffset, this.#anchor.endContainer, this.#anchor.endOffset)
    }
    this.#dragging = true
  }

  move(x: number, y: number): void {
    const anchor = this.#anchor
    if (!this.#dragging || !anchor) return
    const point = caretAt(x, y)
    if (!point) return
    const focus = expand(point, this.#granularity)
    const start = { node: anchor.startContainer, offset: anchor.startOffset }
    if (before(point, start)) getSelection().setBaseAndExtent(anchor.endContainer, anchor.endOffset, focus.startContainer, focus.startOffset)
    else getSelection().setBaseAndExtent(anchor.startContainer, anchor.startOffset, focus.endContainer, focus.endOffset)
  }

  up(): void {
    this.#dragging = false
  }
}
