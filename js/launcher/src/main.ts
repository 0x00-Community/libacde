import { installCore } from '@acde/dom/core'
import type { WebInit } from '../../page/src/contract'
import type { FileFilter, Lent, WebGrants, WebThreadHandle } from '../../page/src/sys/natives'
import { bindNatives, sys, type Natives } from '../../page/src/sys/natives'
import { Log } from '../../page/src/log'
import { screenshotFile } from '../../page/src/sys/os'
import { join } from '../../page/src/sys/path'
import { onceSignal, process } from '../../page/src/sys/process'
import { Rpc } from '../../page/src/rpc'
import { SessionLanguage } from '../../page/src/language'
import { PageSource } from '../../page/src/page'
import { desktopInit, webInit, type Place } from './app/app'
import { FakeLock } from './app/lock'
import { AppMenus } from './app/menus'
import { PageCalls } from './app/page'
import { Lending } from './app/lending'
import type { WireMenu } from '../../page/src/contract'
import { LaunchArgs } from './args'
import { refused, route, unsupported, wallpaperOf } from './app/calls'
import { rejectingCalls, sessionOf, windowGrant, type ManagerEvent, type ManagerLink, type ServiceShape, type Session } from './manager/manager'
import { WindowSnapshots, type ShellEvent } from './app/events'
import { ServiceHost } from './manager/service'
import { DevChannel, type DevOpen, type DevOpened, type DevShell, type DevTarget } from './dev/dev'
import { DevToolsEndpoint, type NativeDevTools } from './dev/devtools'
import { managerRelay, type DevRelay } from './dev/relay'
import { Inspect, type InspectHost, type InspectShell, type PlaceChange, type PlayEvent, type ShellWindow } from './dev/inspect'
import { GuestWindows } from './dev/guest'
import { standalone } from './standalone'
import { Frame } from '../../page/src/frame'
import { AppBackend } from './backend/backend'
import { fixedScheme, followAppearance, type AppearanceChange, type AppearancePortal, type Glass, WallToneOf } from './app/appearance'
import { PagePort } from './manager/pageport'
import { LentCallbacks } from './manager/lent'
import type { AnchorWire } from '../../page/src/pinanchor'

interface Grant {
  readonly window: number
  readonly token: bigint
}

interface GrantOptions {
  readonly desktop?: boolean
  readonly window?: number
  readonly mode?: 'fullscreen' | 'free' | 'pinned'
  readonly size?: readonly number[]
  readonly outerSize?: readonly number[]
  readonly rect?: readonly number[]
  readonly host?: boolean
  readonly anchor?: AnchorWire
}

interface LauncherShell extends DevShell, Omit<InspectShell, 'focus' | 'setState' | 'windows' | 'inject' | 'place' | 'play'> {
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
  readonly anchors?: boolean
  reanchor?(window: number, anchor: AnchorWire): void
  focus(window: number): void
  close(window: number): void
  kill?(window: number): void
  setState?(window: number, state: 'normal' | 'minimized' | 'filled' | 'fullscreen'): void
  windows(): ReturnType<InspectShell['windows']>
  inject?(event: Parameters<InspectShell['inject']>[0]): void
  injectSequence?(events: Parameters<InspectShell['play']>[0]): Promise<unknown>
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
    readonly Appearance?: AppearancePortal
  }
}

const HEAP_MB = 256
const STOP = new Set(['close', 'exit', 'quit'])
const CLOSE_WAIT_MS = 3000
type WindowState = 'normal' | 'minimized' | 'filled' | 'fullscreen'
const STATES: readonly string[] = ['normal', 'minimized', 'filled', 'fullscreen'] satisfies WindowState[]

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
  readonly #lending: Lending
  readonly #lent: LentCallbacks
  readonly #devtools: DevToolsEndpoint
  #inspect: Inspect | null = null
  readonly #guest: GuestWindows | null
  readonly #relay: DevRelay | null
  #devState: unknown = null
  readonly #backend: AppBackend | null
  readonly #port: PagePort | null
  #glass: Glass = { level: 0, reducedTransparency: false }
  #reducedMotion = false
  #accent: string | null = null
  readonly #walls = new WallToneOf(() => this.#shell.windows().find((r) => (r.window ?? r.id) === this.#window)?.output ?? null,
    (tone) => this.#thread?.post({ type: 'walltone', tone }))

  constructor(natives: LauncherNatives, readonly session: Session, readonly manager: ManagerLink | null, readonly args: LaunchArgs) {
    this.#shell = Launcher.shell(natives, session.label)
    this.#replays = !natives.native.Windows
    this.#shell.onEvents(() => this.#wake?.())
    manager?.onEvents(() => this.#wake?.())
    this.#services = session.services
    this.#title = session.label
    const menuKeys = manager ? (keys: readonly string[]) => this.#thread?.post({ type: 'menukeys', keys }) : null
    this.#menus = new AppMenus(this.#shell, () => this.#window, menuKeys)
    const report = process.env['ACDE_DEV'] ? (code: string, text: string) => this.#thread?.post({ rpc: 'violation', code, text }) : null
    this.#lent = new LentCallbacks({ send: (frame) => manager?.send?.(frame), report })
    this.#lending = new Lending({ manager, app: () => this.session.found?.app.id, post: (message) => this.#thread?.post(message) })
    this.#pages = new PageCalls({
      label: session.label,
      language: () => SessionLanguage.of(process.env, sys().languages()).language,
      openFiles: (title: string, multiple: boolean, filters: readonly FileFilter[]) => this.#lending.openFiles(title, multiple, filters),
      saveFile: (title: string, name: string, filters: readonly FileFilter[]) => this.#lending.saveFile(title, name, filters),
      folder: (title: string, writable: boolean) => this.#lending.folder(title, writable),
      unlend: (name: string) => this.#lending.revoke(name),
      dropped: (uris: string) => this.#lending.dropped(uris),
      ...(this.#shell.notify && { notify: (title: string, body: string, subtitle: string) => this.#shell.notify!(title, body, subtitle) }),
    })
    const relay = managerRelay(manager)
    this.#relay = relay
    this.#guest = manager ? new GuestWindows(process.pid, GuestWindows.roleOf(session)) : null
    const page = () => {
      const target = this.#target()
      return target && { ...target, url: this.#init?.url ?? null }
    }
    const fixed = fixedScheme(process.env)
    this.#devtools = new DevToolsEndpoint(natives.native.DevTools, relay ?? this.#shell, page, relay?.announce)
    this.#dev = new DevChannel({
      shell: this.#shell,
      devtools: this.#devtools,
      page: () => this.#target(),
      inspect: () => this.#inspector(),
      reload: () => this.#devReload(false),
      open: (given) => this.#devOpen(given),
      origin: () => Date.now() - Log.clock(),
      appearance: (change) => {
        this.#appearance(change)
        return this.#glass
      },
      ...(relay && { respond: relay.respond }),
      ...(this.#guest && { place: (windows: unknown) => this.#guest!.place(windows) }),
      production: !process.env['ACDE_DEV'],
    }, fixed === true)
    void followAppearance(natives.native.Appearance, fixed !== null, (change) => this.#appearance(change))
    this.#backend = session.kind === 'app' && session.found?.app.backend ? new AppBackend(session.found) : null
    const port = manager?.welcome.port
    this.#port = port ? new PagePort(port.data ?? null, manager.connectBackend?.bind(manager), (line) => process.stderr.write(`acdev: ${line}\n`)) : null
  }

  static shell(natives: LauncherNatives, title: string): LauncherShell {
    const { Windows, Shell } = natives.native
    if (Windows) return Windows.connect()
    if (Shell) return Shell.broker({ title })
    throw new Error('the runtime offers no windows: neither native.Windows (acdes) nor native.Shell (acdev)')
  }

  static place(shell: LauncherShell, window: number, change: PlaceChange): readonly [number, number, number, number] {
    const row = shell.windows().find((r) => (r.window ?? r.id) === window)
    const width = change.width ?? row?.width ?? 0
    const height = change.height ?? row?.height ?? 0
    shell.place(window, change.x, change.y, width, height)
    const now = shell.windows().find((r) => (r.window ?? r.id) === window)
    return now ? [now.x, now.y, now.width, now.height] : [change.x, change.y, width, height]
  }

  #inspectShell(): InspectShell {
    const shell = this.#shell
    return {
      focus: (window) => shell.focus(window),
      ...(shell.setState && { setState: (window, state) => shell.setState!(window, state) }),
      place: (window, change) => Promise.resolve(Launcher.place(shell, window, change)),
      windows: () => shell.windows(),
      ...(shell.menus && { menus: (window) => shell.menus!(window) }),
      inject: (event) => {
        if (!shell.inject) throw new Error('input: this runtime injects no input here (in a guest it goes through the manager)')
        shell.inject(event)
      },
      play: (events) => {
        if (!shell.injectSequence) return Promise.reject(new Error('input: this runtime plays no input here (in a guest it goes through the manager)'))
        return shell.injectSequence(events)
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
    const manager = this.manager
    const play = manager && ((events: readonly PlayEvent[]) => manager.call('dev', 'input', [events]))
    const extras = { ...(relay && { inject: relay.inject, window: relay.window }), ...(play && { play }), panels: () => this.#panelRows() }
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
      ...this.#look,
    }
  }

  get #look(): Pick<Place, 'dark' | 'accent' | 'wallTone' | 'glass' | 'reducedMotion'> {
    return { dark: this.#dev.dark, accent: this.#accent, wallTone: this.#walls.tone, glass: this.#glass, reducedMotion: this.#reducedMotion }
  }

  #appearance({ dark, accent, glass, reducedTransparency, reducedMotion, wallTones, primaryOutput }: AppearanceChange): void {
    if (dark !== undefined) this.#dev.follow(dark)
    this.#walls.take({ wallTones, primaryOutput })
    if (accent !== undefined && accent !== this.#accent) {
      this.#accent = accent
      this.#thread?.post({ type: 'accent', color: accent })
    }
    if (reducedMotion !== undefined && reducedMotion !== this.#reducedMotion) {
      this.#reducedMotion = reducedMotion
      this.#thread?.post({ type: 'motion', reduced: reducedMotion })
    }
    const next: Glass = { level: glass ?? this.#glass.level, reducedTransparency: reducedTransparency ?? this.#glass.reducedTransparency }
    if (next.level === this.#glass.level && next.reducedTransparency === this.#glass.reducedTransparency) return
    this.#glass = next
    this.#thread?.post({ type: 'glass', ...next })
  }

  #grant(): Grant {
    if (this.#desktop) return this.#shell.grant({ desktop: true })
    return this.#shell.grant({ window: 1, mode: 'fullscreen', host: true, ...windowGrant(this.manager?.welcome, this.session.found!.app.size) })
  }

  start(): void {
    const grant = this.#grant()
    this.#window = grant.window
    const { session } = this
    const welcome = this.manager?.welcome
    const adopted = this.#lending.adopt(welcome?.lent, welcome?.fds)
    const request = Lending.entered(session.request, welcome?.lent, adopted)
    const init = this.#desktop
      ? desktopInit(session.page!, session.resources, grant.token, this.#place(), session.tier!, session.services)
      : webInit(session.found!, grant.token, this.#place(), { tier: session.tier, request, services: session.services }, grant.window)
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
    if (this.manager && session.lock) return session.confirm ? DevChannel.CONFIRM : DevChannel.LOCK
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
    const anchors = this.#desktop && this.#shell.anchors === true ? { anchors: true } : {}
    const extra = { ...this.#backend?.init(), ...this.#port?.init(), ...anchors, session, ...lock, ...(state ? { devState: state } : {}) }
    const init: WebInit = { ...given, services, ...dev, ...extra }
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
    this.#port?.open((message) => thread.post(message))
    if (this.#desktop) this.#catchUp(thread)
  }

  #fromPage(message: unknown, rpc: Rpc): void {
    if (this.#dev.take(message)) return
    if (this.#port?.take(message)) return
    const chord = (message as { menukey?: unknown } | null)?.menukey
    if (typeof chord === 'string') return void this.#menuKey(chord)
    const call = message as { rpc?: string; id?: number; service?: string; method?: string; args?: unknown[] } | null
    if (!this.manager || call?.rpc !== 'call' || !call.service || !Object.hasOwn(this.#services, call.service)) return void rpc.receive(message)
    const { service, method = '' } = call
    this.#lent.call(service, method, call.args ?? [], (args) => this.manager!.call(service, method, args)).then(
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
      case 'devreload':
        return void this.#devReload(false)
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
    this.#lent.releaseAll()
    this.#reloading = false
    const grant = this.#grant()
    this.#spawn({ ...this.#init!, token: grant.token, ...this.#look })
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
        const anchor = args[1] as AnchorWire | undefined
        if (anchor && !this.#shell.anchors) throw unsupported('panel', 'open with an anchor')
        const grant = this.#shell.grant({ mode: 'pinned', rect: at, ...(anchor && { anchor: { ...anchor, visible: rect(anchor.visible) } }) })
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
      case 'anchor': {
        if (!this.#panels.has(id)) throw refused(`panel ${id} is not this desktop's`)
        const anchor = args[1] as AnchorWire
        if (!this.#shell.reanchor) throw unsupported('panel', method)
        return this.#shell.reanchor(id, { ...anchor, visible: rect(anchor.visible) })
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
    this.#walls.check()
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
      else if (event.type === 'invoke') { const message = this.#lent.invoke(event); if (message) this.#thread?.post(message) }
      else if (event.type === 'request' && !this.#desktop) this.#deliver(event)
      else if (event.type === 'lend' && !this.#desktop) this.#lending.lend([event as unknown as Lent], event['fds'])
      else if (event.type === 'unlend' && !this.#desktop) this.#lending.unlend(String(event['name'] ?? ''))
      else if (event.type === 'visibility' && !this.#desktop) void this.#rpc?.call('window', 'visible', [event['visible'] === true]).catch(() => undefined)
      else if (event.type === 'menuinvoke' && !this.#desktop) void this.#rpc?.call('window', 'menu', [event['menu'], event['item']]).catch(() => undefined)
      else if (event.type === 'softwarechanged' && !this.#desktop) this.#thread?.post(event)
      else if (this.#desktop) this.#toDesktop(this.#snapshots.toDesktop(event as ShellEvent))
    }
  }

  #replaceServices(services: Readonly<Record<string, ServiceShape>> | undefined): void {
    this.#services = services ?? {}
    this.#thread?.post({ type: 'services', services: this.#services })
  }

  async #devOpen({ name, path, url }: DevOpen): Promise<DevOpened> {
    const app = this.#desktop ? null : this.session.found?.app
    if (!app) throw new Error(`open: ${DevChannel.SINGLE}`)
    if (name !== null && name !== app.id && name !== app.label) return { ok: false, text: `open: ${DevChannel.SINGLE}; this one runs ${app.id}` }
    const request = path !== null ? this.#lending.openPath(path) : url !== null ? { kind: 'open-link', url } : null
    if (!request) return { ok: true, text: `opened ${app.id} (it was running)`, app: app.id }
    await this.#rpc?.call('window', 'request', [request])
    return { ok: true, text: `${app.id} was asked to open ${path ?? url}`, app: app.id }
  }

  #deliver(event: ManagerEvent): void {
    const handed = this.#lending.request(event)
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
    this.#port?.close()
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
    const manager = process.env['ACDE_MANAGER_FD'] && natives.native.Manager ? rejectingCalls(natives.native.Manager.connect()) : null
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
