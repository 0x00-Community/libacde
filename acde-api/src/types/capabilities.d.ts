export type Platform = 'linux' | 'win32' | 'darwin'

export type RunningMode = 'session' | 'desktop' | 'standalone'

export type FeatureSupport = 'full' | 'approximated' | 'unsupported'

export interface WindowFeatures {
  readonly backdrop: FeatureSupport
}

export interface PlatformSupport {
  readonly unsupported: readonly string[]
  readonly window: WindowFeatures
}

export type CapabilityState = 'available' | 'not-asked' | 'asking' | 'denied' | 'not-declared' | 'unsupported' | 'service-not-running' | 'shell-only'

export interface CapabilityStatus {
  readonly name: string
  readonly kind: 'system' | 'service'
  readonly state: CapabilityState
  readonly reason: string
  readonly optional: boolean
}

export interface AcdeCapabilities {
  readonly level: number
  readonly platform: Platform
  readonly mode: RunningMode
  readonly window: WindowFeatures
  get(name: string): CapabilityStatus
  list(): CapabilityStatus[]
  request(name: string): Promise<CapabilityStatus>
}
