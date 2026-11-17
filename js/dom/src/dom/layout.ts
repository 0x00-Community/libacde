import { NONE, type Metrics } from '../native'
import { dom, ID, layoutEpoch, rootElementId, version } from './realm'

let cachedVersion = -1
let cachedEpoch = -1
const rects = new Map<number, Float64Array>()
const metrics = new Map<number, Metrics | null>()
const styles = new Map<number, Map<string, string>>()

function fresh(): void {
  if (cachedVersion === version && cachedEpoch === layoutEpoch) return
  cachedVersion = version
  cachedEpoch = layoutEpoch
  rects.clear()
  metrics.clear()
  styles.clear()
}

export function rectOf(id: number): Float64Array {
  fresh()
  let r = rects.get(id)
  if (!r) rects.set(id, (r = dom.rect(id)))
  return r
}

export function metricsOf(id: number): Metrics | null {
  fresh()
  let m = metrics.get(id)
  if (m === undefined) metrics.set(id, (m = dom.metrics(id)))
  return m
}

export function metricGetters(proto: object, table: Record<string, keyof Metrics>): void {
  for (const [name, key] of Object.entries(table)) {
    Object.defineProperty(proto, name, { get(this: { [ID]: number }) { return metricsOf(this[ID])?.[key] ?? 0 }, enumerable: true, configurable: true })
  }
}

export function computedStyleOf(id: number, pseudo: string, property: string): string {
  fresh()
  let values = styles.get(id)
  if (!values) styles.set(id, (values = new Map()))
  const key = pseudo ? pseudo + property : property
  let value = values.get(key)
  if (value === undefined) values.set(key, (value = dom.computedStyle(id, pseudo, property)))
  return value
}

export function rootScroll(axis: 0 | 1): number {
  const html = rootElementId()
  const m = html === NONE ? null : metricsOf(html)
  return m ? (axis === 0 ? m.scrollLeft : m.scrollTop) : 0
}
