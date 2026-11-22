export type IconName =
  | 'files'
  | 'browser'
  | 'editor'
  | 'office'
  | 'term'
  | 'projects'
  | 'dash'
  | 'team'
  | 'dopa'
  | 'settings'
  | 'store'
  | 'app'
  | 'timer'
  | 'cards'
  | 'list'
  | 'chart'
  | 'calc'
  | 'music'
  | 'video'
  | 'photo'
  | 'paint'
  | 'game'
  | 'chat'
  | 'mail'
  | 'calendar'
  | 'code'
  | 'book'
  | 'map'
  | 'weather'

export type ShellModule = 'windows' | 'packages' | 'session' | 'desktops' | 'inspect' | 'desktop' | 'setDefault' | 'permissions' | 'voice'

export interface IconsChangedEvent {
  readonly theme: string
}

export interface AcdeIcons {
  theme(): string
  lookup(names: readonly string[], size: number, scale?: number): Promise<string | null>
  on(type: 'changed', handler: (event: IconsChangedEvent) => void): () => boolean
}
