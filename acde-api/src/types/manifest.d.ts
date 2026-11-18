import type { SharedPlace } from './places'
import type { IconName, ShellModule } from './names'

export type { IconName, ShellModule }

export interface PackageManifest {
  readonly apiLevel?: number
  readonly desktops?: readonly DesktopManifest[]
  readonly apps?: readonly AppManifest[]
  readonly assets?: Readonly<Record<string, readonly string[]>>
  readonly services?: readonly ServiceManifest[]
}

export interface DesktopManifest {
  readonly id: string
  readonly name: string
  readonly description?: string
  readonly entry: string
  readonly services?: readonly (string | DesktopService)[]
  readonly heapMb?: number
}

export interface DesktopService {
  readonly name: string
  readonly modules?: readonly ShellModule[]
}

export interface AppManifest {
  readonly id: string
  readonly label: string
  readonly icon: IconName | (string & {})
  readonly entry: string
  readonly capabilities?: Capabilities
  readonly heapMb?: number
  readonly size?: readonly [number, number]
  readonly multiple?: boolean
  readonly frame?: boolean
  readonly mimeTypes?: readonly string[]
  readonly schemes?: readonly string[]
  readonly requests?: readonly string[]
  readonly backend?: AppBackend
  readonly optional?: readonly string[]
  readonly services?: readonly string[]
}

export interface AppBackend {
  readonly entry: string
  readonly port?: number
}

export interface Capabilities {
  readonly fs?: boolean | { readonly roots: readonly SharedPlace[] }
  readonly terminal?: boolean
  readonly network?: boolean
  readonly system?: boolean
  readonly bluetooth?: boolean
  readonly audio?: boolean
  readonly devices?: boolean
  readonly power?: boolean
  readonly input?: boolean
  readonly display?: boolean
  readonly idleDetection?: boolean
  readonly office?: boolean
  readonly launch?: boolean
  readonly slots?: boolean
  readonly open?: boolean | readonly string[]
  readonly openFiles?: boolean
  readonly windows?: boolean
  readonly packages?: boolean
  readonly session?: boolean
  readonly desktops?: boolean
  readonly inspect?: boolean
  readonly desktop?: boolean
  readonly setDefault?: boolean
  readonly camera?: boolean
  readonly microphone?: boolean
  readonly [service: string]: unknown
}

export interface ServiceManifest {
  readonly name: string
  readonly module: string
}
