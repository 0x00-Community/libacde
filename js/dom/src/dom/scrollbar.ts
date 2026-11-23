import { metricsOf } from './layout'
import { dom } from './realm'

interface Drag {
  readonly id: number
  readonly vertical: boolean
  readonly ratio: number
  readonly from: number
  readonly start: number
  readonly other: number
}

export class ScrollbarDrag {
  #drag: Drag | null = null

  pointer(type: 'pointermove' | 'pointerdown' | 'pointerup', x: number, y: number, button: number): boolean {
    if (type === 'pointerdown') {
      this.#drag = button === 0 ? grab(x, y) : null
      return this.#drag !== null
    }
    const drag = this.#drag
    if (!drag) return false
    const at = Math.max(0, drag.start + ((drag.vertical ? y : x) - drag.from) * drag.ratio)
    if (drag.vertical) dom.scrollTo(drag.id, drag.other, at, false)
    else dom.scrollTo(drag.id, at, drag.other, false)
    if (type === 'pointerup') this.#drag = null
    return true
  }
}

function grab(x: number, y: number): Drag | null {
  const [id, axis, ratio] = dom.scrollbarAt()
  if (id === undefined || !ratio) return null
  const metrics = metricsOf(id)
  if (!metrics) return null
  const vertical = axis === 1
  return {
    id,
    vertical,
    ratio,
    from: vertical ? y : x,
    start: vertical ? metrics.scrollTop : metrics.scrollLeft,
    other: vertical ? metrics.scrollLeft : metrics.scrollTop,
  }
}
