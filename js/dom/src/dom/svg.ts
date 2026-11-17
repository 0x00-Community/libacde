import { defineHandlers } from '../core/event'
import { constants, getters, tag } from '../core/host'
import { NONE, NodeType } from '../native'
import { Element, HANDLER_TYPES, setAttribute } from './element'
import { DOMMatrix, DOMPoint, DOMQuad, DOMRect, svgTransform } from './geometry'
import { HTMLOrSVGElementMixin, svgClasses, svgFallback } from './html'
import { computedStyleOf, metricsOf, rectOf } from './layout'
import { DOMException } from '../core/exception'
import { elementById, mixin } from './node'
import { dom, ID, wrap } from './realm'
import { fitViewBox, shapeBox } from './svgbox'

function viewBox(id: number): [number, number, number, number] | null {
  const box = (dom.getAttribute(id, 'viewBox') ?? '').trim().split(/[\s,]+/).map(Number)
  return box.length === 4 && box.every(Number.isFinite) ? (box as [number, number, number, number]) : null
}

export class SVGAnimatedString {
  constructor(
    readonly element: Element,
    readonly attribute: string,
  ) {}
  get baseVal(): string {
    return dom.getAttribute(this.element[ID], this.attribute) ?? ''
  }
  set baseVal(value: string) {
    setAttribute(this.element, this.attribute, String(value))
  }
  get animVal(): string {
    return this.baseVal
  }
}
tag(SVGAnimatedString)

export class SVGElement extends Element {
  get ownerSVGElement(): SVGSVGElement | null {
    const parent = dom.parent(this[ID])
    if (parent === NONE || dom.nodeType(parent) !== NodeType.Element) return null
    return wrap<SVGSVGElement>(dom.closest(parent, 'svg'))
  }

  get viewportElement(): SVGElement | null {
    return this.ownerSVGElement
  }
}
mixin(SVGElement, HTMLOrSVGElementMixin)
defineHandlers(SVGElement.prototype, HANDLER_TYPES)
tag(SVGElement)
export interface SVGElement extends HTMLOrSVGElementMixin {}
Object.defineProperty(SVGElement.prototype, 'className', {
  get(this: Element) {
    return new SVGAnimatedString(this, 'class')
  },
  set(this: Element, value: unknown) {
    setAttribute(this, 'class', String(value))
  },
  enumerable: true,
  configurable: true,
})

export class SVGTransform {
  constructor(readonly matrix: DOMMatrix) {}
  setMatrix(matrix: DOMMatrix): void {
    ;(this as { matrix: DOMMatrix }).matrix = DOMMatrix.fromMatrix(matrix)
  }
}
getters(SVGTransform.prototype, { type: 1, angle: 0 })
constants(SVGTransform, { SVG_TRANSFORM_UNKNOWN: 0, SVG_TRANSFORM_MATRIX: 1 })
tag(SVGTransform)

export class SVGTransformList {
  constructor(readonly element: Element) {}

  #items(): SVGTransform[] {
    const text = dom.getAttribute(this.element[ID], 'transform') ?? ''
    const items: SVGTransform[] = []
    for (const m of text.matchAll(/[a-zA-Z]+\s*\([^)]*\)/g)) {
      const matrix = transformMatrix(m[0])
      if (matrix) items.push(new SVGTransform(matrix))
    }
    return items
  }

  get numberOfItems(): number {
    return this.#items().length
  }
  get length(): number {
    return this.numberOfItems
  }
  getItem(index: number): SVGTransform {
    const item = this.#items()[index]
    if (!item) throw new RangeError('index out of range')
    return item
  }
  consolidate(): SVGTransform | null {
    const items = this.#items()
    if (items.length === 0) return null
    const m = new DOMMatrix()
    for (const item of items) m.multiplySelf(item.matrix)
    setAttribute(this.element, 'transform', m.toString().replace(/,/g, ''))
    return new SVGTransform(m)
  }
  clear(): void {
    setAttribute(this.element, 'transform', null)
  }
  appendItem(item: SVGTransform): SVGTransform {
    const now = dom.getAttribute(this.element[ID], 'transform') ?? ''
    setAttribute(this.element, 'transform', `${now} ${item.matrix.toString().replace(/,/g, '')}`.trim())
    return item
  }
  createSVGTransformFromMatrix(matrix: DOMMatrix): SVGTransform {
    return new SVGTransform(DOMMatrix.fromMatrix(matrix))
  }
}
tag(SVGTransformList)

export class SVGAnimatedTransformList {
  constructor(readonly element: Element) {}
  get baseVal(): SVGTransformList {
    return new SVGTransformList(this.element)
  }
  get animVal(): SVGTransformList {
    return this.baseVal
  }
}
tag(SVGAnimatedTransformList)

function transformMatrix(text: string): DOMMatrix | null {
  try {
    return svgTransform(text)
  } catch {
    return null
  }
}

function ownTransform(id: number): DOMMatrix {
  const text = dom.getAttribute(id, 'transform')
  return (text && transformMatrix(text)) || new DOMMatrix()
}

function viewportMatrix(svg: number): DOMMatrix {
  const r = rectOf(svg)
  const metrics = metricsOf(svg)
  const x = r[0]! + (metrics?.clientLeft ?? 0), y = r[1]! + (metrics?.clientTop ?? 0)
  const width = metrics?.clientWidth ?? r[2]!, height = metrics?.clientHeight ?? r[3]!
  return new DOMMatrix().translateSelf(x, y).multiplySelf(fitViewBox(svg, width, height))
}

function screenCTM(id: number): DOMMatrix {
  const chain: number[] = [id]
  let svg = NONE
  const ancestors = dom.ancestors(id)
  if (dom.localName(id) === 'svg') svg = id
  else {
    for (const a of ancestors) {
      if (dom.nodeType(a) !== NodeType.Element) break
      if (dom.localName(a) === 'svg') {
        svg = a
        break
      }
      chain.push(a)
    }
  }
  if (svg === NONE) return new DOMMatrix()
  const m = viewportMatrix(svg)
  if (svg === id) return m
  for (let i = chain.length - 1; i >= 0; i--) m.multiplySelf(ownTransform(chain[i]!))
  return m
}

function drawnBox(id: number): [number, number, number, number] | null {
  const r = rectOf(id)
  if (!r[2] && !r[3]) return null
  const inverse = screenCTM(id).inverse()
  const p = (x: number, y: number) => new DOMPoint(x, y).matrixTransform(inverse)
  const b = new DOMQuad(p(r[0]!, r[1]!), p(r[0]! + r[2]!, r[1]!), p(r[0]! + r[2]!, r[1]! + r[3]!), p(r[0]!, r[1]! + r[3]!)).getBounds()
  return [b.x, b.y, b.x + b.width, b.y + b.height]
}

export class SVGGraphicsElement extends SVGElement {
  get transform(): SVGAnimatedTransformList {
    return new SVGAnimatedTransformList(this)
  }

  getScreenCTM(): DOMMatrix | null {
    return screenCTM(this[ID])
  }

  getCTM(): DOMMatrix | null {
    const owner = this.ownerSVGElement
    const screen = screenCTM(this[ID])
    if (!owner) return screen
    return viewportMatrix(owner[ID]).inverse().multiply(screen)
  }

  getBBox(): DOMRect {
    const box = shapeBox(this[ID], ownTransform, drawnBox)
    if (box) return new DOMRect(box[0], box[1], box[2] - box[0], box[3] - box[1])
    if (this instanceof SVGGeometryElement) return new DOMRect()
    const drawn = drawnBox(this[ID])
    return drawn ? new DOMRect(drawn[0], drawn[1], drawn[2] - drawn[0], drawn[3] - drawn[1]) : new DOMRect()
  }

  get farthestViewportElement(): SVGElement | null {
    let found: SVGElement | null = null
    for (const a of dom.ancestors(this[ID])) if (dom.nodeType(a) === NodeType.Element && dom.localName(a) === 'svg') found = wrap<SVGElement>(a)
    return found
  }

  get nearestViewportElement(): SVGElement | null {
    return this.ownerSVGElement
  }
}
tag(SVGGraphicsElement)

export class SVGGeometryElement extends SVGGraphicsElement {
  get pathLength(): { baseVal: number; animVal: number } {
    const v = parseFloat(dom.getAttribute(this[ID], 'pathLength') ?? '')
    return { baseVal: Number.isFinite(v) ? v : 0, animVal: Number.isFinite(v) ? v : 0 }
  }
  getTotalLength(): number {
    const b = this.getBBox()
    return 2 * (b.width + b.height)
  }
  getPointAtLength(_distance: number): DOMPoint {
    const b = this.getBBox()
    return new DOMPoint(b.x, b.y)
  }
  isPointInFill(): boolean {
    return false
  }
  isPointInStroke(): boolean {
    return false
  }
}
tag(SVGGeometryElement)

class SVGNumber {
  constructor(public value: number) {}
}

export class SVGSVGElement extends SVGGraphicsElement {
  createSVGPoint(): DOMPoint {
    return new DOMPoint()
  }
  createSVGMatrix(): DOMMatrix {
    return new DOMMatrix()
  }
  createSVGRect(): DOMRect {
    return new DOMRect()
  }
  createSVGNumber(): SVGNumber {
    return new SVGNumber(0)
  }
  createSVGTransform(): SVGTransform {
    return new SVGTransform(new DOMMatrix())
  }
  createSVGTransformFromMatrix(matrix: DOMMatrix): SVGTransform {
    return new SVGTransform(DOMMatrix.fromMatrix(matrix))
  }
  getElementById(id: string): Element | null {
    return elementById(this[ID], String(id))
  }
  get viewBox(): { baseVal: DOMRect | null; animVal: DOMRect | null } {
    const box = viewBox(this[ID])
    const rect = box && new DOMRect(...box)
    return { baseVal: rect, animVal: rect }
  }
  get width(): { baseVal: { value: number }; animVal: { value: number } } {
    const v = { value: metricsOf(this[ID])?.clientWidth ?? 0 }
    return { baseVal: v, animVal: v }
  }
  get height(): { baseVal: { value: number }; animVal: { value: number } } {
    const v = { value: metricsOf(this[ID])?.clientHeight ?? 0 }
    return { baseVal: v, animVal: v }
  }
  get currentTranslate(): DOMPoint {
    return new DOMPoint()
  }
  suspendRedraw(): number {
    return 0
  }
  unsuspendRedraw(): void {}
  unsuspendRedrawAll(): void {}
  forceRedraw(): void {}
  pauseAnimations(): void {}
  unpauseAnimations(): void {}
  animationsPaused(): boolean {
    return false
  }
  getCurrentTime(): number {
    return 0
  }
  setCurrentTime(): void {}
}
getters(SVGSVGElement.prototype, { currentScale: 1 })
tag(SVGSVGElement)

function svgClass(name: string, base: typeof SVGElement, ...locals: string[]): typeof SVGElement {
  const cls = { [name]: class extends base {} }[name]!
  tag(cls, name)
  for (const local of locals) svgClasses.set(local, cls)
  return cls
}

svgClasses.set('svg', SVGSVGElement)
export const SVGGElement = svgClass('SVGGElement', SVGGraphicsElement, 'g')
export const SVGDefsElement = svgClass('SVGDefsElement', SVGGraphicsElement, 'defs')
export const SVGUseElement = svgClass('SVGUseElement', SVGGraphicsElement, 'use')
export const SVGSymbolElement = svgClass('SVGSymbolElement', SVGElement, 'symbol')
export const SVGImageElement = svgClass('SVGImageElement', SVGGraphicsElement, 'image')
export const SVGForeignObjectElement = svgClass('SVGForeignObjectElement', SVGGraphicsElement, 'foreignObject')
export const SVGSwitchElement = svgClass('SVGSwitchElement', SVGGraphicsElement, 'switch')
export const SVGAElement = svgClass('SVGAElement', SVGGraphicsElement, 'a')
function svgText(id: number): string {
  return dom.textContent(id).replace(/[\r\n]/g, '').replace(/\t/g, ' ').replace(/ +/g, ' ').trim()
}

function advance(id: number, text: string): number {
  if (!text) return 0
  const style = (property: string) => computedStyleOf(id, '', property)
  const font = `${style('font-style')} ${style('font-weight')} ${style('font-size')} ${style('font-family')}`
  const spacing = parseFloat(style('letter-spacing')) || 0
  return (dom.measureText(id, font, text)[0] ?? 0) + spacing * text.length
}

export class SVGTextContentElement extends SVGGraphicsElement {
  getNumberOfChars(): number {
    return svgText(this[ID]).length
  }

  getComputedTextLength(): number {
    return advance(this[ID], svgText(this[ID]))
  }

  getSubStringLength(charnum: number, nchars: number): number {
    const text = svgText(this[ID])
    const start = charnum >>> 0
    if (start > text.length) throw new DOMException('getSubStringLength: charnum is past the last character', 'IndexSizeError')
    return advance(this[ID], text.slice(start, start + (nchars >>> 0)))
  }
}
tag(SVGTextContentElement)
export const SVGTextElement = svgClass('SVGTextElement', SVGTextContentElement, 'text')
export const SVGTSpanElement = svgClass('SVGTSpanElement', SVGTextContentElement, 'tspan')
export const SVGTextPathElement = svgClass('SVGTextPathElement', SVGTextContentElement, 'textPath')
export const SVGPathElement = svgClass('SVGPathElement', SVGGeometryElement, 'path')
export const SVGRectElement = svgClass('SVGRectElement', SVGGeometryElement, 'rect')
export const SVGCircleElement = svgClass('SVGCircleElement', SVGGeometryElement, 'circle')
export const SVGEllipseElement = svgClass('SVGEllipseElement', SVGGeometryElement, 'ellipse')
export const SVGLineElement = svgClass('SVGLineElement', SVGGeometryElement, 'line')
export const SVGPolylineElement = svgClass('SVGPolylineElement', SVGGeometryElement, 'polyline')
export const SVGPolygonElement = svgClass('SVGPolygonElement', SVGGeometryElement, 'polygon')
export const SVGGradientElement = svgClass('SVGGradientElement', SVGElement)
export const SVGLinearGradientElement = svgClass('SVGLinearGradientElement', SVGGradientElement, 'linearGradient')
export const SVGRadialGradientElement = svgClass('SVGRadialGradientElement', SVGGradientElement, 'radialGradient')
export const SVGStopElement = svgClass('SVGStopElement', SVGElement, 'stop')
export const SVGClipPathElement = svgClass('SVGClipPathElement', SVGElement, 'clipPath')
export const SVGMaskElement = svgClass('SVGMaskElement', SVGElement, 'mask')
export const SVGPatternElement = svgClass('SVGPatternElement', SVGElement, 'pattern')
export const SVGMarkerElement = svgClass('SVGMarkerElement', SVGElement, 'marker')
export const SVGFilterElement = svgClass('SVGFilterElement', SVGElement, 'filter')
export const SVGTitleElement = svgClass('SVGTitleElement', SVGElement, 'title')
export const SVGDescElement = svgClass('SVGDescElement', SVGElement, 'desc')
export const SVGMetadataElement = svgClass('SVGMetadataElement', SVGElement, 'metadata')
export const SVGStyleElement = svgClass('SVGStyleElement', SVGElement, 'style')
export const SVGScriptElement = svgClass('SVGScriptElement', SVGElement, 'script')
export const SVGViewElement = svgClass('SVGViewElement', SVGElement, 'view')
export const SVGAnimationElement = svgClass('SVGAnimationElement', SVGElement, 'animate', 'animateMotion', 'animateTransform', 'set')
for (const fe of ['feBlend', 'feColorMatrix', 'feComponentTransfer', 'feComposite', 'feConvolveMatrix', 'feDiffuseLighting', 'feDisplacementMap', 'feDistantLight', 'feDropShadow', 'feFlood', 'feFuncA', 'feFuncB', 'feFuncG', 'feFuncR', 'feGaussianBlur', 'feImage', 'feMerge', 'feMergeNode', 'feMorphology', 'feOffset', 'fePointLight', 'feSpecularLighting', 'feSpotLight', 'feTile', 'feTurbulence']) {
  svgClass(`SVG${fe[0]!.toUpperCase()}${fe.slice(1)}Element`, SVGElement, fe)
}
svgFallback.cls = SVGElement
