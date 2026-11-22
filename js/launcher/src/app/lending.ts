import type { FileFilter, Lent, LentFile } from '../../../page/src/sys/natives'
import { sys } from '../../../page/src/sys/natives'
import type { ManagerLink } from '../manager/manager'

export type LentEntry = Pick<Lent, 'name' | 'label' | 'kind' | 'writable'>
export type Adopted = { readonly folders: readonly string[]; readonly files: readonly string[] }
type Request = Record<string, unknown>
type Answer = { readonly fds?: unknown }

export interface LendingHost {
  readonly manager: ManagerLink | null
  app(): string | undefined
  post(message: object): void
}

export class Lending {
  readonly #host: LendingHost

  constructor(host: LendingHost) {
    this.#host = host
  }

  adopt(lent: unknown, fds: unknown): Adopted {
    const given = Array.isArray(fds) ? fds.filter((fd): fd is string => typeof fd === 'string') : []
    const none = { folders: [], files: [] }
    if (!given.length) return none
    return sys().adoptLent?.(this.#host.app() ?? '', Array.isArray(lent) ? lent : [], given) ?? none
  }

  lend(lent: unknown, fds: unknown): Adopted {
    const adopted = this.adopt(lent, fds)
    if (adopted.folders.length) this.#host.post({ type: 'fsroots', added: adopted.folders, removed: [] })
    return adopted
  }

  unlend(name: string): boolean {
    const app = this.#host.app()
    if (!app || !name || !sys().unlendRoot?.(app, name)) return false
    this.#host.post({ type: 'fsroots', removed: [name] })
    return true
  }

  async revoke(name: string): Promise<void> {
    const refused = () => Object.assign(new TypeError(`${name} is not a folder lent to this app`), { status: 400, code: 'EINVAL' })
    const manager = this.#host.manager
    if (!this.#host.app()) throw refused()
    if (manager) await manager.call('fs', 'unlend', [{ name }])
    if (!this.unlend(name) && !manager) throw refused()
  }

  async folder(title: string, writable: boolean): Promise<{ readonly name: string; readonly label: string } | null> {
    const app = this.#host.app()
    const manager = this.#host.manager
    if (!app) return null
    if (!manager) return (await sys().pickFolder?.(app, title, writable)) ?? null
    const lent = (await manager.call('fs', 'pick', [{ kind: 'folder', title, writable }])) as (Lent & Answer) | null
    if (!lent) return null
    const [name] = this.lend([lent], lent.fds).folders
    return name === undefined ? null : { name, label: lent.label || name }
  }

  async openFiles(title: string, multiple: boolean, filters: readonly FileFilter[]): Promise<readonly LentFile[] | null> {
    const app = this.#host.app()
    const manager = this.#host.manager
    if (!app) return null
    if (!manager) return (await sys().pickFiles?.(app, title, multiple, filters))?.files ?? null
    const picked = (await manager.call('fs', 'pick', [{ kind: 'open', title, multiple, filters }])) as ({ files?: unknown } & Answer) | null
    const files = this.#files(picked?.files, picked?.fds)
    return files.length ? files : null
  }

  async saveFile(title: string, name: string, filters: readonly FileFilter[]): Promise<LentFile | null> {
    const app = this.#host.app()
    const manager = this.#host.manager
    if (!app) return null
    if (!manager) return (await sys().pickSave?.(app, title, name, filters)) ?? null
    const lent = (await manager.call('fs', 'pick', [{ kind: 'save', title, name, filters }])) as (Lent & Answer) | null
    return lent ? (this.#files([lent], lent.fds)[0] ?? null) : null
  }

  async dropped(uris: string): Promise<readonly LentEntry[] | null> {
    const manager = this.#host.manager
    if (!manager || !uris) return null
    const given = (await manager.call('fs', 'dropped', [{ uris }])) as ({ files?: unknown } & Answer) | null
    const entries = Lending.#entries(given?.files)
    const { folders, files } = this.lend(entries, given?.fds)
    const taken = entries.filter((entry) => folders.includes(entry.name) || files.includes(entry.name))
    return taken.length ? taken.map(({ name, label, kind, writable }) => ({ name, label: label || name, kind, writable })) : null
  }

  request(event: Readonly<Record<string, unknown>>): Request | null {
    const adopted = this.lend(event['lent'], event['fds'])
    const request = Lending.entered(event['request'] as Request | undefined, event['lent'], adopted)
    const [root] = adopted.folders
    return root === undefined ? request : { ...request, root }
  }

  static entered(request: Request | null | undefined, lent: unknown, adopted: Adopted): Request | null {
    if (!request) return null
    const { entry: _entry, lend: _lend, ...rest } = request
    const name = Lending.#named(request)
    if (name === null) return rest
    const taken = [...adopted.folders, ...adopted.files]
    const found = Lending.#entries(lent).find((given) => given.name === name && taken.includes(given.name))
    if (!found) return rest
    return { ...rest, entry: { name: found.name, kind: found.kind, label: found.label || found.name, writable: found.writable } }
  }

  static #named(request: Request): string | null {
    const entry = request['entry'] as { name?: unknown } | undefined
    if (typeof entry?.name === 'string') return entry.name
    const [handed] = Array.isArray(request['lend']) ? (request['lend'] as { name?: unknown }[]) : []
    return typeof handed?.name === 'string' ? handed.name : null
  }

  #files(lent: unknown, fds: unknown): LentFile[] {
    const entries = Lending.#entries(lent)
    const { files } = this.adopt(entries, fds)
    return entries.filter((entry) => files.includes(entry.name)).map(({ name, label, writable }) => ({ name, label: label || name, writable }))
  }

  static #entries(lent: unknown): Lent[] {
    return Array.isArray(lent) ? (lent as Lent[]) : []
  }
}
