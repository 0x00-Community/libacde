export type IconName = string

export type ShellModule = 'windows' | 'packages' | 'session' | 'desktops' | 'inspect' | 'desktop' | 'setDefault' | 'permissions' | 'voice' | 'software'

export interface IconsChangedEvent {
  readonly theme: string
}

export interface AcdeIcons {
  theme(): string
  lookup(names: readonly string[], size: number, scale?: number): Promise<string | null>
  on(type: 'changed', handler: (event: IconsChangedEvent) => void): () => boolean
}
