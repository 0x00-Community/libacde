import type { DesktopManifest } from '@dopame/acde-api'
import type { AppManifest } from '@dopame/acde-api/manifest'
import type { WebTier } from '../../page/src/contract'
import type { WebGrants } from '../../page/src/sys/natives'
import { join } from '../../page/src/sys/path'
import type { Found } from './app'

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
}

export function sessionOf(welcome: Welcome, wanted: 'app' | 'desktop' | 'service'): Session {
  if (welcome.kind !== wanted) throw new Error(`the session manager started this process as ${welcome.kind}, not ${wanted}`)
  const shared = { tier: welcome.tier, grants: welcome.grants, request: welcome.request ?? null, services: welcome.services ?? {} }
  if (welcome.kind === 'desktop') {
    const desktop = welcome.desktop
    if (!desktop) throw new Error('the session manager sent no desktop in its welcome')
    const page = join(welcome.dir, desktop.entry)
    return { kind: 'desktop', found: null, page, resources: welcome.resources, label: desktop.name, heapMb: desktop.heapMb, ...shared }
  }
  const app = welcome.app
  if (!app) throw new Error('the session manager sent no app in its welcome')
  const found = { dir: welcome.dir, app, resources: welcome.resources }
  return { kind: 'app', found, page: null, resources: welcome.resources, label: app.label, heapMb: app.heapMb, ...shared }
}
