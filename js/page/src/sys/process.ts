import { sys } from './natives'

export type Env = Record<string, string | undefined>

let env: Env | null = null

export const process = {
  get env(): Env {
    return (env ??= sys().env())
  },
  get argv(): readonly string[] {
    return sys().argv
  },
  get platform(): string {
    return sys().platform
  },
  get pid(): number {
    return sys().pid
  },
  exit(code = 0): never {
    return sys().exit(code)
  },
  getuid(): number {
    return sys().getuid()
  },
  kill(pid: number, signal: string = 'SIGTERM'): true {
    if (!sys().kill(pid, signal)) throw Object.assign(new Error(`kill ${pid}: no such process`), { code: 'ESRCH' })
    return true
  },
  stdout: { write: (text: string): boolean => (sys().stdout(text), true) },
  stderr: { write: (text: string): boolean => (sys().stderr(text), true) },
}

export function stopGroup(pid: number, alive: () => boolean, ms: number): ReturnType<typeof setTimeout> {
  sys().kill(-pid, 'SIGTERM')
  return setTimeout(() => {
    if (alive()) sys().kill(-pid, 'SIGKILL')
  }, ms)
}

const signalListeners = new Map<string, (() => void)[]>()
let listening = false
export function onceSignal(name: 'SIGINT' | 'SIGTERM' | 'SIGHUP', listener: () => void): void {
  signalListeners.set(name, [...(signalListeners.get(name) ?? []), listener])
  if (listening) return
  listening = true
  sys().onSignal((signal) => {
    const listeners = signalListeners.get(signal) ?? []
    signalListeners.delete(signal)
    if (!listeners.length && signal !== 'SIGHUP') sys().exit(signal === 'SIGINT' ? 130 : 143)
    for (const run of listeners) run()
  })
}
