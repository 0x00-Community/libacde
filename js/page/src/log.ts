import { natives } from './sys/natives'

export const enum Level {
  Error = 1,
  Warn,
  Info,
  Debug,
  Trace,
}

export class Log {
  static readonly #enabled = new Map<string, boolean>()

  static enabled(target: string, level = Level.Debug): boolean {
    const key = `${level}${target}`
    let on = Log.#enabled.get(key)
    if (on === undefined) Log.#enabled.set(key, (on = natives().host.logEnabled(level, target)))
    return on
  }

  static write(level: Level, target: string, text: string): void {
    natives().host.log(level, target, text)
  }

  static clock(): number {
    return natives().host.clock()
  }
}
