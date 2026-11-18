import { DevLog, type DevPost, type DevQuery } from '../../page/src/devlog'
import type { ConsoleLevel } from '@dopame/acde-dom'

export interface DevTarget {
  readonly name: string
  readonly app: string | null
  readonly kind: 'app' | 'desktop'
  readonly window: number
  readonly title: string
  call(service: string, method: string, args: readonly unknown[]): Promise<unknown>
}

export interface DevShell {
  devRespond?(id: number, ok: boolean, payload: string): void
}

type Request = Record<string, unknown> & { readonly id: number; readonly op: string }

export class DevChannel {
  static readonly ANSWERED = ['webs', 'eval', 'console', 'network', 'permissions'] as const
  readonly log = new DevLog()

  constructor(readonly shell: DevShell, readonly page: () => DevTarget | null) {}

  take(post: unknown): boolean {
    if (!DevLog.isPost(post)) return false
    const page = this.page()
    this.log.take({ name: page?.name ?? '', app: page?.app ?? null }, post as DevPost)
    return true
  }

  gone(name: string): void {
    this.log.gone(name)
  }

  async request(line: string): Promise<void> {
    let id: number | null = null
    let ok = true
    let payload: string
    try {
      const request = JSON.parse(line) as Request
      id = request.id
      payload = JSON.stringify(await this.answer(request))
    } catch (error) {
      ok = false
      payload = (error as Error)?.message ?? String(error)
    }
    if (id === null) return
    this.shell.devRespond?.(id, ok, payload)
  }

  async answer(request: Request): Promise<unknown> {
    const { type: _type, id: _id, op, ...args } = request
    switch (op) {
      case 'webs':
        return { webs: this.#webs() }
      case 'eval':
        return this.#eval(args)
      case 'console':
        return this.log.console({ ...DevChannel.#query(args), ...(args['level'] !== undefined && { level: args['level'] as ConsoleLevel | 'errors' }) })
      case 'network':
        return this.log.requests({ ...DevChannel.#query(args), failed: args['failed'] === true })
      case 'permissions':
        return { pending: [], decisions: [] }
    }
    throw new Error(`${op}: not answered in a single-app session yet (answered: ${DevChannel.ANSWERED.join(', ')})`)
  }

  #webs(): object[] {
    const page = this.page()
    return page ? [{ name: page.name, app: page.app, kind: page.kind, window: page.window, title: page.title }] : []
  }

  #web(wanted: unknown): DevTarget {
    const page = this.page()
    const name = String(wanted ?? '')
    const matches = !!page && (name === '' || name === page.name || name === page.app || (name === 'desktop' && page.kind === 'desktop'))
    if (!matches) {
      const known = page ? page.name : 'none'
      throw Object.assign(new Error(`no web ${name} (webs: ${known}; a web name, an app id or desktop)`), { status: 404 })
    }
    return page!
  }

  async #eval(args: Record<string, unknown>): Promise<{ value: unknown }> {
    if (typeof args['code'] !== 'string') throw new Error('eval: code is a string')
    let value: unknown
    try {
      const page = this.#web(args['web'])
      const node = args['node']
      value = node === undefined || node === null
        ? await page.call('debug', 'eval', [args['code']])
        : await page.call('debug', 'evalOn', [Number(node), args['code'], args['arg'] ?? null])
    } catch (error) {
      throw new Error(error instanceof Error ? `${error.name}: ${error.message}` : String(error))
    }
    try {
      return { value: JSON.parse(JSON.stringify(value) ?? 'null') as unknown }
    } catch (error) {
      throw new Error(`eval: the value is not JSON: ${(error as Error).message}`)
    }
  }

  static #query(args: Record<string, unknown>): DevQuery {
    return {
      ...(typeof args['web'] === 'string' && { web: args['web'] }),
      ...(typeof args['since'] === 'number' && { since: args['since'] }),
      ...(typeof args['limit'] === 'number' && { limit: args['limit'] }),
    }
  }
}
