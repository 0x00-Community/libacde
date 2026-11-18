import type { InspectHost, InspectShell, ShellWindow } from './inspect'

export interface GuestRow {
  readonly id: number
  readonly app_id: string | null
  readonly pid: number | null
  readonly title: string | null
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
  readonly activated: boolean
  readonly hidden: boolean
  readonly state?: ShellWindow['state']
  readonly desktop: boolean
}

export class GuestWindows {
  #rows: readonly GuestRow[] = []

  constructor(
    readonly pid: number,
    readonly desktop: boolean,
  ) {}

  place(rows: unknown): void {
    this.#rows = Array.isArray(rows) ? rows.filter((row) => Number.isFinite((row as GuestRow | null)?.id)).map(GuestWindows.#row) : []
  }

  get placed(): boolean {
    return this.#rows.length > 0
  }

  static id(row: GuestRow): number {
    return row.desktop ? 0 : row.id
  }

  mine(): number | null {
    const row = this.#rows.find((r) => r.pid === this.pid && r.desktop === this.desktop)
    return row ? GuestWindows.id(row) : null
  }

  row(id: number): GuestRow | undefined {
    return this.#rows.find((r) => GuestWindows.id(r) === id)
  }

  shellWindows(): ShellWindow[] {
    const count = this.#rows.length
    return this.#rows.map((row, index) => ({
      id: GuestWindows.id(row),
      window: GuestWindows.id(row),
      canvas: 0,
      mode: row.desktop ? 'desktop' : 'window',
      x: row.x,
      y: row.y,
      width: row.width,
      height: row.height,
      z: row.desktop ? -1 : count - index,
      focus: row.activated,
      hidden: row.hidden,
      pid: row.pid,
      ...(row.state && { state: row.state }),
    }))
  }

  host(base: InspectHost, local: () => number): InspectHost {
    const own = (id: number) => this.placed && id === this.mine()
    const to = (id: number) => (own(id) ? local() : id)
    const shell: InspectShell = {
      ...base.shell,
      focus: (id) => base.shell.focus(to(id)),
      ...(base.shell.setState && { setState: (id, state) => base.shell.setState!(to(id), state) }),
      windows: () => (this.placed ? this.shellWindows() : base.shell.windows()),
      inject: (event) => base.shell.inject(event),
    }
    return {
      ...base,
      shell,
      web: (id) => (this.placed && !own(id) ? null : base.web(to(id))),
      record: (id) => {
        if (!this.placed || own(id)) return base.record(to(id))
        const row = this.row(id)
        return row && !row.desktop ? { kind: 'native', app: row.app_id, title: row.title } : null
      },
      panel: (id) => (this.placed && !own(id) ? false : base.panel(to(id))),
      close: (id) => (this.placed && !own(id) ? Promise.reject(new Error(`w${id} belongs to another process`)) : base.close(to(id))),
      hide: (id, hidden) => base.hide(to(id), hidden),
      records: () => (this.placed ? [this.mine() ?? local()] : base.records()),
      ref: (id) => (this.placed ? `w${id}` : (base.ref?.(id) ?? null)),
    }
  }

  static #row(given: GuestRow): GuestRow {
    const number = (value: unknown) => (Number.isFinite(value) ? (value as number) : 0)
    return {
      ...given,
      app_id: given.app_id ?? null,
      pid: given.pid ?? null,
      title: given.title ?? null,
      x: number(given.x),
      y: number(given.y),
      width: number(given.width),
      height: number(given.height),
      activated: given.activated === true,
      hidden: given.hidden === true,
      desktop: given.desktop === true,
    }
  }
}
