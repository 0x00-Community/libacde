import type { NativeWindow } from '@dopame/acde-api'
import type { AppManifest, Capabilities, ShellModule } from '@dopame/acde-api/manifest'
import type { CapabilitySnapshot } from './broker/access/capabilities'
import type { Approval } from './broker/access/approval/permissions'
import type { DevRoute } from './devroutes'
import type { Languages } from './language'

export type { AppManifest, Capabilities }

export interface AppOrigin {
  readonly level: number
  readonly system: boolean
}

export interface WebTier {
  readonly level: number
  readonly system?: boolean
  readonly shell: readonly ShellModule[]
  readonly refused: Readonly<Record<string, string>>
  readonly capabilities?: CapabilitySnapshot
  readonly approvals?: Readonly<Record<string, Approval>>
}

export interface ServiceShape {
  readonly methods: readonly string[]
  readonly values: Readonly<Record<string, unknown>>
  readonly sync: Readonly<Record<string, unknown>>
  readonly callable: boolean
}

export interface WireMenu {
  readonly id: string
  readonly label: string
  readonly items: readonly (WireMenuSeparator | WireMenuItem)[]
}

interface WireMenuSeparator {
  readonly type: 'separator'
}

interface WireMenuItem {
  readonly id: string
  readonly label: string
  readonly shortcut?: string
  readonly checked?: boolean
  readonly disabled?: boolean
  readonly action: boolean
}

export interface StoragePlace {
  readonly key: string
  readonly dir: string | null
  readonly legacy: string | null
}

export interface NativeAccessibility {
  tree(pid: number, title: string | null, limit: number): Promise<string | null>
  act(node: string, action: string, text?: string | null): Promise<boolean>
}

export interface WebInit {
  readonly kind: 'desktop' | 'window'
  readonly name: string
  readonly token: bigint
  readonly window: number
  readonly width: number
  readonly height: number
  readonly scale: number
  readonly screen?: { width: number; height: number; workArea?: { x: number; y: number; width: number; height: number } }
  readonly dark: boolean
  readonly languages: Languages
  readonly navigator: { readonly userAgent: string; readonly platform: string }
  readonly services: Readonly<Record<string, ServiceShape>>
  readonly resources: { prefix: string; dirs: string[] }[]
  readonly page?: string
  readonly standalone?: string | null
  readonly app?: AppManifest
  readonly theme?: string
  readonly request?: Record<string, unknown> | null
  readonly url?: string | null
  readonly sockets?: string
  readonly channel?: boolean
  readonly devState?: unknown
  readonly devRoutes?: readonly DevRoute[]
  readonly devInit?: readonly string[]
  readonly tier: WebTier
  readonly windows?: readonly NativeWindow[]
}
