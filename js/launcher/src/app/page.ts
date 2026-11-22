import type { NativeFileChooser } from '../../../page/src/native'
import { HostStrings } from '../../../page/src/language'
import { Level, Log } from '../../../page/src/log'

export interface PageCallsHost {
  readonly label: string
  readonly language: () => string | undefined
  chooser(): Promise<NativeFileChooser> | null
  folder(title: string, writable: boolean): Promise<{ readonly name: string; readonly label: string } | null>
  notify?(title: string, body: string, subtitle: string): void
}

type Filter = { name: string; globs: string[]; mimes: string[] }

export class PageCalls {
  static readonly METHODS: ReadonlySet<string> = new Set(['open', 'save', 'folder', 'notify'])
  readonly #host: PageCallsHost
  #chooser: Promise<NativeFileChooser | null> | null = null

  constructor(host: PageCallsHost) {
    this.#host = host
  }

  async call(method: string, args: readonly unknown[]): Promise<unknown> {
    const options = (args[0] ?? {}) as Record<string, unknown>
    if (method === 'notify') return this.#notify(options)
    if (method === 'folder') return this.#host.folder(PageCalls.#text(options['title'], 300), options['writable'] === true)
    const chooser = await this.#fileChooser()
    if (!chooser) return null
    const strings = HostStrings.of(this.#host.language())
    const filters = PageCalls.#filters(options['filters'])
    if (method === 'open') {
      const title = `${this.#host.label}: ${strings.open}`
      return chooser.openFile({ title, multiple: options['multiple'] === true, directory: options['directory'] === true, filters })
    }
    return chooser.saveFile({ title: `${this.#host.label}: ${strings.save}`, name: PageCalls.#text(options['name'], 255), filters })
  }

  #notify(options: Record<string, unknown>): boolean {
    if (!this.#host.notify) throw Object.assign(new Error('notifications are unsupported here'), { status: 501, code: 'unsupported' })
    this.#host.notify(PageCalls.#text(options['title'], 200), PageCalls.#text(options['body'], 2000), this.#host.label)
    return true
  }

  #fileChooser(): Promise<NativeFileChooser | null> {
    this.#chooser ??= (async () => {
      try {
        const opened = this.#host.chooser()
        if (!opened) throw new Error('this platform has none')
        return await opened
      } catch (error) {
        Log.write(Level.Warn, 'acde_host', `no file chooser (${(error as Error).message}): open and save pickers are cancelled`)
        return null
      }
    })()
    return this.#chooser
  }

  static #text(value: unknown, max: number): string {
    return (typeof value === 'string' ? value : value === undefined || value === null ? '' : String(value)).slice(0, max)
  }

  static #filters(value: unknown): Filter[] {
    const list = Array.isArray(value) ? value.slice(0, 32) : []
    return list.map((f: { name?: unknown; globs?: unknown; mimes?: unknown }) => ({
      name: PageCalls.#text(f?.name, 200),
      globs: (Array.isArray(f?.globs) ? f.globs : []).map((g: unknown) => PageCalls.#text(g, 100)).filter(Boolean),
      mimes: (Array.isArray(f?.mimes) ? f.mimes : []).map((m: unknown) => PageCalls.#text(m, 100)).filter(Boolean),
    })).filter((f) => f.globs.length > 0 || f.mimes.length > 0)
  }
}
