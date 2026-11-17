import {
  API_LEVEL,
  CAPABILITY_MODULES,
  Manifest,
  SHELL_MODULES,
  WEB_CAPABILITIES,
  platformSupport,
  type CapabilityState,
  type CapabilityStatus,
  type RunningMode,
  type WindowFeatures,
} from '@dopame/acde-api/manifest'
import type { AppManifest, AppOrigin } from '../../contract'
import type { Approval } from './approval/permissions'

export interface CapabilityHost {
  readonly platform: string
  mode(): RunningMode
  running(service: string): boolean
  present(capability: string): boolean
  approval(app: AppManifest, origin: AppOrigin, capability: string): Approval
}

export interface CapabilitySnapshot {
  readonly level: number
  readonly platform: string
  readonly mode: RunningMode
  readonly window: WindowFeatures
  readonly statuses: readonly CapabilityStatus[]
}

export class CapabilityCheck {
  static readonly REASONS: Readonly<Record<CapabilityState, string>> = {
    available: 'available',
    'not-asked': 'not asked for yet',
    asking: "waiting for the user's answer",
    'not-declared': 'not declared in the manifest',
    unsupported: 'unsupported on this platform',
    'service-not-running': 'service not running',
    'shell-only': 'shell only',
    denied: 'denied by the user',
  }
  static readonly #SHELL: ReadonlySet<string> = new Set(SHELL_MODULES)
  static readonly #WEB: ReadonlySet<string> = new Set(WEB_CAPABILITIES)
  static readonly #STARTS: ReadonlySet<CapabilityState> = new Set<CapabilityState>(['available', 'not-asked', 'asking', 'denied'])
  static readonly #NO_FEATURES: WindowFeatures = { backdrop: 'unsupported' }
  readonly #host: CapabilityHost

  constructor(host: CapabilityHost) {
    this.#host = host
  }

  status(name: string, app: AppManifest | null, origin: AppOrigin): CapabilityStatus {
    const place = Manifest.place(name)
    if (place) {
      const declared = !app || Manifest.places(app.capabilities).includes(place)
      const state: CapabilityState = !declared ? 'not-declared' : !this.supported('fs') ? 'unsupported' : this.#approved(name, app, origin)
      return { name, kind: 'system', state, reason: CapabilityCheck.REASONS[state], optional: !!app && app.optional.includes('fs') }
    }
    if (CapabilityCheck.#WEB.has(name)) {
      const state: CapabilityState = this.supported(name) ? 'available' : 'unsupported'
      return { name, kind: 'system', state, reason: CapabilityCheck.REASONS[state], optional: false }
    }
    const system = Manifest.CAPABILITIES.has(name)
    const declared = !app || (system ? !!app.capabilities[name] : app.services.includes(name) || !!app.capabilities[name])
    const third = !!app && !origin.system && CapabilityCheck.#SHELL.has(name)
    const state: CapabilityState = third
      ? 'shell-only'
      : !declared
        ? 'not-declared'
        : system
          ? !this.supported(name) ? 'unsupported' : this.#approved(name, app, origin)
          : this.#host.running(name) ? 'available' : 'service-not-running'
    return {
      name,
      kind: system ? 'system' : 'service',
      state,
      reason: CapabilityCheck.REASONS[state],
      optional: !!app && app.optional.includes(name),
    }
  }

  #approved(name: string, app: AppManifest | null, origin: AppOrigin): CapabilityState {
    const approval = app ? this.#host.approval(app, origin, name) : 'granted'
    return approval === 'granted' ? 'available' : approval
  }

  static asks(state: CapabilityState): boolean {
    return state === 'not-asked' || state === 'asking'
  }

  declared(app: AppManifest | null): string[] {
    if (!app) return [...Manifest.CAPABILITIES]
    const names = Object.keys(app.capabilities).filter((name) => !!app.capabilities[name])
    return [...new Set([...names, ...Manifest.places(app.capabilities).map((place) => `fs.${place}`), ...app.services])]
  }

  statuses(app: AppManifest | null, origin: AppOrigin): CapabilityStatus[] {
    return [...new Set([...Manifest.CAPABILITIES, ...WEB_CAPABILITIES, ...this.declared(app)])].map((name) => this.status(name, app, origin))
  }

  missing(app: AppManifest, origin: AppOrigin): CapabilityStatus[] {
    return this.declared(app)
      .map((name) => this.status(name, app, origin))
      .filter((status) => !status.optional && !CapabilityCheck.#STARTS.has(status.state))
  }

  refusal(app: AppManifest, origin: AppOrigin): Error | null {
    const missing = this.missing(app, origin)
    if (!missing.length) return null
    const where = this.#where()
    const lacking = (status: CapabilityStatus) => status.state === 'unsupported' && status.kind === 'system'
    const names = missing.map((status) => `"${status.name}" (${lacking(status) ? `not available on ${where}` : status.reason})`).join(', ')
    const stopped = missing.some((status) => status.state === 'service-not-running')
    const optional = missing.some(lacking) ? '; list it in "optional" to run without it' : ''
    const error = new Error(`${app.label} needs ${names}${optional}`)
    return Object.assign(error, stopped ? { status: 503, code: 'service_not_running' } : { status: 404, code: 'unsupported_app' })
  }

  refusedModules(app: AppManifest, origin: AppOrigin): Record<string, string> {
    const refused: Record<string, string> = {}
    for (const name of CAPABILITY_MODULES) {
      if (!Manifest.importRule(name, app, origin).allowed) continue
      const status = this.status(name, app, origin)
      if (!CapabilityCheck.#STARTS.has(status.state) && status.state !== 'not-declared') refused[`acde:${name}`] = `acde:${name}: "${name}" is ${status.reason}`
    }
    return refused
  }

  snapshot(app: AppManifest | null, origin: AppOrigin): CapabilitySnapshot {
    const mode = this.#host.mode()
    const row = platformSupport(this.#host.platform, mode)
    return { level: API_LEVEL, platform: this.#host.platform, mode, window: row?.window ?? CapabilityCheck.#NO_FEATURES, statuses: this.statuses(app, origin) }
  }

  #where(): string {
    const mode = this.#host.mode()
    const name = { win32: 'Windows', darwin: 'macOS', linux: 'Linux' }[this.#host.platform] ?? this.#host.platform
    return mode === 'session' ? `${name} sessions` : `${name} host windows`
  }

  supported(name: string): boolean {
    const row = platformSupport(this.#host.platform, this.#host.mode())
    return !!row && !row.unsupported.includes(name) && this.#host.present(name)
  }

  static module(snapshot: CapabilitySnapshot, ask: (name: string) => Promise<CapabilityStatus>): { module: object; update(status: CapabilityStatus): void } {
    const known = new Map(snapshot.statuses.map((status) => [status.name, status]))
    const update = (status: CapabilityStatus) => void known.set(status.name, status)
    const get = (name: string): CapabilityStatus => {
      const key = String(name)
      const found = known.get(key)
      if (found) return { ...found }
      return { name: key, kind: 'service', state: 'not-declared', reason: CapabilityCheck.REASONS['not-declared'], optional: false }
    }
    const module = Object.freeze({
      level: snapshot.level,
      platform: snapshot.platform,
      mode: snapshot.mode,
      window: Object.freeze({ ...snapshot.window }),
      get,
      list: (): CapabilityStatus[] => [...known.values()].map((status) => ({ ...status })),
      request: async (name: string): Promise<CapabilityStatus> => {
        const status = known.get(String(name))
        if (!status || !CapabilityCheck.asks(status.state)) return get(name)
        const answered = await ask(String(name))
        update(answered)
        return { ...answered }
      },
    })
    return { module, update }
  }
}
