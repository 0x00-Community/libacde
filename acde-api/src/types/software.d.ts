export interface SoftwarePackage {
  readonly name: string
  readonly version: string
  readonly description: string
  readonly url: string
  readonly installed: string | null
}

export interface SoftwareUpdate {
  readonly name: string
  readonly from: string
  readonly to: string
}

export interface SoftwareState {
  readonly synced: number | null
  readonly community: SoftwarePackage[]
  readonly updates: SoftwareUpdate[]
}

export interface SoftwareChangedEvent {
  readonly type: 'softwarechanged'
}

export interface AcdeSoftware {
  state(): Promise<SoftwareState>
  refresh(): Promise<null>
  install(name: string): Promise<null>
  remove(name: string): Promise<null>
  update(): Promise<null>
  on(type: 'changed', handler: (event: SoftwareChangedEvent) => void): () => boolean
}
