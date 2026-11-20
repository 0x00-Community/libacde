import { installCore } from '@acde/dom/core'
import type { WebInit } from '../../page/src/contract'
import type { WebGrants, WebThreadHandle } from '../../page/src/sys/natives'
import { bindNatives, sys, type Natives } from '../../page/src/sys/natives'
import { readDirOr, readTextOr } from '../../page/src/sys/fs'
import { screenshotFile, xdgHome } from '../../page/src/sys/os'
import { join } from '../../page/src/sys/path'
import { onceSignal, process } from '../../page/src/sys/process'
import { Rpc } from '../../page/src/rpc'
import { SessionLanguage } from '../../page/src/language'
import { PageSource } from '../../page/src/page'
import { AppLookup, DesktopLookup, desktopInit, webInit, type Found, type Place } from './app/app'
import { FakeLock } from './app/lock'
import { AppMenus } from './app/menus'
import { PageCalls } from './app/page'
import type { WireMenu } from '../../page/src/contract'
import type { NativeFileChooser } from '../../page/src/native'
import { API_LEVEL } from '@acde/api/manifest'
import { LaunchArgs } from './args'
import { refused, route, unsupported, wallpaperOf } from './app/calls'
import type { LentRoot } from '@acde/api'
import { sessionOf, type ManagerEvent, type ManagerLink, type ServiceShape, type Session } from './manager/manager'
import { WindowSnapshots, type ShellEvent } from './app/events'
import { ServiceHost } from './manager/service'
import { DevChannel, type DevShell, type DevTarget } from './dev/dev'
import { DevToolsEndpoint, type NativeDevTools } from './dev/devtools'
import { managerRelay, type DevRelay } from './dev/relay'
import { Inspect, type InspectHost, type InspectShell, type ShellWindow } from './dev/inspect'
import { GuestWindows } from './dev/guest'
import { Frame } from '../../page/src/frame'
import { AppBackend } from './backend/backend'

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
  setMenus?(window: number, menus: string): void
  notify?(title: string, body: string, subtitle: string): void
  readonly socketName?: string
  pump(): ShellEvent[]
  onEvents(callback: () => void): void
  grant(options: GrantOptions): Grant
  place(window: number, x: number, y: number, width: number, height: number): void
  hide(window: number, hidden: boolean): void
  release?(window: number): void
  focus(window: number): void
  close(window: number): void
  kill?(window: number): void
  setState?(window: number, state: 'normal' | 'minimized' | 'filled' | 'fullscreen'): void
  windows(): ReturnType<InspectShell['windows']>
  inject?(event: Parameters<InspectShell['inject']>[0]): void
  screenshot(path: string): Promise<{ width: number; height: number }>
  animate?(window: number, options: object): void
  shortcut?(combo: string, scope: string, owner: number): void
  unshortcut?(combo: string, scope: string, owner: number): void
  stats?(reset?: boolean): Record<string, unknown>
  addDesk?(desk: number): unknown
  removeDesk?(desk: number, into: number): unknown
  showDesk?(desk: number): unknown
  moveToDesk?(window: number, desk: number): unknown
  setWallpaper?(color: string, image?: string | null): unknown
}

interface LauncherNatives extends Natives {
  readonly native: {
    readonly Windows?: { connect(): LauncherShell }
    readonly Shell?: { broker(options?: { title?: string }): LauncherShell }
    readonly Manager?: { connect(): ManagerLink }
    readonly DevTools?: NativeDevTools
    readonly FileChooser?: { open(): Promise<NativeFileChooser> }
  }
}

const HEAP_MB = 256
const STOP = new Set(['close', 'exit', 'quit'])
const CLOSE_WAIT_MS = 3000
type WindowState = 'normal' | 'minimized' | 'filled' | 'fullscreen'
const STATES: readonly string[] = ['normal', 'minimized', 'filled', 'fullscreen'] satisfies WindowState[]

function filesFor(found: Found): WebGrants['fs'] {
  const fs = found.app.capabilities.fs
  if (!fs) return null
  const declared = typeof fs === 'object' ? [...fs.roots] : []
  const places = process.env['ACDE_DEV_PLACES'] ? declared : []
  return { places, paths: false }
}

function grantsFor(found: Found): WebGrants {
  const { app } = found
  const root = process.env['ACDE_STORAGE'] ?? join(xdgHome('data'), 'acde', 'storage')
  return {
    app: app.id, fs: filesFor(found), terminal: false, network: !!app.capabilities.network, system: false, bluetooth: false, audio: !!app.capabilities.audio,
    devices: false, power: false, input: false, display: false, office: false, camera: false, microphone: false,
    storage: { key: `app-${app.id}`, dir: root ? join(root, `app-${app.id}`) : null, legacy: null },
  }
}

function standalone(args: LaunchArgs): Session {
  if (args.kind === 'lock') return lockPreview(args)
  if (args.kind !== 'app') throw new Error(`${args.kind} needs the session manager (ACDE_MANAGER_FD); without it only app <id|dir> and lock <desktop> run`)
  const path = (process.env['ACDE_PATH'] ?? '').split(':').filter(Boolean)
  const found = new AppLookup({ readText: (file) => readTextOr(file, null), readDir: readDirOr }).find(args.app, path, sys().cwd())
  const { app } = found
  const grants = grantsFor(found)
  return {
    kind: 'app', found, page: null, resources: found.resources, label: app.label, heapMb: app.heapMb,
    tier: null, grants, request: null, services: {}, own: false, lock: false,
  }
}

function lockPreview(args: LaunchArgs): Session {
  const path = (process.env['ACDE_PATH'] ?? '').split(':').filter(Boolean)
  const found = new DesktopLookup({ readText: (file) => readTextOr(file, null), readDir: readDirOr }).find(args.app, path, sys().cwd())
  const entry = found.desktop.lock
  if (!entry) throw new Error(`desktop ${found.desktop.id} has no lock page (lock in its package.json)`)
  const grants: WebGrants = {
    app: null, fs: null, terminal: false, network: false, system: false, bluetooth: false, audio: false, devices: false, power: false,
    input: false, display: false, office: false, camera: false, microphone: false, storage: { key: 'lock', dir: null, legacy: null },
  }
  return {
    kind: 'desktop', found: null, page: join(found.dir, entry), resources: found.resources, label: `${found.desktop.name} (lock)`,
    heapMb: found.desktop.heapMb, tier: { level: API_LEVEL, shell: [], refused: {} }, grants, request: null, services: {}, own: false, lock: true,
  }
}

function rect(value: unknown): [number, number, number, number] {
  const v = Array.isArray(value) ? value.map((n) => Math.round(Number(n) || 0)) : []
  return [v[0] ?? 0, v[1] ?? 0, v[2] ?? 0, v[3] ?? 0]
}

class Launcher {
  static readonly SIGNALS = ['SIGTERM', 'SIGINT', 'SIGHUP'] as const
  static readonly DEV_SETTLE_MS = 1000
  readonly #shell: LauncherShell
  readonly #snapshots = new WindowSnapshots()
  readonly #held: ShellEvent[] = []
  readonly #replays: boolean
  #wake: (() => void) | null = null
  #ended: number | null = null
  #rpc: Rpc | null = null
  #thread: WebThreadHandle | null = null
  #init: WebInit | null = null
  #window = 0
  #closing = false
  #reloading = false
  readonly #shortcuts = new Map<string, [string, string]>()
  readonly #panels = new Map<number, { rect: [number, number, number, number]; hidden: boolean }>()
  #services: Readonly<Record<string, ServiceShape>> = {}
  readonly #dev: DevChannel
  readonly #lock = new FakeLock()
  #title: string
  readonly #menus: AppMenus
  readonly #pages: PageCalls
  readonly #devtools: DevToolsEndpoint
  #inspect: Inspect | null = null
  readonly #guest: GuestWindows | null
  readonly #relay: DevRelay | null
  #devState: unknown = null
  readonly #backend: AppBackend | null

  constructor(natives: LauncherNatives, readonly session: Session, readonly manager: ManagerLink | null, readonly args: LaunchArgs) {
    this.#shell = Launcher.shell(natives, session.label)
    this.#replays = !natives.native.Windows
    this.#shell.onEvents(() => this.#wake?.())
    manager?.onEvents(() => this.#wake?.())
    this.#services = session.services
    this.#title = session.label
    const menuKeys = manager ? (keys: readonly string[]) => this.#thread?.post({ type: 'menukeys', keys }) : null
    this.#menus = new AppMenus(this.#shell, () => this.#window, menuKeys)
    const chooser = natives.native.FileChooser
    this.#pages = new PageCalls({
      label: session.label,
      language: () => SessionLanguage.of(process.env, sys().languages()).language,
      chooser: () => chooser?.open() ?? null,
      ...(this.#shell.notify && { notify: (title: string, body: string, subtitle: string) => this.#shell.notify!(title, body, subtitle) }),
    })
    const relay = managerRelay(manager)
    this.#relay = relay
    this.#guest = manager ? new GuestWindows(process.pid, session.lock ? 'lock' : session.kind === 'desktop' ? 'desktop' : 'app') : null
    const page = () => {
      const target = this.#target()
      return target && { ...target, url: this.#init?.url ?? null }
    }
    this.#devtools = new DevToolsEndpoint(natives.native.DevTools, relay ?? this.#shell, page, relay?.announce)
    this.#dev = new DevChannel({
      shell: this.#shell,
      devtools: this.#devtools,
      page: () => this.#target(),
      inspect: () => this.#inspector(),
      reload: () => this.#devReload(false),
      ...(relay && { respond: relay.respond }),
      ...(this.#guest && { place: (windows: unknown) => this.#guest!.place(windows) }),
    }, process.env['ACDE_COLOR_SCHEME'] === 'dark')
    this.#backend = session.kind === 'app' && session.found?.app.backend ? new AppBackend(session.found) : null
  }

  static shell(natives: LauncherNatives, title: string): LauncherShell {
    const { Windows, Shell } = natives.native
    if (Windows) return Windows.connect()
    if (Shell) return Shell.broker({ title })
    throw new Error('the runtime offers no windows: neither native.Windows (acdes) nor native.Shell (acdev)')
  }

  #inspectShell(): InspectShell {
    const shell = this.#shell
    return {
      focus: (window) => shell.focus(window),
      ...(shell.setState && { setState: (window, state) => shell.setState!(window, state) }),
      windows: () => shell.windows(),
      ...(shell.menus && { menus: (window) => shell.menus!(window) }),
      inject: (event) => {
        if (!shell.inject) throw new Error('input: this runtime injects no input here (in a guest it goes through the manager)')
        shell.inject(event)
      },
    }
  }

  #inspector(): Inspect {
    if (this.#inspect) return this.#inspect
    const host: InspectHost = {
      shell: this.#inspectShell(),
      web: (window) => (window === this.#window ? this.#rpc : null),
      record: (window) => (window === this.#window ? { kind: 'web', app: this.session.found?.app.id ?? null, title: this.#title } : null),
      panel: (window) => this.#panels.has(window),
      close: (window) => (window === this.#window ? this.close(true) : Promise.resolve(this.#shell.close(window))),
      hide: (window, hidden) => this.#shell.hide(window, hidden),
      open: () => Promise.reject(new Error(`open: ${DevChannel.SINGLE}`)),
      accessibility: () => Promise.resolve(null),
      records: () => [this.#window],
    }
    const relay = this.#relay
    const extras = { ...(relay && { inject: relay.inject, window: relay.window }), panels: () => this.#panelRows() }
    this.#inspect = new Inspect(this.#guest ? this.#guest.host(host, () => this.#window, extras) : host)
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
    const lock = this.session.lock ? { lock: true } : {}
    const window = this.#guest?.global(this.#window) ?? this.#window
    return { name: init.name, app, kind, window, title: this.#title, frame, ...lock, call, post }
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
    if (session.grants?.office) Launcher.#openOffice()
    if (this.#desktop && this.manager && session.grants?.devices) sys().startDevices?.()
    this.#spawn({ ...init, name: this.#webName(init.name) })
    this.#watchRemovable()
  }

  #watchRemovable(): void {
    const fs = this.session.grants?.fs
    if (fs !== true && !(fs && !Array.isArray(fs) && fs.places.includes('removable'))) return
    let known: readonly string[] = []
    const changed = (ids: string[]) => {
      const added = ids.filter((id) => !known.includes(id))
      const removed = known.filter((id) => !ids.includes(id))
      known = ids
      if (added.length || removed.length) this.#thread?.post({ type: 'fsroots', added, removed })
    }
    try {
      known = sys().watchRemovable?.(changed) ?? []
    } catch (error) {
      process.stderr.write(`acdev: removable drives: ${(error as Error).message}\n`)
    }
  }

  #webName(given: string): string {
    const { session } = this
    if (this.manager && session.lock) return DevChannel.LOCK
    if (session.instance !== undefined && session.found) return `${session.found.app.id}#${session.instance}`
    return given
  }

  static #openOffice(): void {
    const failed = sys().openOffice?.()
    if (failed) process.stderr.write(`acdev: office: ${failed}\n`)
  }

  #spawn(given: WebInit): void {
    const app = this.#desktop ? null : (this.session.found?.app.id ?? null)
    const state = this.#devState
    this.#devState = null
    const session = { own: this.session.own }
    const lock = this.session.lock ? { lock: true } : {}
    const services = this.#services
    const dev = this.#dev.prepare(given.name, app)
    const init: WebInit = { ...given, services, ...dev, ...this.#backend?.init(), session, ...lock, ...(state ? { devState: state } : {}) }
    const rpc = new Rpc((message) => thread.post(message))
    rpc.serve((service, method, args) => this.#serve(service, method, args))
    const env = { ...Launcher.env(), ...(this.#shell.socketName && { WAYLAND_DISPLAY: this.#shell.socketName }) }
    const thread = sys().spawnWeb(init.name, init, this.session.heapMb ?? HEAP_MB, env, (message) => this.#fromPage(message, rpc), (code, reason) => {
      rpc.fail(Inspect.PAGE_GONE)
      if (this.#reloading) return this.#respawn()
      if (reason) process.stderr.write(`acdev: ${init.name} stopped: ${reason}\n`)
      this.#end(code)
    }, this.session.grants ?? {} as WebGrants)
    this.#rpc = rpc
    this.#thread = thread
    this.#init = init
    if (this.#desktop) this.#catchUp(thread)
  }

  #fromPage(message: unknown, rpc: Rpc): void {
    if (this.#dev.take(message)) return
    const chord = (message as { menukey?: unknown } | null)?.menukey
    if (typeof chord === 'string') return void this.#menuKey(chord)
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
      case 'title':
        this.#title = String(args[0] ?? '') || this.session.label
        return undefined
      case 'menus':
        return this.#menus.set((Array.isArray(args[0]) ? args[0] : []) as WireMenu[])
      case 'page':
        return this.#pages.call(method, args)
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
        if (service === 'window' && method === 'setMenus') this.#menus.remember((Array.isArray(args[0]) ? args[0] : []) as WireMenu[])
        return this.manager!.call(service, method, args)
      case 'session':
        if (service === 'desktops' && method === 'list') return { desktops: [], current: '' }
        throw unsupported(service, method)
      case 'lock':
        if (!this.session.lock) throw unsupported(service, method)
        return this.#lock.answer(method, args)
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

  #releaseShortcuts(): void {
    for (const [combo, scope] of this.#shortcuts.values()) {
      try {
        this.#shell.unshortcut?.(combo, scope, 0)
      } catch (error) {
        process.stderr.write(`acdev: releasing the shortcut ${combo} (${scope}): ${(error as Error).message}\n`)
      }
    }
    this.#shortcuts.clear()
  }

  #respawn(): void {
    this.#releaseShortcuts()
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

  #panelRows(): ShellWindow[] {
    return [...this.#panels].map(([id, { rect: [x, y, width, height], hidden }]) => ({
      id, window: id, canvas: 0, mode: 'pinned', x, y, width, height, z: 0, focus: false, hidden,
    }))
  }

  #panel(method: string, args: unknown[]): unknown {
    const id = Number(args[0])
    switch (method) {
      case 'open': {
        const at = rect(args[0])
        const grant = this.#shell.grant({ mode: 'pinned', rect: at })
        this.#panels.set(grant.window, { rect: at, hidden: false })
        return grant
      }
      case 'place': {
        if (!this.#panels.has(id)) throw refused(`panel ${id} is not this desktop's`)
        const at = rect(args[1])
        const [x, y, width, height] = at
        const hidden = width <= 0 || height <= 0
        this.#panels.set(id, { rect: hidden ? this.#panels.get(id)!.rect : at, hidden })
        if (hidden) return this.#shell.hide(id, true)
        this.#shell.place(id, x, y, width, height)
        return this.#shell.hide(id, false)
      }
      case 'close':
        if (this.#panels.delete(id)) this.#shell.release?.(id)
        return undefined
    }
    throw unsupported('panel', method)
  }

  #windowsCall(method: string, args: unknown[]): unknown {
    const id = Number(args[0])
    const shell = this.#shell
    switch (method) {
      case 'close':
        shell.close(id)
        return true
      case 'kill':
        if (shell.kill) shell.kill(id)
        else shell.close(id)
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
        if (typeof args[0] === 'string') return shell.screenshot(args[0])
        if (this.manager) return this.manager.call('windows', 'screenshot', args)
        return this.#screenshot(args[0])
      case 'animate':
        return shell.animate?.(id, (args[1] ?? {}) as object)
      case 'stats':
        return shell.stats?.() ?? {}
      case 'shortcut':
      case 'unshortcut': {
        const call = method === 'shortcut' ? shell.shortcut : shell.unshortcut
        if (!call) throw unsupported('windows', method)
        const combo = String(args[0])
        const scope = String(args[1] ?? 'desktop')
        const done = call.call(shell, combo, scope, 0)
        if (method === 'shortcut') this.#shortcuts.set(`${scope} ${combo}`, [combo, scope])
        else this.#shortcuts.delete(`${scope} ${combo}`)
        return done
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
      case 'setWallpaper':
        return this.#setWallpaper(args[0])
      case 'launch':
        if (this.manager) return this.manager.call('launch', 'start', args)
        break
      case 'invokeMenu':
        if (this.manager) return this.manager.call('windows', 'invokeMenu', args)
    }
    throw unsupported('windows', method)
  }

  async #screenshot(given: unknown): Promise<{ path: string; width: number; height: number }> {
    const { screen, window, path } = (given ?? {}) as { screen?: unknown; window?: unknown; path?: unknown }
    if (window !== undefined && window !== null) throw unsupported('windows', 'screenshot of one window')
    if (screen !== undefined && screen !== null && screen !== 0) throw unsupported('windows', `screenshot of screen ${String(screen)}`)
    if (path !== undefined && path !== null && (typeof path !== 'string' || !path.startsWith('/'))) throw refused('windows.screenshot: path is an absolute path')
    const file = typeof path === 'string' ? path : screenshotFile()
    const { width, height } = await this.#shell.screenshot(file)
    return { path: file, width, height }
  }

  #setWallpaper(given: unknown): unknown {
    if (!this.#desktop || !this.#shell.setWallpaper) throw unsupported('windows', 'setWallpaper')
    const { color, image } = wallpaperOf(given)
    return image === undefined ? this.#shell.setWallpaper(color) : this.#shell.setWallpaper(color, image)
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
      else if (!this.#desktop && (event.type === 'menuselect' || event.type === 'shortcut')) this.#appKey(event)
      else if (event.type === 'devreload') void this.#devReload(event['title'] === 'hot')
      else if (this.#desktop) this.#toDesktop(this.#snapshots.toDesktop(event))
    }
  }

  #toDesktop(event: ShellEvent): void {
    if (this.#thread && !this.#reloading) return void this.#thread.post(event)
    const folded = WindowSnapshots.FOLDED.has(event.type)
    if (folded && (this.#replays || this.#reloading)) return
    this.#held.push(event)
  }

  #catchUp(thread: WebThreadHandle): void {
    const replayed = this.#replays ? this.#snapshots.replay() : []
    if (!this.#replays) this.#snapshots.clear()
    for (const event of [...replayed, ...this.#held.splice(0)]) thread.post(event)
  }

  #menuKey(chord: string): void {
    const hit = this.#menus.pressed(chord)
    if (hit) void this.#rpc?.call('window', 'menu', [hit.menu, hit.item]).catch(() => undefined)
  }

  #appKey(event: ShellEvent): void {
    const title = event['title']
    if (event.type === 'menuselect') return void this.#rpc?.call('window', 'menu', [event['key'], title]).catch(() => undefined)
    if (title === 'menu') return void this.#menuKey(String(event['key'] ?? ''))
    if (title === 'app') this.#thread?.post(event)
  }

  #managerEvents(): void {
    for (const event of this.manager?.pump() ?? []) {
      if (event.type === 'managerclosed') this.#end(0)
      else if (event.type === 'services') this.#replaceServices(event['services'] as Readonly<Record<string, ServiceShape>> | undefined)
      else if (event.type === 'devrequest') void this.#dev.request(String(event['key'] ?? ''))
      else if (event.type === 'devreload') void this.#devReload(event['hot'] === true)
      else if (event.type === 'invoke') this.#thread?.post({ rpc: 'invoke', fn: event['fn'], args: event['args'] ?? [] })
      else if (event.type === 'request' && !this.#desktop) this.#deliver(event)
      else if (event.type === 'unlend' && !this.#desktop) this.#unlend(String(event['name'] ?? ''))
      else if (event.type === 'menuinvoke' && !this.#desktop) void this.#rpc?.call('window', 'menu', [event['menu'], event['item']]).catch(() => undefined)
      else if (this.#desktop) this.#toDesktop(this.#snapshots.toDesktop(event as ShellEvent))
    }
  }

  #replaceServices(services: Readonly<Record<string, ServiceShape>> | undefined): void {
    this.#services = services ?? {}
    this.#thread?.post({ type: 'services', services: this.#services })
  }

  #unlend(name: string): void {
    const app = this.session.found?.app.id
    if (!app || !name || !sys().unlendRoot?.(app, name)) return
    this.#thread?.post({ type: 'fsroots', removed: [name] })
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
    for (const signal of Launcher.SIGNALS) onceSignal(signal, () => this.#end(0))
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
    await this.#dev.settle(Launcher.DEV_SETTLE_MS)
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
    if (manager && args.kind === 'lock') throw new Error('lock is the lock page preview of acdev dev; in a session the manager opens the lock page')
    const session = manager ? sessionOf(manager.welcome, args.kind as 'app' | 'desktop') : standalone(args)
    const launcher = new Launcher(natives, session, manager, args)
    launcher.start()
    process.exit(await launcher.run())
  } catch (error) {
    process.stderr.write(`acdev: ${(error as Error).message}\n`)
    process.exit(2)
  }
}
