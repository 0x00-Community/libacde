import type { DesktopManifest } from '@acde/api'
import type { AppManifest } from '@acde/api/manifest'
import type { WebTier } from '../../../page/src/contract'
import type { WebGrants } from '../../../page/src/sys/natives'
import { join } from '../../../page/src/sys/path'
import type { Found } from '../app/app'

export interface ServiceShape {
  readonly methods: readonly string[]
  readonly values: Readonly<Record<string, unknown>>
  readonly sync: Readonly<Record<string, unknown>>
  readonly callable: boolean
}

export interface Welcome {
  readonly api: number
  readonly kind: 'app' | 'desktop' | 'service'
  readonly level: string
  readonly dir: string
  readonly app?: AppManifest
  readonly desktop?: DesktopManifest & { readonly heapMb?: number }
  readonly resources: { prefix: string; dirs: string[] }[]
  readonly tier: WebTier
  readonly grants: WebGrants
  readonly request: Record<string, unknown> | null
  readonly services?: Readonly<Record<string, ServiceShape>>
  readonly name?: string
  readonly module?: string
  readonly modules?: readonly string[]
  readonly stateDir?: string
  readonly backend?: { readonly role: string; readonly up: boolean }
  readonly port?: { readonly data: unknown }
  readonly size?: readonly [number, number]
  readonly outerSize?: readonly [number, number]
  readonly session?: { readonly own: boolean }
  readonly lock?: boolean
  readonly instance?: number
}

export function windowSize(given: unknown): readonly [number, number] | null {
  if (!Array.isArray(given) || given.length !== 2) return null
  const [width, height] = given.map((n) => (typeof n === 'number' && Number.isFinite(n) ? Math.round(n) : 0))
  return width! >= 1 && height! >= 1 && width! <= 16384 && height! <= 16384 ? [width!, height!] : null
}

export function windowGrant(
  welcome: Pick<Welcome, 'size' | 'outerSize'> | undefined,
  manifest: readonly [number, number] | null,
): { readonly size?: readonly [number, number]; readonly outerSize?: readonly [number, number] } {
  const outerSize = windowSize(welcome?.outerSize)
  if (outerSize) return { outerSize }
  const size = windowSize(welcome?.size) ?? manifest
  return size ? { size } : {}
}

export interface ManagerEvent {
  readonly type: string
  readonly [field: string]: unknown
}

export interface ManagerLink {
  readonly welcome: Welcome
  call(service: string, method: string, args?: readonly unknown[]): Promise<unknown>
  onEvents(callback: () => void): void
  pump(): ManagerEvent[]
  send?(frame: object): void
  connectBackend?(): Promise<BackendLink>
}

export function withoutUndefined(value: unknown, at: () => string): unknown {
  if (value === undefined) throw new TypeError(`${at()} is undefined; leave it out or pass null`)
  if (Array.isArray(value)) return value.map((item, i) => withoutUndefined(item, () => `${at()}[${i}]`))
  if (typeof value !== 'object' || value === null) return value
  const proto = Object.getPrototypeOf(value)
  if (proto !== Object.prototype && proto !== null) return value
  const kept = Object.entries(value).filter(([, item]) => item !== undefined)
  return Object.fromEntries(kept.map(([key, item]) => [key, withoutUndefined(item, () => `${at()}.${key}`)]))
}

export function nativeArgs(service: string, method: string, args: readonly unknown[] = []): unknown[] {
  let length = args.length
  while (length > 0 && args[length - 1] === undefined) length--
  return args.slice(0, length).map((arg, i) => withoutUndefined(arg, () => `${service}.${method}: argument ${i + 1}`))
}

export function rejectingCalls(link: ManagerLink): ManagerLink {
  const call = (service: string, method: string, args?: readonly unknown[]): Promise<unknown> => {
    try {
      return link.call(service, method, nativeArgs(service, method, args))
    } catch (error) {
      return Promise.reject(error)
    }
  }
  return {
    welcome: link.welcome,
    call,
    onEvents: (callback) => link.onEvents(callback),
    pump: () => link.pump(),
    ...(link.send && { send: (frame: object) => link.send!(frame) }),
    ...(link.connectBackend && { connectBackend: () => link.connectBackend!() }),
  }
}

export interface BackendLink {
  readonly role: string
  write(data: string | Uint8Array): void
  onData(handler: (chunk: Uint8Array) => void): void
  onClose(handler: () => void): void
  close(): void
}

export interface Session {
  readonly kind: 'app' | 'desktop'
  readonly found: Found | null
  readonly page: string | null
  readonly resources: Found['resources']
  readonly label: string
  readonly heapMb: number | undefined
  readonly tier: WebTier | null
  readonly grants: WebGrants | null
  readonly request: Record<string, unknown> | null
  readonly services: Readonly<Record<string, ServiceShape>>
  readonly own: boolean
  readonly lock: boolean
  readonly instance?: number
}

export function sessionOf(welcome: Welcome, wanted: 'app' | 'desktop' | 'service'): Session {
  if (welcome.kind !== wanted) throw new Error(`the session manager started this process as ${welcome.kind}, not ${wanted}`)
  const own = welcome.session?.own === true
  const lock = welcome.kind === 'desktop' && welcome.lock === true
  const shared = { tier: welcome.tier, grants: welcome.grants, request: welcome.request ?? null, services: welcome.services ?? {}, own, lock }
  if (welcome.kind === 'desktop') {
    const desktop = welcome.desktop
    if (!desktop) throw new Error('the session manager sent no desktop in its welcome')
    const page = join(welcome.dir, lock ? (desktop.lock ?? desktop.entry) : desktop.entry)
    return { kind: 'desktop', found: null, page, resources: welcome.resources, label: desktop.name, heapMb: desktop.heapMb, ...shared }
  }
  const app = welcome.app
  if (!app) throw new Error('the session manager sent no app in its welcome')
  const found = { dir: welcome.dir, app, resources: welcome.resources }
  const instance = Number.isInteger(welcome.instance) && welcome.instance! > 0 ? { instance: welcome.instance } : {}
  return { kind: 'app', found, page: null, resources: welcome.resources, label: app.label, heapMb: app.heapMb, ...shared, ...instance }
}
