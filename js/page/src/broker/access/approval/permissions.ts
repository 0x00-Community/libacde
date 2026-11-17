import type {
  AcdePermissionError,
  ApprovalCapability,
  FolderCapability,
  PermissionAnsweredEvent,
  PermissionDecision,
  PermissionPrompt,
  PermissionPromptEvent,
} from '@dopame/acde-api'
import { APPROVAL_CAPABILITIES } from '@dopame/acde-api/manifest'
import type { AppManifest, AppOrigin } from '../../../contract'

export type Permit = 'granted' | 'denied' | 'ask'

export type Approval = 'granted' | 'denied' | 'not-asked' | 'asking'

export interface DecisionStore {
  read(): Record<string, string> | null
  changed(entries: Record<string, string>): void
  flush(): void
}

export interface PermissionsHost {
  readonly store: DecisionStore | null
  label(appId: string): string | null
  supported(capability: string): boolean
  tell(event: PermissionPromptEvent | PermissionAnsweredEvent): void
  changed(appId: string, capability: ApprovalCapability, approval: Approval): void
  log(line: string): void
  asksAgain?(): boolean
  folders?(appId?: string): PickedFolder[]
  revokeFolder?(appId: string, id: string): boolean
}

export interface PickedFolder {
  readonly app: string
  readonly id: string
  readonly label: string
  readonly exists: boolean
}

interface Waiting {
  readonly prompt: PermissionPrompt
  readonly answered: Promise<boolean>
  readonly settle: (allowed: boolean) => void
}

export class Permissions {
  static readonly #APPROVAL: ReadonlySet<string> = new Set(APPROVAL_CAPABILITIES)
  readonly #host: PermissionsHost
  readonly #decisions: Map<string, 'granted' | 'denied'>
  readonly #asked = new Set<string>()
  readonly #waiting = new Map<number, Waiting>()
  #next = 1

  constructor(host: PermissionsHost) {
    this.#host = host
    const saved = host.store?.read() ?? {}
    this.#decisions = new Map(Object.entries(saved).filter((entry): entry is [string, 'granted' | 'denied'] => entry[1] === 'granted' || entry[1] === 'denied'))
  }

  static approval(capability: string): capability is ApprovalCapability {
    return Permissions.#APPROVAL.has(capability)
  }

  static folder(capability: string): string | null {
    return /^fs\.folder-[0-9a-f]{16}$/.test(capability) ? capability.slice('fs.'.length) : null
  }

  static #key(appId: string, capability: string): string {
    return `${appId}:${capability}`
  }

  static reason(capability: string, approval: Exclude<Approval, 'granted'>): string {
    if (approval === 'denied') return `"${capability}" is denied by the user`
    if (approval === 'asking') return `"${capability}" is waiting for the user's answer`
    return `"${capability}" is not asked for yet; ask for it with acde:capabilities request("${capability}")`
  }

  static error(app: string, capability: string, approval: Exclude<Approval, 'granted'> = 'denied'): AcdePermissionError {
    const error = new Error(`${app}: ${Permissions.reason(capability, approval)}`)
    const code = approval === 'denied' ? ('denied' as const) : approval === 'asking' ? ('asking' as const) : ('not_asked' as const)
    return Object.assign(error, { status: 403 as const, code, capability })
  }

  gated(app: AppManifest, origin: AppOrigin): ApprovalCapability[] {
    if (origin.system) return []
    const declared = Object.keys(app.capabilities).filter((name): name is ApprovalCapability => !!app.capabilities[name] && Permissions.approval(name))
    return declared
  }

  permit(app: AppManifest, origin: AppOrigin, capability: string): Permit {
    if (origin.system || !Permissions.approval(capability)) return 'granted'
    const key = Permissions.#key(app.id, capability)
    if (this.#stale(key)) return 'ask'
    return this.#decisions.get(key) ?? (this.#asked.has(key) ? 'denied' : 'ask')
  }

  approval(app: AppManifest, origin: AppOrigin, capability: string): Approval {
    if (origin.system || !Permissions.approval(capability)) return 'granted'
    if (this.#prompt(app.id, capability)) return 'asking'
    const permit = this.permit(app, origin, capability)
    return permit === 'ask' ? 'not-asked' : permit
  }

  approvals(app: AppManifest, origin: AppOrigin): Record<string, Approval> {
    return Object.fromEntries(this.gated(app, origin).map((name) => [name, this.approval(app, origin, name)]))
  }

  async ask(app: AppManifest, origin: AppOrigin, capability: ApprovalCapability, reason: PermissionPrompt['reason']): Promise<boolean> {
    const already = this.#prompt(app.id, capability)
    if (already) return already.answered
    const permit = this.permit(app, origin, capability)
    if (permit !== 'ask') return permit === 'granted'
    const key = Permissions.#key(app.id, capability)
    if (this.#stale(key)) {
      this.#decisions.delete(key)
      this.#save()
      this.#host.log(`${app.label} (${app.id}) was denied "${capability}" in an earlier run: asking again`)
    }
    let settle: (allowed: boolean) => void = () => {}
    const answered = new Promise<boolean>((resolve) => (settle = resolve))
    const prompt: PermissionPrompt = { id: this.#next++, app: app.id, label: app.label, capability, reason }
    this.#waiting.set(prompt.id, { prompt, answered, settle })
    this.#asked.add(key)
    this.#host.log(`${app.label} (${app.id}) waits for the user to allow "${capability}" (prompt ${prompt.id}; answer it with acde allow or acde deny, the acde driver, or acde:permissions)`)
    this.#host.changed(app.id, capability, 'asking')
    this.#host.tell({ type: 'permissionprompt', ...prompt })
    return answered
  }

  pending(): PermissionPrompt[] {
    return [...this.#waiting.values()].map((w) => ({ ...w.prompt }))
  }

  answer(id: number, allow: boolean): void {
    const waiting = this.#waiting.get(id)
    if (!waiting) throw Object.assign(new Error(`no approval prompt ${id}`), { status: 404 })
    this.#waiting.delete(id)
    const { app, capability } = waiting.prompt
    this.#decide(app, capability, allow ? 'granted' : 'denied')
    this.#host.tell({ type: 'permissionanswered', id, app, capability, allowed: allow })
    waiting.settle(allow)
  }

  list(appId?: string): PermissionDecision[] {
    const rows: PermissionDecision[] = []
    for (const [key, decision] of this.#decisions) {
      const at = key.lastIndexOf(':')
      const app = key.slice(0, at)
      const capability = key.slice(at + 1)
      if ((appId === undefined || app === appId) && Permissions.approval(capability)) rows.push({ app, label: this.#host.label(app), capability, decision })
    }
    for (const { app, id, label, exists } of this.#host.folders?.(appId) ?? []) {
      rows.push({ app, label: this.#host.label(app), capability: `fs.${id}` as FolderCapability, decision: 'granted', folder: { label, exists } })
    }
    return rows.sort((a, b) => a.app.localeCompare(b.app) || a.capability.localeCompare(b.capability))
  }

  revoke(appId: string, capability: string): boolean {
    const folder = Permissions.folder(capability)
    if (folder) {
      const had = this.#host.revokeFolder?.(appId, folder) ?? false
      this.#host.changed(appId, capability as ApprovalCapability, 'denied')
      return had
    }
    if (!Permissions.approval(capability)) throw Object.assign(new TypeError(`"${capability}" is not given by approval`), { status: 400 })
    const key = Permissions.#key(appId, capability)
    const had = this.#decisions.delete(key)
    this.#asked.add(key)
    if (had) this.#save()
    this.#host.changed(appId, capability, 'denied')
    return had
  }

  #prompt(appId: string, capability: string): Waiting | undefined {
    return [...this.#waiting.values()].find((w) => w.prompt.app === appId && w.prompt.capability === capability)
  }

  #stale(key: string): boolean {
    return this.#decisions.get(key) === 'denied' && !this.#asked.has(key) && this.#host.asksAgain?.() === true
  }

  #decide(appId: string, capability: ApprovalCapability, decision: 'granted' | 'denied'): void {
    this.#decisions.set(Permissions.#key(appId, capability), decision)
    this.#save()
    this.#host.changed(appId, capability, decision)
  }

  #save(): void {
    const store = this.#host.store
    if (!store) return
    store.changed(Object.fromEntries(this.#decisions))
    store.flush()
  }
}
