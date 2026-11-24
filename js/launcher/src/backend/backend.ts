import type { AppManifest } from '@acde/api/manifest'
import { Level, Log } from '../../../page/src/log'
import { PageSource } from '../../../page/src/page'
import { mkdirSync, readDirOr, rmSync, stat } from '../../../page/src/sys/fs'
import { sys } from '../../../page/src/sys/natives'
import { ownRuntimeDir, xdgHome } from '../../../page/src/sys/os'
import { dirname, join } from '../../../page/src/sys/path'
import { process, stopGroup } from '../../../page/src/sys/process'
import type { Found } from '../app/app'

type Mode = 'channel' | 'sockets'
type Exit = (code: number | null, signal: number | null) => void

export type BackendInit = { readonly channel: true } | { readonly sockets: string } | Record<string, never>

export const BackendPlace = {
  ENV: ['HOME', 'USER', 'PATH', 'LANG', 'LANGUAGE', 'TZ', 'XDG_RUNTIME_DIR'] as readonly string[],
  SUN_PATH: { darwin: 104, freebsd: 104, linux: 108 } as Readonly<Record<string, number>>,

  windows(): boolean {
    return sys().spawnBackend !== undefined
  },

  socketDir(id: string, pid = process.pid, root = ownRuntimeDir()): string {
    return join(root, String(pid), id)
  },

  dataDir(id: string): string {
    return join(xdgHome('data'), 'acde', 'apps', id)
  },

  stale(names: readonly string[], own: number, alive: (pid: number) => boolean): string[] {
    return names.filter((name) => /^[1-9]\d*$/.test(name) && Number(name) !== own && !alive(Number(name)))
  },

  sweep(): void {
    const root = ownRuntimeDir()
    for (const name of BackendPlace.stale(readDirOr(root), process.pid, (pid) => sys().kill(pid, '0'))) {
      rmSync(join(root, name), { recursive: true, force: true })
    }
  },

  tooLong(socket: string, platform = process.platform): string | null {
    const max = BackendPlace.SUN_PATH[platform] ?? 104
    const bytes = new globalThis.TextEncoder().encode(socket).length
    return bytes < max ? null : `the socket path ${socket} is ${bytes} bytes, the system allows ${max - 1}: use a shorter app id or TMPDIR`
  },

  env(key: string): string | undefined {
    const env = process.env
    if (env[key] !== undefined || process.platform !== 'win32') return env[key]
    const name = Object.keys(env).find((candidate) => candidate.toUpperCase() === key.toUpperCase())
    return name === undefined ? undefined : env[name]
  },

  mode(argv: readonly string[]): Mode {
    return /(^|[\\/])node(\.exe)?$/i.test(argv[0] ?? '') ? 'channel' : 'sockets'
  },

  argv(path: string | undefined, preload: string, server: string, prefer?: string): string[] | null {
    const windows = process.platform === 'win32'
    const find = (name: string) => {
      for (const dir of (path ?? '').split(windows ? ';' : ':')) {
        const exe = dir && join(dir, windows ? `${name}.exe` : name)
        if (exe && stat(exe)?.isFile) return exe
      }
      return null
    }
    const href = windows ? `file:///${encodeURI(preload.replaceAll('\\', '/'))}` : new globalThis.URL(`file://${preload}`).href
    for (const name of prefer === 'node' ? ['node', 'bun'] : ['bun', 'node']) {
      const exe = find(name)
      if (exe) return name === 'bun' ? [exe, '--preload', preload, server] : [exe, '--import', href, server]
    }
    return null
  },

  init(app: AppManifest): BackendInit {
    if (!app.backend) return {}
    if (!BackendPlace.windows()) return { sockets: BackendPlace.socketDir(app.id) }
    const argv = BackendPlace.argv(BackendPlace.env('PATH'), 'acde-listen.js', 'server.js', process.env['ACDE_RUNTIME'])
    if (argv && BackendPlace.mode(argv) === 'channel') return { channel: true }
    const base = BackendPlace.env('XDG_RUNTIME_DIR') || BackendPlace.env('TMP') || BackendPlace.env('TEMP') || ''
    return { sockets: join(base, 'acde', 'b') }
  },
}

export class AppBackend {
  static readonly RESTARTS = 3
  static readonly CRASH_WINDOW_MS = 60_000
  static readonly RESTART_MS = 500
  static readonly STOP_MS = 3000

  #found: Found
  #pid: number | null = null
  #ended: Promise<void> = Promise.resolve()
  #end: () => void = () => undefined
  #generation = 0
  #crashes: number[] = []

  constructor(found: Found) {
    this.#found = found
  }

  get #app(): AppManifest {
    return this.#found.app
  }

  get #name(): string {
    return `${this.#app.id}#backend`
  }

  #report(line: string, level = Level.Info): void {
    Log.write(level, 'acde_host', `[${this.#name}] ${line}`)
  }

  init(): BackendInit {
    return BackendPlace.init(this.#app)
  }

  get running(): boolean {
    return this.#pid !== null
  }

  start(): void {
    if (!this.#app.backend || this.#pid !== null) return
    this.#crashes = []
    const sockets = BackendPlace.windows() ? null : BackendPlace.socketDir(this.#app.id)
    if (!this.#prepare(sockets)) return
    this.#spawn(++this.#generation)
  }

  async restart(found: Found = this.#found): Promise<void> {
    await this.stop()
    this.#found = found
    this.start()
  }

  async stop(): Promise<void> {
    this.#generation++
    const pid = this.#pid
    if (pid === null) return
    const timer = this.#signalStop(pid)
    await this.#ended
    if (timer) clearTimeout(timer)
    if (!BackendPlace.windows()) rmSync(BackendPlace.socketDir(this.#app.id), { recursive: true, force: true })
    this.#report('stopped')
  }

  #signalStop(pid: number): ReturnType<typeof setTimeout> | null {
    const stopBackend = sys().stopBackend
    if (!stopBackend) return stopGroup(pid, () => this.#pid === pid, AppBackend.STOP_MS)
    stopBackend(pid, AppBackend.STOP_MS)
    return null
  }

  async close(): Promise<void> {
    await this.stop()
    if (!BackendPlace.windows()) rmSync(join(ownRuntimeDir(), String(process.pid)), { recursive: true, force: true })
  }

  #prepare(sockets: string | null): boolean {
    const tooLong = sockets && BackendPlace.tooLong(join(sockets, `${this.#app.backend!.port}.sock`))
    if (tooLong) return this.#refuse(tooLong)
    try {
      if (sockets) {
        BackendPlace.sweep()
        mkdirSync(ownRuntimeDir(), { recursive: true, mode: 0o700 })
        mkdirSync(sockets, { recursive: true, mode: 0o700 })
        for (const name of readDirOr(sockets)) if (name.endsWith('.sock')) rmSync(join(sockets, name), { force: true })
      }
      mkdirSync(BackendPlace.dataDir(this.#app.id), { recursive: true })
    } catch (error) {
      return this.#refuse((error as Error).message)
    }
    return true
  }

  #refuse(why: string): false {
    this.#report(`not started: ${why}`, Level.Warn)
    return false
  }

  #command(): { argv: string[]; preload: string } | string {
    const entry = this.#app.backend!.entry
    const server = PageSource.file(entry, this.#found.resources)
    if (!server) return `no file for ${entry}`
    const preload = join(dirname(server), 'acde-listen.js')
    if (!stat(preload)?.isFile) return `no acde-listen.js beside ${server} (built by an older acdev: acdev build it again)`
    const { spawn, spawnBackend } = sys()
    if (!spawn && !spawnBackend) return 'this runtime cannot start processes'
    const argv = BackendPlace.argv(BackendPlace.env('PATH'), preload, server, process.env['ACDE_RUNTIME'])
    return argv ? { argv, preload } : 'neither bun nor node is on PATH'
  }

  #env(): Record<string, string> {
    const env: Record<string, string> = {}
    for (const key of BackendPlace.ENV) {
      const value = BackendPlace.env(key)
      if (value) env[key] = value
    }
    const { id, backend } = this.#app
    Object.assign(env, sys().proxyEnv(), { ACDE_APP_ID: id, ACDE_APP_DATA: BackendPlace.dataDir(id), PORT: String(backend!.port), NODE_ENV: 'production' })
    if (!BackendPlace.windows()) env['ACDE_APP_SOCKETS'] = BackendPlace.socketDir(id)
    return env
  }

  #spawn(generation: number): void {
    const command = this.#command()
    if (typeof command === 'string') return void this.#refuse(command)
    this.#ended = new Promise((done) => {
      this.#end = done
    })
    try {
      this.#pid = this.#launch(command.argv, command.preload, (code, signal) => this.#exited(generation, code, signal))
    } catch (error) {
      this.#end()
      return void this.#refuse((error as Error).message)
    }
    this.#report(`started (pid ${this.#pid}, port ${this.#app.backend!.port})`)
  }

  #launch(argv: string[], preload: string, onExit: Exit): number {
    const options = { cwd: BackendPlace.dataDir(this.#app.id), env: this.#env(), prefix: `[${this.#name}] ` }
    const { spawn, spawnBackend } = sys()
    if (spawnBackend) return spawnBackend(argv, { ...options, app: this.#app.id, mode: BackendPlace.mode(argv), preload }, onExit)
    return spawn!(argv, options, onExit)
  }

  #exited(generation: number, code: number | null, signal: number | null): void {
    this.#pid = null
    this.#end()
    if (generation !== this.#generation) return
    const why = signal === null ? `exit ${code}` : `signal ${signal}`
    const now = Date.now()
    this.#crashes = [...this.#crashes.filter((time) => now - time < AppBackend.CRASH_WINDOW_MS), now]
    if (this.#crashes.length > AppBackend.RESTARTS) {
      this.#report(`stopped ${this.#crashes.length} times in a minute (${why}); not started again until the app reloads`, Level.Warn)
      return
    }
    this.#report(`stopped (${why}); starting it again`, Level.Warn)
    setTimeout(() => {
      if (generation === this.#generation && this.#pid === null) this.#spawn(generation)
    }, AppBackend.RESTART_MS)
  }
}
