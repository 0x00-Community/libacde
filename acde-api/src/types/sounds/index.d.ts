export interface SoundsChangedEvent {
  readonly theme: string
}

export interface AcdeSounds {
  theme(): string
  lookup(names: readonly string[]): Promise<string | null>
  on(type: 'changed', handler: (event: SoundsChangedEvent) => void): () => boolean
}

declare module '../page' {
  interface AcdePage {
    readonly sounds?: AcdeSounds
  }
}
