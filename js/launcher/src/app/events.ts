export interface ShellEvent {
  readonly type: string
  readonly window?: number
  readonly [field: string]: unknown
}

type Rect = { readonly x: number; readonly y: number; readonly width: number; readonly height: number }

interface Snapshot extends ShellEvent {
  readonly type: 'window'
  readonly window: number
  readonly appId: string | null
  readonly title: string | null
  readonly state: string
  readonly hidden: boolean
  readonly rect: Rect
  readonly active: boolean
  readonly desk: number
  readonly parent?: number
  readonly instance?: number
}

const NUMBER = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value)
const TEXT = (value: unknown): string | null => (typeof value === 'string' ? value : null)
const NO_RECT: Rect = { x: 0, y: 0, width: 0, height: 0 }

export class WindowSnapshots {
  readonly #windows = new Map<number, Snapshot>()

  toDesktop(event: ShellEvent): ShellEvent {
    const id = event.window
    if (event.type === 'window' && NUMBER(id)) {
      this.#windows.set(id, event as Snapshot)
      return event
    }
    if ((event.type === 'windowclose' || event.type === 'windowcrash') && NUMBER(id)) this.#windows.delete(id)
    if (!(event.type in APPLY) || !NUMBER(id)) return event
    return this.#fromOld(id, event)
  }

  #fromOld(id: number, event: ShellEvent): ShellEvent {
    const was = this.#windows.get(id)
    if (!was && event.type !== 'windowopen') return event
    const base: Snapshot = was ?? WindowSnapshots.#blank(id)
    const next: Record<string, unknown> = { ...base }
    const changed: string[] = []
    const set = (field: string, value: unknown) => {
      if (value === undefined || JSON.stringify(next[field]) === JSON.stringify(value)) return
      next[field] = value
      changed.push(field)
    }
    if (event.type === 'windowactivate') this.#deactivateOthers(id)
    APPLY[event.type]?.(event, base, set, next)
    const first = !was
    const snapshot = { ...next, first, ...(first ? {} : { changed }) } as unknown as Snapshot
    this.#windows.set(id, snapshot)
    return snapshot
  }

  #deactivateOthers(id: number): void {
    for (const [other, snapshot] of this.#windows) if (other !== id && snapshot.active) this.#windows.set(other, { ...snapshot, active: false })
  }

  static #blank(id: number): Snapshot {
    return { type: 'window', window: id, appId: null, title: null, state: 'normal', hidden: false, rect: NO_RECT, active: false, desk: 0 }
  }
}

type Setter = (field: string, value: unknown) => void
type Apply = (event: ShellEvent, base: Snapshot, set: Setter, next: Record<string, unknown>) => void

const naming: Apply = (event, base, set) => {
  set('appId', TEXT(event['appId']) ?? base.appId)
  set('title', TEXT(event['title']) ?? base.title)
}

const APPLY: Readonly<Record<string, Apply>> = {
  windowopen: (event, base, set, next) => {
    naming(event, base, set, next)
    for (const field of ['instance', 'desk', 'web', 'icon']) if (event[field] !== undefined) next[field] = event[field]
  },
  windowtitle: naming,
  windowstate: (event, base, set, next) => {
    next['previous'] = base.state
    set('state', TEXT(event['state']) ?? base.state)
  },
  windowactivate: (_, __, set) => set('active', true),
  windowparent: (event, _, set) => set('parent', NUMBER(event['parent']) ? event['parent'] : undefined),
  windowdesk: (event, _, set) => set('desk', NUMBER(event['desk']) ? event['desk'] : undefined),
  windowgeometry: (event, _, set) => {
    const { x, y, width, height } = event
    if ([x, y, width, height].every(NUMBER)) set('rect', { x, y, width, height })
  },
}
