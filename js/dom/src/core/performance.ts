import { EventTarget } from './event'
import { DOMException } from './exception'
import { host, tag } from './host'
import { now as clockNow } from './clock'

export class PerformanceEntry {
  constructor(
    readonly name: string,
    readonly entryType: string,
    readonly startTime: number,
    readonly duration: number,
    readonly detail: unknown = null,
  ) {}

  toJSON(): object {
    return { name: this.name, entryType: this.entryType, startTime: this.startTime, duration: this.duration }
  }
}
tag(PerformanceEntry)

export class PerformanceMark extends PerformanceEntry {}
tag(PerformanceMark)
export class PerformanceMeasure extends PerformanceEntry {}
tag(PerformanceMeasure)

const MAX = 1000

export class Performance extends EventTarget {
  #entries: PerformanceEntry[] = []

  now(): number {
    return clockNow()
  }

  get timeOrigin(): number {
    return host.timeOrigin()
  }

  mark(name: string, options?: { startTime?: number; detail?: unknown }): PerformanceMark {
    const entry = new PerformanceMark(String(name), 'mark', options?.startTime ?? clockNow(), 0, options?.detail ?? null)
    this.#add(entry)
    return entry
  }

  measure(name: string, start?: string | { start?: number | string; end?: number | string; duration?: number; detail?: unknown }, end?: string): PerformanceMeasure {
    const time = (at: number | string | undefined, fallback: number) => {
      if (at === undefined) return fallback
      if (typeof at === 'number') return at
      const mark = this.#entries.findLast((e) => e.name === at && e.entryType === 'mark')
      if (!mark) throw new DOMException(`The mark '${at}' does not exist.`, 'SyntaxError')
      return mark.startTime
    }
    let from: number, to: number, detail: unknown = null
    if (typeof start === 'object' && start !== null) {
      detail = start.detail ?? null
      to = time(start.end, start.duration !== undefined && start.start !== undefined ? time(start.start, 0) + start.duration : clockNow())
      from = time(start.start, start.duration !== undefined ? to - start.duration : 0)
    } else {
      from = time(start, 0)
      to = time(end, clockNow())
    }
    const entry = new PerformanceMeasure(String(name), 'measure', from, to - from, detail)
    this.#add(entry)
    return entry
  }

  #add(entry: PerformanceEntry): void {
    this.#entries.push(entry)
    if (this.#entries.length > 2 * MAX) this.#entries = this.#entries.slice(-MAX)
  }

  getEntries(): PerformanceEntry[] {
    return this.#entries.slice()
  }

  getEntriesByType(type: string): PerformanceEntry[] {
    return this.#entries.filter((e) => e.entryType === type)
  }

  getEntriesByName(name: string, type?: string): PerformanceEntry[] {
    return this.#entries.filter((e) => e.name === name && (type === undefined || e.entryType === type))
  }

  clearMarks(name?: string): void {
    this.#entries = this.#entries.filter((e) => e.entryType !== 'mark' || (name !== undefined && e.name !== name))
  }

  clearMeasures(name?: string): void {
    this.#entries = this.#entries.filter((e) => e.entryType !== 'measure' || (name !== undefined && e.name !== name))
  }

  clearResourceTimings(): void {}
  setResourceTimingBufferSize(): void {}

  toJSON(): object {
    return { timeOrigin: this.timeOrigin }
  }
}
tag(Performance)
