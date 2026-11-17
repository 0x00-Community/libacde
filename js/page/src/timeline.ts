import { Level, Log } from './log'

export class StartupTimeline {
  static readonly #seen = new Set<string>()

  static mark(name: string): void {
    if (StartupTimeline.#seen.has(name)) return
    StartupTimeline.#seen.add(name)
    Log.write(Level.Info, 'acde_host', `startup: ${name} ms=${Log.clock().toFixed(0)}`)
  }
}
