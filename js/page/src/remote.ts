import type { AppMenu, NativeWindow, WindowApp, WindowEdge, WindowEventName, WindowMode, WindowOutput, WindowRect, WindowState } from '@dopame/acde-api'
import type { ServiceShape, WebInit, WireMenu } from './contract'
import { Level, Log } from './log'
import { WINDOWS_EVENT_NAMES, type NativeSurface, type WindowEvent } from './native'
import { Notices } from './notices'
import { Rpc } from './rpc'
import { CapabilityCheck } from './broker/access/capabilities'
import { Permits } from './broker/access/approval/permits'
import type { Approval } from './broker/access/approval/permissions'
import type { HandOver } from './pagecalls'
import type { CapabilityStatus } from '@dopame/acde-api/manifest'

export class WindowList {
  static readonly EVENTS = WINDOWS_EVENT_NAMES
  static readonly STATES: readonly WindowState[] = ['normal', 'minimized', 'filled', 'fullscreen']
  readonly notices = new Notices()
  readonly #windows = new Map<number, NativeWindow>()
  readonly #rpc: Rpc

  constructor(rpc: Rpc, rows: readonly NativeWindow[] = []) {
    this.#rpc = rpc
    for (const row of rows) this.#windows.set(row.id, { ...row })
  }

  deliver(event: WindowEvent): void {
    this.#track(event)
    this.notices.emit(`windows:${event.type.slice('window'.length)}`, event)
  }

  #track(event: WindowEvent): void {
    if (event.type === 'windowclose') return void this.#windows.delete(event.window)
    const was = this.#windows.get(event.window)
    if (event.type === 'windowstate') {
      const state = WindowList.STATES.find((known) => known === event['state'])
      if (was && state) this.#windows.set(event.window, { ...was, state })
      return
    }
    if (event.type !== 'windowopen' && event.type !== 'windowtitle') return
    const text = (value: unknown, fallback: string | null) => (typeof value === 'string' && value ? value : fallback)
    const web = event['web'] === true || was?.web === true
    const appId = text(event['appId'], was?.appId ?? null)
    const title = text(event['title'], was?.title ?? null)
    this.#windows.set(event.window, { id: event.window, appId, title, ...(web ? { web } : {}), state: was?.state ?? 'normal' })
  }

  screenshot(path: string): Promise<{ width: number; height: number }> {
    return this.#call('screenshot', path) as Promise<{ width: number; height: number }>
  }

  #call(method: 'launch' | 'close' | 'screenshot', ...args: unknown[]): Promise<unknown> {
    const call = this.#rpc.call('windows', method, args)
    call.catch((error: Error) => Log.write(Level.Warn, 'acde_host', `windows.${method}: ${error.message}`))
    return call
  }

  readonly api = {
    on: this.notices.for('windows', WindowList.EVENTS),
    list: (): NativeWindow[] => [...this.#windows.values()].map((window) => ({ ...window })),
    launch: (argv: string[], options?: { cwd?: string; env?: Record<string, string>; slot?: boolean }): Promise<number> =>
      this.#call('launch', argv, options ?? {}) as Promise<number>,
    close: (id: number): void => void this.#call('close', id),
  }
}

type PageLike = { dispatchEvent(event: Event): boolean; Event: typeof Event; document: Document }

export class WindowSelf {
  static readonly #EDGES: Record<WindowEdge, number> = { top: 1, bottom: 2, left: 4, right: 8 }
  static readonly #OWN: Record<WindowEventName, true> = {
    activate: true,
    deactivate: true,
    beforeclose: true,
    request: true,
    suspend: true,
    resume: true,
    grab: true,
    grablost: true,
    state: true,
    output: true,
    hostcontrols: true,
    minimizable: true,
  }
  static readonly EVENTS: readonly string[] = [...Object.keys(WindowSelf.#OWN), 'shortcut']
  readonly #rpc: Rpc
  readonly #surface: NativeSurface
  readonly #page: PageLike
  #menus: AppMenu[] = []
  readonly #handlers = new Map<WindowEventName, Set<(value?: unknown) => unknown>>()
  #unmount: (() => void) | null = null
  #opening: Record<string, unknown> | null
  #state: WindowState = 'normal'
  #output: WindowOutput
  onVisible: (visible: boolean) => void = () => {}
  onTitle: (title: string) => void = () => {}
  handOver: HandOver | null = null

  constructor(rpc: Rpc, surface: NativeSurface, page: PageLike, opening: Record<string, unknown> | null, output: WindowOutput) {
    this.#rpc = rpc
    this.#surface = surface
    this.#page = page
    this.#opening = opening
    this.#output = output
  }

  get state(): WindowState {
    return this.#state
  }

  get output(): WindowOutput {
    return this.#output
  }

  get minimizable(): boolean {
    return this.#surface.minimizable !== false
  }

  get hostControls(): boolean {
    return this.#surface.hostControls === true
  }

  changed(state: string, previous: string): void {
    this.#state = state as WindowState
    this.emit('state', { state, previous })
  }

  moved(output: WindowOutput): void {
    const was = this.#output
    this.#output = output
    if (JSON.stringify(was) !== JSON.stringify(output)) this.emit('output', output)
  }

  static output(width: number, height: number, scale: number, workArea?: WindowRect | null): WindowOutput {
    return Object.freeze({ width, height, scale, workArea: Object.freeze({ ...(workArea ?? { x: 0, y: 0, width, height }) }) })
  }

  readonly host = {
    setTitle: (title: string): void => {
      this.onTitle(String(title))
      void this.#rpc.call('window', 'setTitle', [String(title)]).catch(() => undefined)
    },
    setMenus: (menus: readonly AppMenu[]): void => {
      this.#menus = Array.isArray(menus) ? [...menus] : []
      const wire: WireMenu[] = this.#menus.map((m) => ({
        id: m.id,
        label: m.label,
        items: m.items.map((it) =>
          'type' in it
            ? { type: 'separator' as const }
            : { id: it.id, label: it.label, shortcut: it.shortcut, checked: it.checked, disabled: it.disabled, action: !!it.onSelect },
        ),
      }))
      void this.#rpc.call('window', 'setMenus', [wire]).catch(() => undefined)
    },
  }

  get id(): number {
    return this.#surface.window
  }

  readonly api = {
    host: this.host,
    mount: (app: WindowApp) => this.#mount(app),
    setTitle: this.host.setTitle,
    setMenus: this.host.setMenus,
    ...Rpc.calls(() => this.#rpc, 'window', 'close', 'minimize', 'requestAttention', 'shortcut', 'unshortcut'),
    requestMode: (mode: WindowMode) => this.#surface.requestMode(mode),
    requestState: (state: WindowState) => this.#surface.requestState(state),
    setGrabHint: (rect: WindowRect, edges: readonly WindowEdge[] = []) =>
      this.#surface.setGrabHint(rect.x, rect.y, rect.width, rect.height, edges.reduce((bits, e) => bits | WindowSelf.#EDGES[e], 0)),
    setBackdrop: (rect: WindowRect | null, effect: { radius?: number; blur?: number; saturate?: number; brightness?: number } = {}) => {
      const r = rect ?? { x: 0, y: 0, width: 0, height: 0 }
      this.#surface.setBackdrop(r.x, r.y, r.width, r.height, effect.radius ?? 0, effect.blur ?? 0, effect.saturate ?? 1, effect.brightness ?? 1)
    },
    requestActivate: () => this.#surface.requestActivate(),
    requestGrab: (kind: 'pointer' | 'keyboard') => this.#surface.requestGrab(kind),
    releaseGrab: (kind: 'pointer' | 'keyboard') => this.#surface.releaseGrab(kind),
    on: (name: WindowEventName, handler: (value?: unknown) => unknown) => {
      if (!Object.hasOwn(WindowSelf.#OWN, name)) throw new TypeError(`acde:window has no ${String(name)} event (${WindowSelf.EVENTS.join(', ')})`)
      const set = this.#handlers.get(name) ?? new Set()
      set.add(handler)
      this.#handlers.set(name, set)
      const opening = name === 'request' ? this.#opening : null
      if (opening) {
        this.#opening = null
        queueMicrotask(() => handler(this.#received(opening)))
      }
      return () => set.delete(handler)
    },
  }

  #mount(app: WindowApp): void {
    const root = this.#page.document.getElementById('app')
    if (!root) throw new Error('acde.window.mount: the window document has no #app')
    this.#unmount = app.mount(root, this.host)
    void this.#rpc.call('window', 'ready').catch(() => undefined)
  }

  async serve(method: string, args: unknown[]): Promise<unknown> {
    switch (method) {
      case 'beforeclose': {
        for (const handler of this.#handlers.get('beforeclose') ?? []) if ((await handler()) === false) return false
        if (!WindowSelf.unload(this.#page)) return false
        this.#unmount?.()
        this.#unmount = null
        return true
      }
      case 'request':
        return this.deliver(args[0] as Record<string, unknown>)
      case 'visible':
        return this.onVisible(args[0] === true)
      case 'minimize':
        return this.#surface.requestState('minimized')
      case 'menu': {
        const [menu, item] = args as [string, string]
        const it = this.#menus.find((m) => m.id === menu)?.items.find((x) => 'id' in x && x.id === item) as { onSelect?(): unknown } | undefined
        return it?.onSelect?.()
      }
    }
    throw new Error(`a window web has no ${method}`)
  }

  static unload(page: PageLike): boolean {
    const window = page as unknown as { BeforeUnloadEvent?: typeof Event; PageTransitionEvent?: new (type: string, init?: { persisted?: boolean }) => Event }
    const BeforeUnload = window.BeforeUnloadEvent ?? page.Event
    if (!page.dispatchEvent(new BeforeUnload('beforeunload', { cancelable: true }))) return false
    if (window.PageTransitionEvent) page.dispatchEvent(new window.PageTransitionEvent('pagehide', { persisted: false }))
    page.dispatchEvent(new page.Event('unload'))
    return true
  }

  deliver(request: Record<string, unknown>): void {
    const handlers = this.#handlers.get('request')
    if (!handlers?.size) return void (this.#opening = request)
    const received = this.#received(request)
    for (const handler of handlers) handler(received)
  }

  #received(request: Record<string, unknown>): Record<string, unknown> {
    const { entry, ...rest } = request
    const given = entry as { path?: unknown; kind?: unknown; writable?: unknown } | undefined
    if (rest['kind'] !== 'open-file' || typeof given?.path !== 'string') return rest
    const handle = this.handOver?.handle(given.path, given.kind === 'directory' ? 'directory' : 'file', given.writable === true)
    return handle ? { ...rest, handle } : rest
  }

  emit(name: 'activate' | 'deactivate' | 'grab' | 'grablost' | 'state' | 'output' | 'hostcontrols' | 'minimizable', value?: unknown): void {
    for (const handler of this.#handlers.get(name) ?? []) handler(value)
  }
}

type ModuleHost = { defineModule(url: string, exports: object): void; refuseModule?(url: string, reason: string): void }

export class RemoteBridge {
  readonly value: Record<string, unknown> = {}
  readonly permits: Permits
  readonly #rpc: Rpc
  #capabilities: { update(status: CapabilityStatus): void } | null = null
  #host: ModuleHost | null = null
  readonly #waiting = new Set<string>()
  handOver: HandOver | null = null

  constructor(rpc: Rpc, init: WebInit, self: WindowSelf | null, windows: WindowList, permits = new Permits(init.tier?.approvals, init.app?.label ?? init.kind)) {
    this.#rpc = rpc
    this.permits = permits
    const permissions = () => ({
      ...Rpc.calls(() => rpc, 'permissions', 'pending', 'answer', 'list', 'revoke'),
      on: windows.notices.for('permissions', ['prompt', 'answered']),
    })
    for (const [service, shape] of Object.entries(init.services)) this.value[service] = this.#proxy(service, shape)
    const calls = Rpc.calls(() => rpc, 'apps', 'list', 'open')
    const apps = { ...calls, open: async (...args: unknown[]) => calls.open(...args.map((arg) => this.#outgoing(arg))) }
    const files = Rpc.calls(() => rpc, 'apps', 'openPath', 'handlers', 'setDefault')
    const shell = new Set<string>(init.tier?.shell ?? [])
    const globalWindows = () => ({
      ...windows.api,
      ...Rpc.calls(() => rpc, 'windows', 'kill', 'focus', 'hide', 'show', 'requestState', 'invokeMenu', 'stats', 'animate', 'shortcut', 'unshortcut'),
    })
    const tier = {
      packages: () => ({ ...Rpc.calls(() => rpc, 'packages', 'refresh', 'install', 'natives', 'runNative'), on: windows.notices.for('apps', ['changed']) }),
      launch: () => ({ ...Rpc.calls(() => rpc, 'launch', 'start', 'embed', 'close'), list: windows.api.list, on: windows.api.on }),
    }
    if (init.kind === 'desktop') {
      this.value['apps'] = { ...apps, ...files, on: windows.notices.for('apps', ['changed']) }
      const events = ['download', 'notification', 'screenshot', 'shortcut', 'edge']
      this.value['desktop'] = { apps: async () => ({ apps: await apps.list() }), on: windows.notices.for('desktop', events) }
      this.value['windows'] = globalWindows()
      Object.assign(this.value, { packages: tier.packages(), launch: tier.launch(), open: apps.open, permissions: permissions(), ...files })
    } else {
      const caps = init.app?.capabilities ?? {}
      this.value['apps'] = { list: apps.list, open: apps.open, on: windows.notices.for('apps', ['changed']) }
      this.value['open'] = apps.open
      if (caps['openFiles']) Object.assign(this.value, { openPath: files.openPath, handlers: files.handlers })
      if (caps['launch'] || caps['slots']) this.value['launch'] = tier.launch()
      if (shell.has('windows')) this.value['windows'] = globalWindows()
      if (shell.has('packages')) this.value['packages'] = tier.packages()
      if (shell.has('setDefault')) this.value['setDefault'] = files.setDefault
      if (shell.has('permissions')) this.value['permissions'] = permissions()
      if (shell.has('desktop')) {
        this.value['desktop'] = { apps: async () => ({ apps: await apps.list() }), on: windows.notices.for('desktop', ['download', 'notification', 'edge']) }
      }
      if (self) this.value['window'] = this.#window(self, windows)
    }
    if (init.tier?.capabilities) {
      const live = CapabilityCheck.module(init.tier.capabilities, (name) => rpc.call('capabilities', 'request', [name]) as Promise<CapabilityStatus>)
      this.value['capabilities'] = live.module
      this.#capabilities = live
    }
    this.#refused = init.tier?.refused ?? {}
  }

  readonly #refused: Readonly<Record<string, string>>

  #outgoing(arg: unknown): unknown {
    if (typeof arg !== 'object' || arg === null || (arg as { kind?: unknown }).kind !== 'open-file') return arg
    const { handle, held: _held, entry: _entry, ...request } = arg as Record<string, unknown>
    if (handle === undefined) return request
    const held = this.handOver?.held(handle)
    if (!held) throw new TypeError('open-file: the handle is not a file or folder this page was given')
    return { ...request, held }
  }

  #window(self: WindowSelf, windows: WindowList): unknown {
    const api = self.api
    const shortcut = windows.notices.for('window', WindowSelf.EVENTS)
    const on = (name: string, handler: (value?: unknown) => unknown) =>
      name === 'shortcut' ? shortcut(name, handler) : api.on(name as WindowEventName, handler)
    const value = { ...api, id: self.id, on }
    Object.defineProperty(value, 'state', { get: () => self.state, enumerable: true })
    Object.defineProperty(value, 'output', { get: () => self.output, enumerable: true })
    Object.defineProperty(value, 'hostControls', { get: () => self.hostControls, enumerable: true })
    Object.defineProperty(value, 'minimizable', { get: () => self.minimizable, enumerable: true })
    return value
  }

  define(host: ModuleHost): void {
    this.#host = host
    for (const name of Object.keys(this.value)) this.#define(name)
    for (const [url, reason] of Object.entries(this.#refused)) if (!(url.slice('acde:'.length) in this.value)) host.refuseModule?.(url, reason)
  }

  permit(capability: string, approval: Approval, status: CapabilityStatus | null): void {
    this.permits.set(capability, approval)
    if (status) this.#capabilities?.update(status)
    for (const name of [...this.#waiting]) this.#define(name)
  }

  #define(name: string): void {
    const host = this.#host
    if (!host) return
    if (!this.permits.usable(name)) {
      this.#waiting.add(name)
      return host.refuseModule?.(`acde:${name}`, this.permits.reason(name))
    }
    this.#waiting.delete(name)
    const value = this.permits.guard(name, this.value[name] as object)
    host.defineModule(`acde:${name}`, { [name]: value, default: value })
  }

  #proxy(service: string, shape: ServiceShape): unknown {
    const methods: Record<string, unknown> = { ...shape.values }
    for (const method of shape.methods) {
      methods[method] = method in shape.sync ? () => shape.sync[method] : (...args: unknown[]) => this.#rpc.call(service, method, args)
    }
    if (!shape.callable) return methods
    const call = (...args: unknown[]) => this.#rpc.call(service, '', args)
    for (const [key, value] of Object.entries(methods)) Object.defineProperty(call, key, { value, enumerable: true, configurable: true, writable: true })
    return call
  }
}
