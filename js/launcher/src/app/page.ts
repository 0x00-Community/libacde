import type { FileFilter, LentFile } from '../../../page/src/sys/natives'
import { HostStrings } from '../../../page/src/language'

export interface PageCallsHost {
  readonly label: string
  readonly language: () => string | undefined
  openFiles(title: string, multiple: boolean, filters: readonly FileFilter[]): Promise<readonly LentFile[] | null>
  saveFile(title: string, name: string, filters: readonly FileFilter[]): Promise<LentFile | null>
  folder(title: string, writable: boolean): Promise<{ readonly name: string; readonly label: string } | null>
  unlend(name: string): Promise<void>
  notify?(title: string, body: string, subtitle: string): void
}

export class PageCalls {
  static readonly METHODS: ReadonlySet<string> = new Set(['open', 'save', 'folder', 'unlend', 'notify'])
  readonly #host: PageCallsHost

  constructor(host: PageCallsHost) {
    this.#host = host
  }

  async call(method: string, args: readonly unknown[]): Promise<unknown> {
    const options = (args[0] ?? {}) as Record<string, unknown>
    if (method === 'notify') return this.#notify(options)
    if (method === 'unlend') return this.#host.unlend(PageCalls.#text(args[0], 200))
    if (method === 'folder') return this.#host.folder(PageCalls.#text(options['title'], 300), options['writable'] === true)
    const strings = HostStrings.of(this.#host.language())
    const filters = PageCalls.#filters(options['filters'])
    if (method === 'open') return this.#host.openFiles(`${this.#host.label}: ${strings.open}`, options['multiple'] === true, filters)
    return this.#host.saveFile(`${this.#host.label}: ${strings.save}`, PageCalls.#text(options['name'], 255), filters)
  }

  #notify(options: Record<string, unknown>): boolean {
    if (!this.#host.notify) throw Object.assign(new Error('notifications are unsupported here'), { status: 501, code: 'unsupported' })
    this.#host.notify(PageCalls.#text(options['title'], 200), PageCalls.#text(options['body'], 2000), this.#host.label)
    return true
  }

  static #text(value: unknown, max: number): string {
    return (typeof value === 'string' ? value : value === undefined || value === null ? '' : String(value)).slice(0, max)
  }

  static #filters(value: unknown): FileFilter[] {
    const list = Array.isArray(value) ? value.slice(0, 32) : []
    return list.map((f: { name?: unknown; globs?: unknown; mimes?: unknown }) => ({
      name: PageCalls.#text(f?.name, 200),
      globs: (Array.isArray(f?.globs) ? f.globs : []).map((g: unknown) => PageCalls.#text(g, 100)).filter(Boolean),
      mimes: (Array.isArray(f?.mimes) ? f.mimes : []).map((m: unknown) => PageCalls.#text(m, 100)).filter(Boolean),
    })).filter((f) => f.globs.length > 0 || f.mimes.length > 0)
  }
}
