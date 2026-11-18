import { installCore } from '@dopame/acde-dom/core'
import type { WebInit } from '../../page/src/contract'
import type { WebGrants, WebThreadHandle } from '../../page/src/sys/natives'
import { bindNatives, sys, type Natives } from '../../page/src/sys/natives'
import { readDirOr, readTextOr } from '../../page/src/sys/fs'
import { xdgHome } from '../../page/src/sys/os'
import { join } from '../../page/src/sys/path'
import { process } from '../../page/src/sys/process'
import { Rpc } from '../../page/src/rpc'
import { SessionLanguage } from '../../page/src/language'
import { PageSource } from '../../page/src/page'
import { AppLookup, desktopInit, webInit, type Found, type Place } from './app'
import { LaunchArgs } from './args'
import { refused, route, unsupported } from './calls'
import type { LentRoot } from '@dopame/acde-api'
import { sessionOf, type ManagerEvent, type ManagerLink, type ServiceShape, type Session } from './manager'
import { toDesktop, type ShellEvent } from './events'
import { ServiceHost } from './service'
import { DevChannel, type DevShell, type DevTarget } from './dev'
import { DevToolsEndpoint, type NativeDevTools } from './devtools'
import { Inspect, type InspectShell } from './inspect'
import { Frame } from '../../page/src/frame'
import { AppBackend } from './backend'

interface Grant {
  readonly window: number
  readonly token: bigint
}

interface GrantOptions {
  readonly desktop?: boolean
  readonly window?: number
  readonly mode?: 'fullscreen' | 'free' | 'pinned'
  readonly size?: readonly number[]
  readonly rect?: readonly number[]
  readonly host?: boolean
}

interface LauncherShell extends DevShell, Omit<InspectShell, 'focus' | 'setState' | 'windows' | 'inject'> {
  devEvent?(json: string): number
  readonly socketName?: string
  pump(): ShellEvent[]
  onEvents(callback: () => void): void
  grant(options: GrantOptions): Grant
  place(window: number, x: number, y: number, width: number, height: number): void
  hide(window: number, hidden: boolean): void
  release(window: number): void
  focus(window: number): void
  close(window: number): void
  setState?(window: number, state: 'normal' | 'minimized' | 'filled' | 'fullscreen'): void
  windows(): ReturnType<InspectShell['windows']>
  inject(event: Parameters<InspectShell['inject']>[0]): void
  screenshot(path: string): Promise<{ width: number; height: number }>
  animate?(window: number, options: object): void
  shortcut?(combo: string, scope: string, owner: number): void
  unshortcut?(combo: string, scope: string, owner: number): void
  stats?(reset?: boolean): Record<string, unknown>
  addDesk?(desk: number): unknown
  removeDesk?(desk: number, into: number): unknown
  showDesk?(desk: number): unknown
  moveToDesk?(window: number, desk: number): unknown
}

interface LauncherNatives extends Natives {
  readonly native: {
    readonly Shell: { broker(options?: { title?: string }): LauncherShell }
    readonly Manager?: { connect(): ManagerLink }
    readonly DevTools?: NativeDevTools
  }
}

const HEAP_MB = 256
const STOP = new Set(['close', 'exit', 'quit'])
const CLOSE_WAIT_MS = 3000
type WindowState = 'normal' | 'minimized' | 'filled' | 'fullscreen'
const STATES: readonly string[] = ['normal', 'minimized', 'filled', 'fullscreen'] satisfies WindowState[]

function grantsFor(found: Found): WebGrants {
  const { app } = found
  const root = process.env['ACDE_STORAGE'] ?? join(xdgHome('data'), 'acde', 'storage')
  return {
    app: app.id, fs: null, terminal: false, network: !!app.capabilities.network, system: false, bluetooth: false, audio: !!app.capabilities.audio,
    devices: false, power: false, input: false, display: false, office: false, camera: false, microphone: false,
    storage: { key: `app-${app.id}`, dir: root ? join(root, `app-${app.id}`) : null, legacy: null },
  }
}

function standalone(args: LaunchArgs): Session {
  if (args.kind !== 'app') throw new Error(`${args.kind} needs the session manager (ACDE_MANAGER_FD); without it only app <id|dir> runs`)
  const path = (process.env['ACDE_PATH'] ?? '').split(':').filter(Boolean)
  const found = new AppLookup({ readText: (file) => readTextOr(file, null), readDir: readDirOr }).find(args.app, path, sys().cwd())
  const { app } = found
  const grants = grantsFor(found)
  return { kind: 'app', found, page: null, resources: found.resources, label: app.label, heapMb: app.heapMb, tier: null, grants, request: null, services: {} }
}

function rect(value: unknown): [number, number, number, number] {
  const v = Array.isArray(value) ? value.map((n) => Math.round(Number(n) || 0)) : []
  return [v[0] ?? 0, v[1] ?? 0, v[2] ?? 0, v[3] ?? 0]
}

class Launcher {
  readonly #shell: LauncherShell
  #wake: (() => void) | null = null
  #ended: number | null = null
  #rpc: Rpc | null = null
  #thread: WebThreadHandle | null = null
  #init: WebInit | null = null
  #window = 0
  #closing = false
  #reloading = false
  readonly #panels = new Set<number>()
  #services: Readonly<Record<string, ServiceShape>> = {}
  readonly #dev: DevChannel
  readonly #devtools: DevToolsEndpoint
  #inspect: Inspect | null = null
  #devState: unknown = null
  readonly #backend: AppBackend | null

  constructor(natives: LauncherNatives, readonly session: Session, readonly manager: ManagerLink | null, readonly args: LaunchArgs) {
    this.#shell = natives.native.Shell.broker({ title: session.label })
    this.#shell.onEvents(() => this.#wake?.())
    manager?.onEvents(() => this.#wake?.())
    this.#services = session.services
    this.#devtools = new DevToolsEndpoint(natives.native.DevTools, this.#shell, () => {
      const target = this.#target()
      return target && { ...target, url: this.#init?.url ?? null }
    })
    this.#dev = new DevChannel({
      shell: this.#shell,
      devtools: this.#devtools,
      page: () => this.#target(),
      inspect: () => this.#inspector(),
      reload: () => this.#devReload(false),
    }, process.env['ACDE_COLOR_SCHEME'] === 'dark')
    this.#backend = session.kind === 'app' && session.found?.app.backend ? new AppBackend(session.found) : null
  }

  #inspector(): Inspect {
    this.#inspect ??= new Inspect({
      shell: this.#shell,
      web: (window) => (window === this.#window ? this.#rpc : null),
      record: (window) => (window === this.#window ? { kind: 'web', app: this.session.found?.app.id ?? null, title: this.session.label } : null),
      panel: (window) => this.#panels.has(window),
      close: (window) => (window === this.#window ? this.close(true) : Promise.resolve(this.#shell.close(window))),
      hide: (window, hidden) => this.#shell.hide(window, hidden),
      open: () => Promise.reject(new Error(`open: ${DevChannel.SINGLE}`)),
      accessibility: () => Promise.resolve(null),
      records: () => [this.#window],
    })
    return this.#inspect
  }

  #target(): DevTarget | null {
    const init = this.#init
    const rpc = this.#rpc
    if (!init || !rpc || this.#closing) return null
    const desktop = this.#desktop
    const kind = desktop ? 'desktop' as const : 'app' as const
    const app = desktop ? null : (this.session.found?.app.id ?? null)
    const call = (service: string, method: string, args: readonly unknown[]) => rpc.call(service, method, args)
    const post = (message: Record<string, unknown>) => this.#thread?.post(message)
    const frame = desktop ? 0 : Frame.height(this.session.found?.app)
    return { name: init.name, app, kind, window: this.#window, title: this.session.label, frame, call, post }
  }

  get #desktop(): boolean {
    return this.session.kind === 'desktop'
  }

  #place(): Place {
    return {
      width: this.args.width,
      height: this.args.height,
      scale: this.args.scale,
      languages: SessionLanguage.of(process.env, sys().languages()),
      navigator: { userAgent: '', platform: process.platform === 'darwin' ? 'MacIntel' : process.platform === 'win32' ? 'Win32' : 'Linux x86_64' },
    }
  }

  #grant(): Grant {
    if (this.#desktop) return this.#shell.grant({ desktop: true })
    const size = this.session.found!.app.size
    return this.#shell.grant({ window: 1, mode: 'fullscreen', host: true, ...(size && { size }) })
  }

  start(): void {
    const grant = this.#grant()
    this.#window = grant.window
    const { session } = this
    const init = this.#desktop
      ? desktopInit(session.page!, session.resources, grant.token, this.#place(), session.tier!, session.services)
      : webInit(session.found!, grant.token, this.#place(), { tier: session.tier, request: session.request, services: session.services }, grant.window)
    this.#backend?.start()
    this.#spawn(init)
  }

  #spawn(given: WebInit): void {
    const app = this.#desktop ? null : (this.session.found?.app.id ?? null)
    const state = this.#devState
    this.#devState = null
    const init: WebInit = { ...given, ...this.#dev.prepare(given.name, app), ...this.#backend?.init(), ...(state ? { devState: state } : {}) }
    const rpc = new Rpc((message) => thread.post(message))
    rpc.serve((service, method, args) => this.#serve(service, method, args))
    const env = { ...Launcher.env(), ...(this.#shell.socketName && { WAYLAND_DISPLAY: this.#shell.socketName }) }
    const thread = sys().spawnWeb(init.name, init, this.session.heapMb ?? HEAP_MB, env, (message) => this.#fromPage(message, rpc), (code, reason) => {
      if (this.#reloading) return this.#respawn()
      if (reason) process.stderr.write(`acdev: ${init.name} stopped: ${reason}\n`)
      this.#end(code)
    }, this.session.grants ?? {} as WebGrants)
    this.#rpc = rpc
    this.#thread = thread
    this.#init = init
  }

  #fromPage(message: unknown, rpc: Rpc): void {
    if (this.#dev.take(message)) return
    const call = message as { rpc?: string; id?: number; service?: string; method?: string; args?: unknown[] } | null
    if (!this.manager || call?.rpc !== 'call' || !call.service || !Object.hasOwn(this.#services, call.service)) return void rpc.receive(message)
    this.manager.call(call.service, call.method ?? '', call.args ?? []).then(
      (value) => this.#thread?.post({ rpc: 'result', id: call.id, value }),
      (error: Error & { status?: number; code?: string }) => {
        const fields = { ...(error.status !== undefined && { status: error.status }), ...(error.code !== undefined && { code: error.code }) }
        this.#thread?.post({ rpc: 'result', id: call.id, error: { name: error.name ?? 'Error', message: error.message, fields } })
      },
    )
  }

  #serve(service: string, method: string, args: unknown[]): unknown {
    const caller = { window: this.#window, desktop: this.#desktop, managed: this.manager !== null }
    switch (route(service, method, args, caller)) {
      case 'acknowledge':
        return undefined
      case 'close':
        void this.close(true)
        return true
      case 'minimize':
        if (method === 'requestState' && args[0] !== 'minimized') throw unsupported(service, `${method}(${String(args[0])})`)
        return this.#rpc!.call('window', 'minimize')
      case 'reload':
        return this.#reload(args[0])
      case 'capabilities':
        return this.#capabilities(method, args)
      case 'panel':
        return this.#panel(method, args)
      case 'windows':
        return this.#windowsCall(method, args)
      case 'manager':
        return this.manager!.call(service, method, args)
      case 'unsupported':
        throw unsupported(service, method)
    }
  }

  #reload(url: unknown): boolean {
    const given = url === undefined || url === null ? null : String(url)
    if (this.#closing || this.#reloading || (given !== null && !given.startsWith(`${PageSource.ORIGIN}/`))) return false
    if (given !== null) this.#init = { ...this.#init!, url: given }
    this.#reloading = true
    this.#devtools.reloading(this.#init!.name)
    void this.#backend?.restart()
    this.#thread?.terminate()
    return true
  }

  async #devReload(hot: boolean): Promise<void> {
    if (hot) return void this.#thread?.post({ type: 'devhot' })
    if (this.#closing || this.#reloading) return
    this.#devState = await this.#dev.snapshot()
    const fresh = !this.manager && this.session.found ? standalone(this.args).found : null
    if (fresh) this.#init = webInit(fresh, 0n, this.#place(), { tier: null, request: null, services: {} }, this.#window)
    this.#reloading = true
    this.#devtools.reloading(this.#init!.name)
    void this.#backend?.restart(fresh ?? undefined)
    this.#thread?.terminate()
  }

  #respawn(): void {
    this.#reloading = false
    const grant = this.#grant()
    this.#spawn({ ...this.#init!, token: grant.token })
  }

  #capabilities(method: string, args: unknown[]): unknown {
    const declared = Object.keys(this.session.found?.app.capabilities ?? {})
    const status = (name: string) => {
      const state = declared.includes(name) ? 'available' : 'not-declared'
      return { name, kind: 'service', state, reason: state === 'available' ? '' : `not in the capabilities of ${this.session.label}`, optional: false }
    }
    return method === 'list' ? declared.map(status) : status(String(args[0]))
  }

  #panel(method: string, args: unknown[]): unknown {
    const id = Number(args[0])
    switch (method) {
      case 'open': {
        const grant = this.#shell.grant({ mode: 'pinned', rect: rect(args[0]) })
        this.#panels.add(grant.window)
        return grant
      }
      case 'place': {
        if (!this.#panels.has(id)) throw refused(`panel ${id} is not this desktop's`)
        const [x, y, width, height] = rect(args[1])
        if (width <= 0 || height <= 0) return this.#shell.hide(id, true)
        this.#shell.place(id, x, y, width, height)
        return this.#shell.hide(id, false)
      }
      case 'close':
        if (this.#panels.delete(id)) this.#shell.release(id)
        return undefined
    }
    throw unsupported('panel', method)
  }

  #windowsCall(method: string, args: unknown[]): unknown {
    const id = Number(args[0])
    const shell = this.#shell
    switch (method) {
      case 'close':
      case 'kill':
        shell.close(id)
        return true
      case 'focus':
        return shell.focus(id)
      case 'hide':
      case 'show':
        return shell.hide(id, method === 'hide')
      case 'requestState':
        if (!STATES.includes(String(args[1]))) throw refused(`no window state ${String(args[1])} (${STATES.join(', ')})`)
        if (!shell.setState) throw unsupported('windows', method)
        return shell.setState(id, String(args[1]) as WindowState)
      case 'screenshot':
        return shell.screenshot(String(args[0]))
      case 'animate':
        return shell.animate?.(id, (args[1] ?? {}) as object)
      case 'stats':
        return shell.stats?.() ?? {}
      case 'shortcut':
      case 'unshortcut': {
        const call = method === 'shortcut' ? shell.shortcut : shell.unshortcut
        if (!call) throw unsupported('windows', method)
        return call.call(shell, String(args[0]), String(args[1] ?? 'desktop'), 0)
      }
      case 'place': {
        const [x, y, width, height] = rect(args[1])
        return shell.place(id, x, y, width, height)
      }
      case 'addDesk':
      case 'showDesk':
      case 'removeDesk':
      case 'moveToDesk': {
        const call = shell[method]
        if (!call) throw unsupported('windows', method)
        return (call as (...numbers: number[]) => unknown).call(shell, ...args.map(Number))
      }
      case 'launch':
        if (this.manager) return this.manager.call('launch', 'start', args)
    }
    throw unsupported('windows', method)
  }

  async close(force: boolean): Promise<boolean> {
    if (this.#closing) return true
    if (!force && this.#rpc) {
      const asked = this.#rpc.call('window', 'beforeclose').catch(() => true)
      const waited = new Promise((later) => setTimeout(() => later(true), CLOSE_WAIT_MS))
      if ((await Promise.race([asked, waited])) === false) return false
    }
    this.#closing = true
    this.#thread?.terminate()
    this.#end(0)
    return true
  }

  static env(): Record<string, string> {
    const keep = /^(ACDE_|XDG_|LANG$|LC_|LANGUAGE$|HOME$|USER$|DBUS_SESSION_BUS_ADDRESS$|RUST_LOG$|TZ$|PATH$|RUST_BACKTRACE$)/
    return Object.fromEntries(
      Object.entries(process.env).filter((entry): entry is [string, string] => keep.test(entry[0]) && entry[1] !== undefined && entry[0] !== 'ACDE_MANAGER_FD'),
    )
  }

  #end(code: number): void {
    this.#ended ??= code
    this.#wake?.()
  }

  #shellEvents(): void {
    for (const event of this.#shell.pump()) {
      if (STOP.has(event.type)) this.#end(0)
      else if (event.type === 'windowcloserequest') void this.close(false)
      else if (event.type === 'devrequest') void this.#dev.request(String(event['key'] ?? ''))
      else if (event.type === 'devtools') this.#devtools.pressed()
      else if (event.type === 'devreload') void this.#devReload(event['title'] === 'hot')
      else if (this.#desktop) this.#thread?.post(toDesktop(event))
    }
  }

  #managerEvents(): void {
    for (const event of this.manager?.pump() ?? []) {
      if (event.type === 'managerclosed') this.#end(0)
      else if (event.type === 'services') this.#replaceServices(event['services'] as Readonly<Record<string, ServiceShape>> | undefined)
      else if (event.type === 'invoke') this.#thread?.post({ rpc: 'invoke', fn: event['fn'], args: event['args'] ?? [] })
      else if (event.type === 'request' && !this.#desktop) this.#deliver(event)
      else if (this.#desktop) this.#thread?.post(event)
    }
  }

  #replaceServices(services: Readonly<Record<string, ServiceShape>> | undefined): void {
    this.#services = services ?? {}
    this.#thread?.post({ type: 'services', services: this.#services })
  }

  #deliver(event: ManagerEvent): void {
    const request = event['request'] as Record<string, unknown> | undefined
    const lend = event['lend'] as LentRoot | undefined
    let root: string | undefined
    if (lend) {
      try {
        root = sys().lendRoot?.(this.session.found!.app.id, lend)
      } catch (error) {
        process.stderr.write(`acdev: the lent folder ${lend.dir}: ${(error as Error).message}\n`)
      }
    }
    const handed = root === undefined ? request : { ...request, root }
    void this.#rpc?.call('window', 'request', [handed]).catch(() => undefined)
  }

  async run(): Promise<number> {
    if (this.args.shot) this.#shoot(this.args.shot).catch((error: Error) => process.stderr.write(`acdev: screenshot: ${error.message}\n`))
    if (this.args.seconds !== null) setTimeout(() => this.#end(0), this.args.seconds * 1000)
    while (this.#ended === null) {
      this.#shellEvents()
      this.#managerEvents()
      if (this.#ended !== null) break
      await new Promise<void>((wake) => {
        this.#wake = wake
      })
    }
    await this.#backend?.close()
    return this.#ended
  }

  async #shoot(file: string): Promise<void> {
    await new Promise((later) => setTimeout(later, this.args.shotAfterMs))
    const { width, height } = await this.#shell.screenshot(file)
    process.stderr.write(`acdev: screenshot ${width}x${height} -> ${file}\n`)
  }
}

export default async function launch(natives: LauncherNatives): Promise<void> {
  installCore(globalThis, natives.host, { nodeTimers: true })
  bindNatives(natives)
  try {
    const args = LaunchArgs.parse(process.argv)
    const manager = process.env['ACDE_MANAGER_FD'] && natives.native.Manager ? natives.native.Manager.connect() : null
    if (args.kind === 'service') {
      if (!manager) throw new Error('service needs the session manager (ACDE_MANAGER_FD)')
      if (manager.welcome.kind !== 'service') throw new Error(`the session manager started this process as ${manager.welcome.kind}, not service`)
      const host = new ServiceHost(manager, manager.welcome, natives.native as unknown as Record<string, unknown>)
      await host.start()
      process.exit(await host.run())
    }
    const session = manager ? sessionOf(manager.welcome, args.kind) : standalone(args)
    const launcher = new Launcher(natives, session, manager, args)
    launcher.start()
    process.exit(await launcher.run())
  } catch (error) {
    process.stderr.write(`acdev: ${(error as Error).message}\n`)
    process.exit(2)
  }
}
