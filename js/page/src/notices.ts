import { Level, Log } from './log'

export class Notices {
  readonly #handlers = new Map<string, Set<(detail: unknown) => void>>()

  on(type: string, handler: (detail: unknown) => void): () => boolean {
    const set = this.#handlers.get(type) ?? new Set()
    set.add(handler)
    this.#handlers.set(type, set)
    return () => set.delete(handler)
  }

  emit(type: string, detail: unknown): void {
    for (const handler of this.#handlers.get(type) ?? []) {
      try {
        handler(detail)
      } catch (error) {
        Log.write(Level.Warn, 'acde_host', `${type} handler: ${(error as Error)?.message ?? String(error)}`)
      }
    }
  }

  for(scope: string, types: readonly string[]): (type: string, handler: (detail: unknown) => void) => () => boolean {
    return (type, handler) => {
      if (!types.includes(type)) throw new TypeError(`acde:${scope} has no ${String(type)} event (${types.join(', ')})`)
      return this.on(`${scope}:${type}`, handler)
    }
  }
}
