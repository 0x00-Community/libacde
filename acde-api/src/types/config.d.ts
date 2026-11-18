import type { Capabilities, IconName, ShellModule } from './manifest'

export type AcdeConfig = AppConfig | DesktopConfig

export interface ConfigCommon {
  readonly $schema?: string
  readonly name?: string
  readonly version?: string
  readonly apiLevel?: number
  readonly public?: string
}

export interface WindowAppConfig {
  readonly id?: string
  readonly label?: string
  readonly icon?: IconName | (string & {})
  readonly entry?: string
  readonly capabilities?: Capabilities
  readonly optional?: readonly string[]
  readonly services?: readonly string[]
  readonly heapMb?: number
  readonly size?: readonly [number, number]
  readonly multiple?: boolean
  readonly frame?: boolean
  readonly mimeTypes?: readonly string[]
  readonly backend?: false | BackendConfig
}

export interface BackendConfig {
  readonly entry?: string
  readonly port?: number
}

export interface ServiceConfig {
  readonly module: string
  readonly modules?: readonly ShellModule[]
}

export type Framework = 'auto' | 'none' | 'next' | 'nuxt' | 'sveltekit' | 'astro' | 'react-router' | 'remix' | 'angular' | 'vite' | 'cra' | 'script'

export interface AppConfig extends ConfigCommon, WindowAppConfig {
  readonly type: 'app'
  readonly framework?: Framework
}

export interface DesktopConfig extends ConfigCommon {
  readonly type: 'desktop'
  readonly id?: string
  readonly label?: string
  readonly description?: string
  readonly entry?: string
  readonly heapMb?: number
  readonly services?: Readonly<Record<string, string | ServiceConfig>>
  readonly apps?: readonly (WindowAppConfig & { readonly id: string; readonly entry: string })[]
}
