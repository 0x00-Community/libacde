import { API_LEVEL, Manifest, SHARED_PLACES, SHELL_MODULES } from '@acde/api/manifest'
import type { ShellEvent } from '../app/events'
import type { Found } from '../app/app'
import { CALLBACK, sessionOf, windowGrant, type ServiceShape, type Session, type Welcome } from '../manager/manager'
import type { GrantOptions, LauncherShell } from '../shell'
import type { Rpc } from '../../../page/src/rpc'
import type { NativeDevTools, NativeDevToolsTarget } from '../dev/devtools'
import { join } from '../../../page/src/sys/path'
import { xdgHome } from '../../../page/src/sys/os'
import { mkdirSync } from '../../../page/src/sys/fs'
import { process } from '../../../page/src/sys/process'
import { ShellHub, type ShellView } from './hub'
import { LocalLink, type Peer, type PeerSide } from './link'
import { LocalRegistry, type LocalApp, type LocalService } from './registry'
import { DESKTOP_SUFFIX, LocalOpener, UNKNOWN, type Chosen } from './open'
import { sys } from '../../../page/src/sys/natives'

export interface SeatedPage {
  readonly window: number
  readonly rpc: Rpc | null
  readonly record: { readonly kind: 'web'; readonly app: string | null; readonly title: string | null }
}

export interface SessionWindows {
  web(window: number): Rpc | null
  record(window: number): SeatedPage['record'] | null
  close(window: number): Promise<unknown>
  records(): readonly number[]
  ref(window: number): string | null
}

export interface Seat {
  readonly shell: LauncherShell
  readonly devtools: NativeDevTools | undefined
  readonly grant: (() => GrantOptions) | null
  readonly windows: SessionWindows
  fresh(): Found | null
}

export interface SeatedLauncher {
  start(): void
  run(): Promise<number>
  close(force: boolean): Promise<boolean>
  devAnswer(request: Record<string, unknown>): Promise<unknown>
  page(): SeatedPage
}

export interface ServiceRunner {
  start(): Promise<void>
  run(): Promise<number>
}

export interface LocalHost {
  readonly shell: LauncherShell
  readonly opener?: LocalOpener
  openDefault?(path: string): Promise<void>
  readonly devtools?: NativeDevTools
  readonly registry: LocalRegistry
  launcher(session: Session, link: LocalLink, seat: Seat): SeatedLauncher
  service(link: LocalLink, welcome: Welcome, generation: number): ServiceRunner
  rescan(): LocalRegistry
}

interface Running {
  readonly peer: Peer
  readonly link: LocalLink
  readonly view: ShellView
  readonly launcher: SeatedLauncher
  readonly app: LocalApp | null
  window: number
  title: string
  menus: unknown
  web: string
}

interface Provided {
  readonly namespace: string
  readonly service: string
  readonly peer: Peer
  readonly shape: ServiceShape
}

interface Forward {
  readonly caller: Peer
  readonly resolve: (value: unknown) => void
  readonly reject: (error: Error) => void
  readonly service: Peer
}

interface LentFn {
  readonly page: Peer
  readonly number: number
  readonly services: Set<number>
}

const RESERVED = new Set(['lock', 'supervisor', 'windows', 'apps', 'window', 'open', 'desktop', 'desktops', 'session', 'debug', 'inspect', 'packages', 'launch',
  'openPath', 'handlers', 'setDefault', 'capabilities', 'permissions', 'icons', 'sounds', 'dev'])
const SYSTEM_CAPABILITIES = new Set(['fs', 'terminal', 'network', 'system', 'bluetooth', 'audio', 'devices', 'power', 'input', 'display', 'office', 'launch', 'slots',
  'open', 'openFiles', 'camera', 'microphone', 'idleDetection'])
const PRIVILEGED = ['terminal', 'system', 'bluetooth', 'devices', 'power', 'input', 'display', 'office'] as const
const SERVICE_READY_MS = 5000
const WEBS_MS = 1500
const SERVICE_ANSWER_MS = 30000

function refusal(status: number, code: string, message: string): Error {
  return Object.assign(new Error(message), { status, code })
}

const unsupported = (service: string, method: string) => refusal(501, 'unsupported', `${service}.${method} is not supported by acdev dev's local session`)
const notFound = (message: string) => refusal(404, 'not_found', message)
const badRequest = (message: string) => refusal(400, 'bad_request', message)
const forbidden = (message: string) => refusal(403, 'forbidden', message)

export class LocalSession implements PeerSide {
  readonly hub: ShellHub
  #registry: LocalRegistry
  readonly #running: Running[] = []
  readonly #services = new Map<string, { peer: Peer; link: LocalLink; spec: LocalService; ended: Promise<void> }>()
  #generation = 0
  readonly #provided: Provided[] = []
  readonly #forwards = new Map<number, Forward>()
  readonly #functions = new Map<number, LentFn>()
  readonly #instances = new Map<string, number>()
  #desktop: Running | null = null
  readonly #refs = new Map<number, string>()
  readonly #places = new Map<number, readonly [number, number, number, number]>()
  readonly #targets = new Map<number, readonly NativeDevToolsTarget[]>()
  readonly #lentFolders = new Map<number, Map<string, string>>()
  #nextRef = 1
  #next = 0
  readonly windows: SessionWindows = {
    web: (window) => this.#ownerOf(window)?.launcher.page().rpc ?? null,
    record: (window) => {
      const owner = this.#ownerOf(window)
      return owner ? { kind: 'web', app: owner.app?.app.id ?? null, title: owner.app ? owner.title : null } : null
    },
    close: (window) => this.#ownerOf(window)?.launcher.close(true) ?? Promise.resolve(this.host.shell.close(window)),
    records: () => this.#all().map((r) => r.launcher.page().window),
    ref: (window) => {
      if (window === 0 || window === this.#desktop?.window) return 'w0'
      let ref = this.#refs.get(window)
      if (!ref) this.#refs.set(window, (ref = `w${this.#nextRef++}`))
      return ref
    },
  }
  #ended: ((code: number) => void) | null = null
  #ending = false

  constructor(readonly host: LocalHost) {
    this.#registry = host.registry
    this.hub = new ShellHub(host.shell, (event, owner) => this.#route(event, owner))
    for (const line of this.#registry.refused) process.stderr.write(`acdev: ${line}\n`)
  }

  get registry(): LocalRegistry {
    return this.#registry
  }

  #peer(kind: Peer['kind'], app: string | null, service: string | null, instance = 0): Peer {
    return { id: ++this.#next, kind, app, service, instance }
  }

  async run(): Promise<number> {
    const done = new Promise<number>((resolve) => {
      this.#ended = resolve
    })
    await this.#startServices()
    this.#startDesktop()
    const code = await done
    await this.#shutdown()
    return code
  }

  end(code: number): void {
    const ended = this.#ended
    this.#ended = null
    ended?.(code)
  }

  async #startServices(): Promise<void> {
    const started = this.#registry.desktop.services.map(async (spec) => {
      const peer = this.#peer('service', null, spec.grant.name)
      const welcome = this.#serviceWelcome(spec)
      const link = new LocalLink(welcome, peer, this)
      const runner = this.host.service(link, welcome, this.#generation)
      let ended: () => void = () => {}
      this.#services.set(spec.grant.name, { peer, link, spec, ended: new Promise<void>((done) => (ended = done)) })
      try {
        await runner.start()
      } catch (error) {
        process.stderr.write(`acdev: service ${spec.grant.name}: ${(error as Error).message}\n`)
        this.#serviceGone(peer)
        ended()
        return
      }
      void runner.run().then(
        () => {
          this.#serviceGone(peer)
          ended()
        },
        (error: Error) => {
          process.stderr.write(`acdev: service ${spec.grant.name}: ${error.message}\n`)
          this.#serviceGone(peer)
          ended()
        },
      )
    })
    const late = new Promise<void>((done) => setTimeout(done, SERVICE_READY_MS))
    await Promise.race([Promise.allSettled(started), late])
  }

  #startDesktop(): void {
    const { desktop } = this.#registry
    const peer = this.#peer('desktop', null, null)
    const welcome = this.#desktopWelcome()
    const link = new LocalLink(welcome, peer, this)
    const view = this.hub.view()
    const seat: Seat = { shell: view.shell, devtools: this.#devtools(peer), grant: null, windows: this.windows, fresh: () => null }
    const launcher = this.host.launcher(sessionOf(welcome, 'desktop'), link, seat)
    const running: Running = { peer, link, view, launcher, app: null, window: 0, title: desktop.desktop.name, menus: null, web: 'desktop' }
    this.#desktop = running
    launcher.start()
    running.window = [...view.windows][0] ?? 0
    void launcher.run().then(
      (code) => this.end(code),
      (error: Error) => {
        process.stderr.write(`acdev: desktop: ${error.message}\n`)
        this.end(1)
      },
    )
  }

  async #shutdown(): Promise<void> {
    this.#ending = true
    await Promise.allSettled(this.#apps().map((r) => r.launcher.close(true)))
    for (const { link } of this.#services.values()) link.close()
    this.#desktop?.link.close()
  }

  #devtools(peer: Peer): NativeDevTools | undefined {
    const native = this.host.devtools
    if (!native) return undefined
    return {
      open: () => native.open(),
      targets: (targets) => {
        if (targets.length) this.#targets.set(peer.id, targets)
        else this.#targets.delete(peer.id)
        native.targets([...this.#targets.values()].flat())
      },
      ...(native.reloading && { reloading: (web: string) => native.reloading!(web) }),
    }
  }

  #apps(): Running[] {
    return this.#running.filter((r) => r.app !== null)
  }

  #all(): Running[] {
    return [this.#desktop, ...this.#apps()].filter((r): r is Running => r !== null)
  }

  #ownerOf(window: number): Running | null {
    return this.#all().find((r) => r.window === window || r.view.windows.has(window)) ?? null
  }

  static #declares(app: LocalApp, capability: string): boolean {
    return !!(app.app.capabilities as Record<string, unknown>)[capability]
  }

  #grants(app: LocalApp | null, key: string): Record<string, unknown> {
    const trusted = app === null || app.package.system
    const has = (capability: string) => app === null || LocalSession.#declares(app, capability)
    const privileged = (capability: (typeof PRIVILEGED)[number]) => trusted && has(capability)
    const root = process.env['ACDE_STORAGE'] || join(xdgHome('data'), 'acde', 'storage')
    const sandbox = !!process.env['ACDE_DEV_PLACES']
    const fs = app === null ? { places: sandbox ? [...SHARED_PLACES] : [], paths: false } : LocalSession.#declares(app, 'fs') ? { places: sandbox ? Manifest.places(app.app.capabilities) : [], paths: false } : null
    return {
      app: app?.app.id ?? null, fs, network: has('network'), audio: has('audio'),
      ...Object.fromEntries(PRIVILEGED.map((capability) => [capability, privileged(capability)])),
      camera: app !== null && has('camera'), microphone: app !== null && has('microphone'),
      storage: { key, dir: join(root, key), legacy: null },
    }
  }

  #desktopWelcome(): Welcome {
    const { desktop } = this.#registry
    const pkg = desktop.package
    return {
      api: API_LEVEL, kind: 'desktop', level: 'desktop', dir: pkg.dir, desktop: desktop.desktop,
      resources: [...pkg.resources, ...this.#registry.packageResources()],
      tier: { level: API_LEVEL, system: true, shell: [...SHELL_MODULES], refused: {} } as unknown as Welcome['tier'],
      grants: this.#grants(null, `desktop-${desktop.desktop.id}`) as unknown as Welcome['grants'],
      request: null, services: this.#shapesFor('desktop', null), session: { own: true },
    }
  }

  #appWelcome(app: LocalApp, instance: number, request: Record<string, unknown> | null): Welcome {
    const system = app.package.system
    const caps = app.app.capabilities as Record<string, unknown>
    const shell = SHELL_MODULES.filter((m) => system && !!caps[m])
    const tier = { level: API_LEVEL, system, shell, refused: {} }
    const welcome: Welcome = {
      api: API_LEVEL, kind: 'app', level: system ? 'system' : 'third-party', dir: app.dir, app: app.app, resources: app.resources,
      tier: tier as unknown as Welcome['tier'], grants: this.#grants(app, `app-${app.app.id}`) as unknown as Welcome['grants'], request, instance,
      services: this.#shapesFor('app', app),
    }
    return shell.includes('session') ? { ...welcome, session: { own: true } } : welcome
  }

  #serviceWelcome(spec: LocalService): Welcome {
    const { desktop } = this.#registry
    const pkg = desktop.package
    const state = join(xdgHome('state'), 'acde', pkg.name, spec.grant.name)
    try {
      mkdirSync(state, { recursive: true, mode: 0o700 })
    } catch (error) {
      process.stderr.write(`acdev: the state folder of service ${spec.grant.name}: ${(error as Error).message}\n`)
    }
    return {
      api: API_LEVEL, kind: 'service', level: 'service', name: spec.grant.name, dir: pkg.dir, module: spec.module, modules: spec.grant.modules,
      stateDir: state, resources: pkg.resources,
      tier: { level: API_LEVEL, system: true, shell: spec.grant.modules, refused: {} } as unknown as Welcome['tier'],
      grants: this.#grants(null, `service-${spec.grant.name}`) as unknown as Welcome['grants'], request: null,
      ...(spec.grant.backend && { backend: { role: spec.grant.backend, up: false } }),
    }
  }

  #shapesFor(kind: 'desktop' | 'app', app: LocalApp | null): Record<string, ServiceShape> {
    const allowed = (service: string) => kind === 'desktop' || (app?.app.services ?? []).includes(service)
    return Object.fromEntries(this.#provided.filter((p) => allowed(p.service)).map((p) => [p.namespace, p.shape]))
  }

  #tellShapes(): void {
    const desktop = this.#desktop
    if (desktop) desktop.link.push({ type: 'services', services: this.#shapesFor('desktop', null) })
    for (const running of this.#apps()) {
      if ((running.app!.app.services ?? []).length) running.link.push({ type: 'services', services: this.#shapesFor('app', running.app) })
    }
  }

  #toDesktop(event: Record<string, unknown> & { type: string }): void {
    this.#desktop?.link.push(event)
  }

  #route(event: ShellEvent, owner: ShellView | null): void {
    if ((event.type === 'windowgone' || event.type === 'windowgeometry') && typeof event.window === 'number') {
      const { x, y, width, height } = event as { x?: unknown; y?: unknown; width?: unknown; height?: unknown }
      const rect = [x, y, width, height]
      const normal = event.type === 'windowgone' || this.host.shell.windows().find((r) => (r.window ?? r.id) === event.window)?.state === 'normal'
      if (normal && rect.every((n) => typeof n === 'number' && Number.isFinite(n))) this.#places.set(event.window, rect as [number, number, number, number])
      if (event.type === 'windowgone') return
    }
    switch (event.type) {
      case 'devrequest':
        return void this.#devRequest(String(event['key'] ?? ''))
      case 'devreload':
        return this.#devReload(event)
      case 'quit':
      case 'close':
      case 'exit':
        return this.end(0)
      case 'devtools':
        return (owner ?? this.#desktop?.view)?.push(event)
      case 'windowcloserequest':
      case 'menuselect':
        return (owner ?? this.#desktop?.view)?.push(event)
      case 'shortcut': {
        const scope = event['title']
        if ((scope === 'app' || scope === 'menu') && owner && owner !== this.#desktop?.view) return owner.push(event)
        return this.#desktop?.view.push(event)
      }
    }
    this.#desktop?.view.push(event)
  }

  #devReload(event: ShellEvent): void {
    const app = typeof event['key'] === 'string' ? event['key'] : null
    this.#registry = this.host.rescan()
    if (app !== null) {
      for (const target of this.#apps().filter((r) => r.app!.app.id === app)) target.view.push(event)
      return
    }
    void this.#restartServices().then(() => this.#desktop?.view.push(event))
  }

  async #restartServices(): Promise<void> {
    const old = [...this.#services.values()]
    for (const service of old) service.link.close()
    await Promise.all(old.map((service) => service.ended))
    this.#generation++
    await this.#startServices()
  }

  #byWeb(name: unknown): Running | null {
    const all = [this.#desktop, ...this.#apps()].filter((r): r is Running => r !== null)
    if (name === undefined || name === null || name === '' || name === 'desktop') return this.#desktop
    const wanted = String(name)
    return all.find((r) => r.web === wanted)
      ?? this.#apps().filter((r) => r.app!.app.id === wanted).sort((a, b) => a.peer.instance - b.peer.instance)[0]
      ?? null
  }

  #focused(): Running | null {
    const row = this.host.shell.windows().find((r) => r.focus)
    const window = row ? (row.window ?? row.id) : null
    return this.#apps().find((r) => window !== null && r.view.windows.has(window)) ?? this.#desktop
  }

  async #answerWebs(): Promise<unknown> {
    const all = [this.#desktop, ...this.#apps()].filter((r): r is Running => r !== null)
    const answers = await Promise.all(all.map(async (r) => {
      const late = new Promise<null>((done) => setTimeout(() => done(null), WEBS_MS))
      const answer = (await Promise.race([r.launcher.devAnswer({ op: 'webs' }).catch(() => null), late])) as { webs?: object[] } | null
      if (answer?.webs) return answer.webs
      return [{ name: r.web, app: r.app?.app.id ?? null, kind: r.app ? 'app' : 'desktop', window: r.window, title: null, stalled: true }]
    }))
    return { webs: answers.flat() }
  }

  async #answerPages(request: Record<string, unknown>): Promise<unknown> {
    const { pages: _pages, since, ...rest } = request
    const all = [this.#desktop, ...this.#apps()].filter((r): r is Running => r !== null)
    const pages = await Promise.all(all.map(async (r) => {
      const key = `${r.app?.app.id ?? 'desktop'}:${process.pid}#${r.peer.id}`
      const mark = since && typeof since === 'object' ? (since as Record<string, unknown>)[key] : undefined
      try {
        const answer = (await r.launcher.devAnswer({ ...rest, ...(typeof mark === 'number' && { since: mark }) })) as Record<string, unknown>
        return { key, app: r.app?.app.id ?? null, pid: process.pid, ...answer }
      } catch (error) {
        return { key, app: r.app?.app.id ?? null, pid: process.pid, error: (error as Error).message }
      }
    }))
    return { pages }
  }

  #answerOpen(request: Record<string, unknown>): unknown {
    const text = (value: unknown) => (typeof value === 'string' && value !== '' ? value : null)
    const name = text(request['name'])
    const path = text(request['path'])
    const url = text(request['url'])
    if ((request['name'] !== undefined && name === null) || (request['path'] !== undefined && path === null) || (request['url'] !== undefined && url === null)) {
      return { ok: false, text: 'open: name, path and url are non-empty strings' }
    }
    if (path !== null && url !== null) return { ok: false, text: 'open: give a path or a url, not both' }
    if (path === null && url === null && name === null) return { ok: false, text: 'open: give name, path or url' }
    try {
      if (url !== null) {
        if (!/^https?:/i.test(url)) return { ok: false, text: `open: ${url}: only http and https links open here` }
        const opened = this.#openLink(url, name === null ? null : (this.#registry.byName(name)?.app.id ?? name), false) as { app: string }
        return { ok: true, text: `opened ${url} with ${opened.app}`, app: opened.app }
      }
      if (path !== null) {
        if (!path.startsWith('/') || !this.host.opener?.files.exists(path)) return { ok: false, text: `open: ${path} is not an absolute path that exists` }
        const target = name === null ? null : name.endsWith(DESKTOP_SUFFIX) ? name : (this.#registry.byName(name)?.app.id ?? name)
        return this.#openFile(path, target, false, true).then(
          (opened) => ({ ok: true, text: `opened ${path} with ${(opened as { id?: string }).id ?? 'the system'}`, app: (opened as { id?: string }).id }),
          (error: Error) => ({ ok: false, text: `open: ${error.message}` }),
        )
      }
      const app = this.#registry.byName(name!)
      if (!app) return { ok: false, text: `open: no app ${name} (apps: ${this.#registry.apps.map((a) => a.app.id).join(', ') || 'none'})` }
      this.#start(app, null, false)
      return { ok: true, text: `opened ${app.app.id}`, app: app.app.id }
    } catch (error) {
      return { ok: false, text: `open: ${(error as Error).message}` }
    }
  }

  async #devRequest(line: string): Promise<void> {
    let id: number | null = null
    let request: Record<string, unknown>
    try {
      request = JSON.parse(line) as Record<string, unknown>
      id = typeof request['id'] === 'number' ? request['id'] : null
    } catch {
      return
    }
    const respond = (ok: boolean, payload: string) => {
      if (id !== null) this.host.shell.devRespond?.(id, ok, payload)
    }
    const op = String(request['op'] ?? '')
    try {
      if (op === 'webs') return respond(true, JSON.stringify(await this.#answerWebs()))
      if ((op === 'console' || op === 'network') && request['pages'] === true) return respond(true, JSON.stringify(await this.#answerPages(request)))
      if (op === 'open') return respond(true, JSON.stringify(await this.#answerOpen(request)))
      const target = op === 'snapshot' && request['web'] === undefined ? this.#focused() : this.#byWeb(request['web'])
      if (!target) {
        const known = [this.#desktop, ...this.#apps()].filter(Boolean).map((r) => r!.web).join(', ')
        throw notFound(`no web ${String(request['web'])} (webs: ${known}; a web name, an app id or desktop)`)
      }
      target.view.push({ type: 'devrequest', key: line })
    } catch (error) {
      respond(false, (error as Error).message)
    }
  }

  call(peer: Peer, service: string, method: string, args: readonly unknown[]): Promise<unknown> {
    try {
      if (peer.kind === 'service') {
        this.#serviceMay(peer, service, method)
        return Promise.resolve(this.#answer(peer, service, method, args))
      }
      if (this.#admit(peer, service, method, args) === 'service') return this.#callService(peer, service, method, args)
      return Promise.resolve(this.#answer(peer, service, method, args))
    } catch (error) {
      return Promise.reject(error as Error)
    }
  }

  #admit(peer: Peer, service: string, method: string, args: readonly unknown[]): 'manager' | 'service' {
    const desktop = peer.kind === 'desktop'
    const app = peer.app ? (this.#registry.app(peer.app) ?? null) : null
    const label = app?.app.label ?? 'the desktop'
    const declares = (capability: string) => desktop || (app !== null && LocalSession.#declares(app, capability))
    const needs = (what: string, capability: string) => forbidden(`${what} needs "${capability}" in the capabilities of ${label}`)
    const shell = (module: string) => {
      if (desktop) return
      if (!app?.package.system) throw forbidden(`acde:${module} is a shell module: only the desktop, its services and system apps may import it, and ${label} is not a system app`)
      if (!declares(module)) throw forbidden(`acde:${module} is a shell module: declare "${module}" in the capabilities of ${label}`)
    }
    const call = `${service}.${method}`
    if (service === 'session' || service === 'desktops') {
      shell(service)
      return 'manager'
    }
    if (['window.ready', 'window.setTitle', 'window.setMenus', 'window.requestAttention', 'window.openUrl', 'page.shown', 'page.notify', 'apps.list'].includes(call)) return 'manager'
    if (call === 'apps.open') {
      this.#mayOpen(peer, args)
      return 'manager'
    }
    if (call === 'apps.openPath' || call === 'apps.handlers') {
      if (!declares('openFiles')) throw needs(`acde:${method}`, 'openFiles')
      return 'manager'
    }
    if (call === 'apps.setDefault') return shell('setDefault'), 'manager'
    if (service === 'apps' || service === 'packages') return shell('packages'), 'manager'
    if (call === 'capabilities.request') {
      const name = args[0]
      if (typeof name !== 'string') throw badRequest('capabilities.request needs a capability name')
      if (!declares(name)) throw needs(`capabilities.request(${JSON.stringify(name)})`, name)
      return 'manager'
    }
    if (service === 'launch') {
      if (!declares('launch') && !declares('slots')) throw needs('acde:launch', 'launch" or "slots')
      return 'manager'
    }
    if (service === 'windows' || service === 'permissions' || service === 'software') return shell(service), 'manager'
    if ((SHELL_MODULES as readonly string[]).includes(service)) return shell(service), 'manager'
    if (service === 'inspect' || service === 'debug' || service === 'dev') {
      if (!desktop) throw forbidden(`${call}: only the desktop or a development session`)
      return 'manager'
    }
    if (RESERVED.has(service) || SYSTEM_CAPABILITIES.has(service)) throw unsupported(service, method)
    if (desktop || (app?.app.services ?? []).length > 0) return 'service'
    throw forbidden(`acde:${service} is not a system module; if a desktop service provides it, list that service's name in the services of ${label}`)
  }

  #serviceMay(peer: Peer, service: string, method: string): void {
    if (service === 'apps' && method === 'list') return
    const granted = this.#services.get(peer.service ?? '')?.spec.grant.modules ?? []
    if ((SHELL_MODULES as readonly string[]).includes(service) && (granted as readonly string[]).includes(service)) return
    throw forbidden(`service ${peer.service} may call acde:${service} only if the desktop lists it in the service's modules`)
  }

  #recordOf(peer: Peer): Running | null {
    if (this.#desktop?.peer.id === peer.id) return this.#desktop
    return this.#running.find((r) => r.peer.id === peer.id) ?? null
  }

  #answer(peer: Peer, service: string, method: string, args: readonly unknown[]): unknown {
    const desktop = peer.kind === 'desktop'
    const me = this.#recordOf(peer)
    switch (`${service}.${method}`) {
      case 'window.ready':
      case 'page.shown':
      case 'window.requestAttention':
        return null
      case 'window.setTitle': {
        if (!me || desktop) return null
        me.title = String(args[0] ?? '') || me.app!.app.label
        this.#toDesktop({ type: 'windowtitle', window: me.window, appId: me.app!.app.id, title: me.title })
        return null
      }
      case 'window.setMenus': {
        if (!me || desktop) return null
        me.menus = Array.isArray(args[0]) ? args[0] : []
        this.#toDesktop({ type: 'windowmenus', window: me.window, app: me.app!.app.id, instance: me.peer.instance, menus: me.menus })
        return null
      }
      case 'window.openUrl':
        return this.#openLink(String(args[0] ?? ''), null, false)
      case 'page.notify': {
        const options = (args[0] ?? {}) as Record<string, unknown>
        const clip = (value: unknown, max: number) => (typeof value === 'string' ? value.slice(0, max) : '')
        this.#toDesktop({
          type: 'notification', window: 0, app: me?.app?.app.id ?? null, label: me?.app?.app.label ?? null,
          title: clip(options['title'], 200), body: clip(options['body'], 2000), icon: clip(options['icon'], 4096), tag: clip(options['tag'], 200),
        })
        return true
      }
      case 'apps.list':
        return this.#registry.listed()
      case 'apps.open':
        return this.#open(peer, args)
      case 'apps.handlers':
        return this.#handlers(String(args[0] ?? ''))
      case 'apps.openPath': {
        const options = (args[1] ?? {}) as { app?: unknown }
        const natives = desktop || me?.app?.package.system === true
        return this.#openFile(String(args[0] ?? ''), typeof options.app === 'string' ? options.app : null, false, natives)
      }
      case 'apps.setDefault':
        return this.#setDefault(args)
      case 'apps.unlend':
        return this.#unlend(args[0])
      case 'capabilities.request': {
        const name = String(args[0] ?? '')
        const declared = desktop || Object.hasOwn(me?.app?.app.capabilities ?? {}, name)
        return { name, kind: 'service', state: declared ? 'available' : 'not-declared', reason: declared ? '' : `not in the capabilities of ${me?.app?.app.label ?? 'this app'}`, optional: false }
      }
      case 'permissions.list':
        return []
      case 'permissions.pending':
        return []
      case 'windows.screenshot':
        return this.#screenshot(args[0])
      case 'windows.invokeMenu':
        return this.#invokeMenu(args)
      case 'desktops.list':
        return { desktops: [{ id: this.#registry.desktop.desktop.id, name: this.#registry.desktop.desktop.name }], current: this.#registry.desktop.desktop.id }
      case 'session.others':
        return []
      case 'session.logout':
        setTimeout(() => this.end(0), 0)
        return null
    }
    throw unsupported(service, method)
  }

  async #screenshot(given: unknown): Promise<unknown> {
    const { screen, window, path } = (given ?? {}) as { screen?: unknown; window?: unknown; path?: unknown }
    if (window !== undefined && window !== null) throw unsupported('windows', 'screenshot of one window')
    if (screen !== undefined && screen !== null && screen !== 0) throw unsupported('windows', `screenshot of screen ${String(screen)}`)
    if (typeof path !== 'string' || !path.startsWith('/')) throw badRequest('windows.screenshot: path is an absolute path')
    const { width, height } = await this.host.shell.screenshot(path)
    return { path, width, height }
  }

  #invokeMenu(args: readonly unknown[]): null {
    const target = (args[0] ?? {}) as { window?: unknown; pid?: unknown }
    const window = typeof target === 'number' ? target : target.window
    const owner = this.#apps().find((r) => r.window === window || r.view.windows.has(Number(window)))
    if (!owner) throw notFound('windows.invokeMenu: no app owns that window')
    owner.link.push({ type: 'menuinvoke', menu: args[1] ?? null, item: args[2] ?? null })
    return null
  }

  #open(peer: Peer, args: readonly unknown[]): unknown {
    const desktop = peer.kind === 'desktop'
    const [target, rawRequest] = typeof args[0] === 'string' ? [args[0], args[1]] : [null, args[0]]
    const request = rawRequest && typeof rawRequest === 'object' ? { ...(rawRequest as Record<string, unknown>) } : null
    const options = (typeof args[0] === 'string' ? args[2] : args[1]) as Record<string, unknown> | undefined
    const newWindow = options?.['newWindow'] === true
    const root = options?.['root'] === undefined || options['root'] === null ? null : LocalSession.#folder(options['root'])
    if (root && !desktop) throw forbidden('only the desktop lends an app a folder (options.root)')
    const kind = request?.['kind']
    const system = kind === 'open-link' || kind === 'open-file'
    if (root === null && (target === null || system)) {
      if (!request) throw badRequest('apps.open needs an app id or a system request')
      if (kind === 'open-link') return this.#openLink(String(request['url'] ?? ''), target, newWindow)
      if (kind === 'open-file') {
        const path = request['path'] ?? request['file']
        if (typeof path !== 'string' || !path) throw badRequest('an open-file request needs a path here')
        return this.#openFile(path, target, newWindow, false)
      }
      throw badRequest('a system request is open-file or open-link')
    }
    if (target === null) throw badRequest('lending a folder names the app: apps.open(app, request, {root})')
    const app = this.#registry.app(target)
    if (!app) throw notFound(`no app ${target}`)
    if (root && !LocalSession.#declares(app, 'fs')) throw refusal(409, 'not_accepted', `${target} does not declare "fs" in its capabilities: it cannot take a folder`)
    if (request && !system) {
      if (typeof kind !== 'string') throw refusal(400, 'untyped_request', `a request to ${target} needs a kind`)
      if (!app.app.requests.includes(kind)) throw refusal(409, 'not_accepted', `${target} does not accept ${kind} requests (its manifest's requests)`)
    }
    return this.#start(app, request, newWindow, root?.dir ?? null)
  }

  static #folder(given: unknown): { dir: string } {
    const root = given as { dir?: unknown } | string
    const dir = typeof root === 'string' ? root : root?.dir
    if (typeof dir !== 'string' || !dir.startsWith('/')) throw badRequest('options.root is a folder: an absolute path, or {dir, label}')
    return { dir }
  }

  #lendFolder(record: Running, dir: string): string {
    const [entry] = sys().lendPaths?.(record.app!.app.id, [dir]) ?? []
    if (!entry || entry.kind !== 'directory') throw badRequest(`${dir} is not a folder this session can lend`)
    let folders = this.#lentFolders.get(record.peer.id)
    if (!folders) this.#lentFolders.set(record.peer.id, (folders = new Map()))
    folders.set(dir, entry.name)
    return entry.name
  }

  #unlend(given: unknown): unknown {
    if (typeof given !== 'string' || !given) throw badRequest("apps.unlend needs a folder's path")
    let revoked = 0
    for (const record of this.#apps()) {
      const name = this.#lentFolders.get(record.peer.id)?.get(given)
      if (name === undefined) continue
      this.#lentFolders.get(record.peer.id)!.delete(given)
      record.link.push({ type: 'unlend', name })
      revoked++
    }
    return { revoked }
  }

  #opener(): LocalOpener {
    if (!this.host.opener) throw unsupported('apps', 'openPath')
    return this.host.opener
  }

  #chosen(mime: string, natives: ReturnType<LocalOpener['natives']>): Chosen | null {
    return this.#opener().chosen(mime, natives, (id) => this.#registry.app(id) !== undefined, (type) => this.#registry.forType(type)?.app.id ?? null)
  }

  async #openFile(path: string, target: string | null, newWindow: boolean, nativesAllowed: boolean): Promise<unknown> {
    if (!path.startsWith('/')) throw badRequest(`${path} is not an absolute path`)
    const opener = this.#opener()
    const mime = opener.mimeOf(path)
    let chosen: Chosen | null
    if (target !== null && this.#registry.app(target)) chosen = { kind: 'acde', app: target }
    else if (target !== null && target.endsWith(DESKTOP_SUFFIX)) {
      if (!nativesAllowed) throw forbidden(`${target}: only the desktop and system apps open files with programs`)
      const native = opener.natives(mime).find((n) => n.id === target)
      if (!native) throw notFound(`${target} does not open ${mime}`)
      chosen = { kind: 'native', native }
    } else if (target !== null) throw notFound(`no app ${target}`)
    else chosen = this.#chosen(mime, opener.natives(mime))
    if (chosen?.kind === 'acde') {
      const app = this.#registry.app(chosen.app)!
      const [entry] = sys().lendPaths?.(app.app.id, [path]) ?? []
      if (!entry) throw badRequest(`${path} is neither a file nor a folder this session can lend`)
      return this.#start(app, { kind: 'open-file', file: entry.label, entry }, newWindow)
    }
    if (chosen?.kind === 'native') {
      if (!nativesAllowed) throw refusal(404, 'no_app', `no app opens ${mime} (${path})`)
      const spawn = sys().spawn
      if (!spawn) throw unsupported('apps', 'openPath with a program')
      const pid = spawn(['gio', 'launch', chosen.native.file, path], { env: { ...process.env } as Record<string, string>, prefix: `[${chosen.native.id}] ` }, () => {})
      return { kind: 'native', id: chosen.native.id, window: 0, pid }
    }
    if (nativesAllowed && this.host.openDefault) {
      await this.host.openDefault(path)
      return { kind: 'native', id: null, window: 0, pid: 0 }
    }
    throw refusal(404, 'no_app', `no app opens ${mime} (${path})`)
  }

  #handlers(path: string): unknown {
    const opener = this.#opener()
    const mime = opener.mimeOf(path)
    const natives = opener.natives(mime)
    const chosen = this.#chosen(mime, natives)
    const preferred = chosen?.kind === 'acde' ? chosen.app : chosen?.kind === 'native' ? chosen.native.id : null
    const handles = (types: readonly string[]) => types.some((t) => t === mime || (t.endsWith('/*') && mime.split('/')[0] === t.slice(0, -2)))
    const acde = this.#registry.apps.filter((a) => handles(a.app.mimeTypes)).map((a) => ({
      kind: 'acde', id: a.app.id, label: a.app.label, icon: a.app.icon, default: preferred === a.app.id, system: a.package.system,
    }))
    const native = natives.map((n) => ({ kind: 'native', id: n.id, label: n.label, icon: n.icon ?? '', default: preferred === n.id }))
    return { mime: mime === UNKNOWN ? '' : mime, association: mime === UNKNOWN ? null : mime, handlers: [...acde, ...native] }
  }

  #setDefault(args: readonly unknown[]): unknown {
    const opener = this.#opener()
    const mime = args[0]
    if (typeof mime !== 'string' || !mime.includes('/')) throw badRequest('setDefault needs a MIME type')
    const handler = args[1]
    if (typeof handler !== 'string') throw badRequest('setDefault needs an app id or a .desktop id')
    const id = this.#registry.app(handler) ? LocalOpener.desktopId(handler) : opener.natives(mime).some((n) => n.id === handler) ? handler : null
    if (id === null) throw notFound(`${handler} is neither an app nor a program that opens ${mime}`)
    try {
      opener.remember(mime, id)
    } catch (error) {
      throw refusal(503, 'unavailable', (error as Error).message)
    }
    return handler
  }

  #mayOpen(peer: Peer, args: readonly unknown[]): void {
    if (peer.kind === 'desktop') return
    const caller = peer.app ? (this.#registry.app(peer.app) ?? null) : null
    const label = caller?.app.label ?? 'this caller'
    const [target, request] = typeof args[0] === 'string' ? [args[0], args[1]] : [null, args[0]]
    const kind = (request as { kind?: unknown } | null | undefined)?.kind
    if (target === null) {
      if (kind !== 'open-file' && kind !== 'open-link') throw badRequest('apps.open needs an app id or a system request (open-file, open-link)')
      if (caller && LocalSession.#declares(caller, 'openFiles')) return
      throw forbidden(`opening files and links by type needs "openFiles" in the capabilities of ${label}`)
    }
    const open = (caller?.app.capabilities as Record<string, unknown> | undefined)?.['open']
    if (open === true || (Array.isArray(open) && open.includes(target))) return
    throw forbidden(`open ${target} needs "open" (true or a list with ${target}) in the capabilities of ${label}`)
  }

  #openLink(url: string, target: string | null, newWindow: boolean): unknown {
    const colon = url.indexOf(':')
    if (colon <= 0) throw badRequest(`${url} is not a URL`)
    const scheme = url.slice(0, colon).toLowerCase()
    const app = target !== null ? this.#registry.app(target) : this.#registry.forScheme(scheme)
    if (!app) throw notFound(`no app opens ${scheme}: links`)
    return this.#start(app, { kind: 'open-link', url }, newWindow)
  }

  #start(app: LocalApp, request: Record<string, unknown> | null, newWindow: boolean, folder: string | null = null): unknown {
    const id = app.app.id
    const fresh = newWindow && app.app.multiple
    const running = fresh ? undefined : this.#apps().filter((r) => r.app!.app.id === id).sort((a, b) => a.peer.instance - b.peer.instance)[0]
    if (running) {
      const root = folder === null ? null : this.#lendFolder(running, folder)
      if (root !== null) running.link.push({ type: 'lend', name: root })
      if (request) running.link.push({ type: 'request', request: root === null ? request : { ...request, root }, lent: [] })
      this.host.shell.focus(running.window)
      return { kind: 'acde', id, app: id, window: running.window, pid: process.pid, instance: running.peer.instance, running: true }
    }
    const instance = (this.#instances.get(id) ?? 0) + 1
    this.#instances.set(id, instance)
    const peer = this.#peer('app', id, null, instance)
    const lent = folder === null ? null : sys().lendPaths?.(id, [folder])?.[0] ?? null
    if (folder !== null && (!lent || lent.kind !== 'directory')) throw badRequest(`${folder} is not a folder this session can lend`)
    const welcome = this.#appWelcome(app, instance, request && lent ? { ...request, root: lent.name } : request)
    const link = new LocalLink(welcome, peer, this)
    const view = this.hub.view()
    const sized: GrantOptions = { mode: 'free', ...windowGrant(welcome, app.app.size) }
    let record: Running | null = null
    const grant = (): GrantOptions => {
      this.hub.flush()
      const place = record && record.window !== 0 ? this.#places.get(record.window) : undefined
      return place ? { ...sized, rect: [...place] } : sized
    }
    const seat: Seat = { shell: view.shell, devtools: this.#devtools(peer), grant, windows: this.windows, fresh: () => this.host.rescan().app(id) ?? null }
    const launcher = this.host.launcher(sessionOf(welcome, 'app'), link, seat)
    const web = `${id}#${instance}`
    record = { peer, link, view, launcher, app, window: 0, title: app.app.label, menus: null, web }
    this.#running.push(record)
    try {
      launcher.start()
    } catch (error) {
      this.#gone(record)
      throw refusal(503, 'unavailable', `${id}: ${(error as Error).message}`)
    }
    record.window = [...view.windows][0] ?? 0
    if (lent) {
      this.#lentFolders.set(peer.id, new Map([[folder!, lent.name]]))
      link.push({ type: 'lend', name: lent.name })
    }
    this.#toDesktop({ type: 'windowopen', window: record.window, appId: id, title: app.app.label, web, instance })
    void launcher.run().then(
      () => this.#gone(record),
      (error: Error) => {
        process.stderr.write(`acdev: ${web}: ${error.message}\n`)
        this.#gone(record)
      },
    )
    return { kind: 'acde', id, app: id, window: record.window, pid: process.pid, instance }
  }

  #gone(record: Running): void {
    const at = this.#running.indexOf(record)
    if (at < 0) return
    this.#running.splice(at, 1)
    this.#forgetCallbacksOf(record.peer)
    record.link.close()
    record.view.releaseAll()
    this.#refs.delete(record.window)
    this.#places.delete(record.window)
    this.#lentFolders.delete(record.peer.id)
    if (this.#targets.delete(record.peer.id)) this.host.devtools?.targets([...this.#targets.values()].flat())
    if (record.window && !this.#ending) this.#toDesktop({ type: 'windowclose', window: record.window })
  }

  #callService(caller: Peer, namespace: string, method: string, args: readonly unknown[]): Promise<unknown> {
    const app = caller.app ? this.#registry.app(caller.app) : null
    const allowed = (service: string) => caller.kind !== 'app' || (app?.app.services ?? []).includes(service)
    const provided = this.#provided.find((p) => p.namespace === namespace && allowed(p.service))
    if (!provided) {
      if (caller.kind !== 'app' || (app?.app.services ?? []).includes(namespace)) return Promise.reject(refusal(503, 'unavailable', `acde.${namespace} is not running`))
      const label = app?.app.label ?? 'the desktop'
      return Promise.reject(forbidden(`acde:${namespace} is not a system module; if a desktop service provides it, list that service's name in the services of ${label}`))
    }
    const service = this.#services.get(provided.service)
    if (!service) return Promise.reject(refusal(503, 'unavailable', `acde.${namespace} is not running`))
    const forward = ++this.#next
    const lent = args.map((a) => this.#lendFunctions(caller, provided.peer, a))
    const callerJson = caller.kind === 'app' ? { kind: 'app', app: caller.app, instance: caller.instance } : { kind: 'desktop' }
    return new Promise<unknown>((resolve, reject) => {
      this.#forwards.set(forward, { caller, resolve, reject, service: provided.peer })
      const timer = setTimeout(() => {
        if (this.#forwards.delete(forward)) reject(refusal(504, 'timeout', `acde.${namespace}.${method} did not answer within ${SERVICE_ANSWER_MS} ms`))
      }, SERVICE_ANSWER_MS)
      const settle = this.#forwards.get(forward)!
      this.#forwards.set(forward, {
        ...settle,
        resolve: (value) => {
          clearTimeout(timer)
          resolve(value)
        },
        reject: (error) => {
          clearTimeout(timer)
          reject(error)
        },
      })
      service.link.push({ type: 'call', id: forward, service: namespace, method, args: lent, caller: callerJson })
    })
  }

  #lendFunctions(page: Peer, service: Peer, value: unknown): unknown {
    if (Array.isArray(value)) return value.map((v) => this.#lendFunctions(page, service, v))
    if (typeof value !== 'object' || value === null) return value
    const record = value as Record<string, unknown>
    const keys = Object.keys(record)
    if (keys.length === 1 && typeof record[CALLBACK] === 'number') {
      const number = record[CALLBACK] as number
      const known = [...this.#functions].find(([, f]) => f.page.id === page.id && f.number === number)
      if (known) {
        known[1].services.add(service.id)
        return { [CALLBACK]: known[0] }
      }
      const global = ++this.#next
      this.#functions.set(global, { page, number, services: new Set([service.id]) })
      return { [CALLBACK]: global }
    }
    return Object.fromEntries(Object.entries(record).map(([k, v]) => [k, this.#lendFunctions(page, service, v)]))
  }

  frame(peer: Peer, frame: Record<string, unknown>): void {
    if (peer.kind === 'service') return this.#serviceFrame(peer, frame)
    const release = (frame['release'] as { fn?: unknown } | undefined)?.fn
    if (typeof release === 'number') this.#release(peer, release)
  }

  #serviceFrame(peer: Peer, frame: Record<string, unknown>): void {
    const ready = frame['ready'] as { provides?: Record<string, ServiceShape> } | undefined
    if (ready) return this.#serviceReady(peer, ready.provides ?? {})
    const invoke = frame['invoke'] as { fn?: unknown; args?: unknown } | undefined
    if (invoke && typeof invoke.fn === 'number') return this.#invoke(peer, invoke.fn, invoke.args ?? [])
    if (typeof frame['id'] === 'number') {
      const forward = this.#forwards.get(frame['id'])
      if (!forward || forward.service.id !== peer.id) return
      this.#forwards.delete(frame['id'])
      if (Object.hasOwn(frame, 'error')) {
        const e = (frame['error'] ?? {}) as { status?: number; code?: string; message?: string }
        forward.reject(refusal(e.status ?? 500, e.code ?? 'internal', e.message ?? 'the service failed'))
      } else forward.resolve(frame['ok'] ?? null)
    }
  }

  #serviceReady(peer: Peer, provides: Record<string, ServiceShape>): void {
    const name = peer.service!
    for (let i = this.#provided.length - 1; i >= 0; i--) if (this.#provided[i]!.peer.id === peer.id) this.#provided.splice(i, 1)
    for (const [namespace, shape] of Object.entries(provides)) {
      const other = this.#provided.find((p) => p.namespace === namespace)
      if (RESERVED.has(namespace) || SYSTEM_CAPABILITIES.has(namespace)) process.stderr.write(`acdev: service ${name}: acde:${namespace} is a system module's name: dropped\n`)
      else if (other) process.stderr.write(`acdev: service ${name}: acde:${namespace} is already service ${other.service}'s: dropped\n`)
      else this.#provided.push({ namespace, service: name, peer, shape })
    }
    this.#tellShapes()
  }

  #invoke(service: Peer, fn: number, args: unknown): void {
    const lent = this.#functions.get(fn)
    if (!lent || !lent.services.has(service.id)) return this.#services.get(service.service ?? '')?.link.push({ type: 'release', fn })
    const page = this.#recordOf(lent.page)
    page?.link.push({ type: 'invoke', fn: lent.number, args })
  }

  #release(page: Peer, number: number): void {
    const found = [...this.#functions].find(([, f]) => f.page.id === page.id && f.number === number)
    if (!found) return
    this.#functions.delete(found[0])
    for (const id of found[1].services) [...this.#services.values()].find((s) => s.peer.id === id)?.link.push({ type: 'release', fn: found[0] })
  }

  #forgetCallbacksOf(page: Peer): void {
    for (const [, f] of [...this.#functions]) if (f.page.id === page.id) this.#release(page, f.number)
    for (const [id, f] of [...this.#forwards]) if (f.caller.id === page.id) this.#forwards.delete(id)
  }

  #serviceGone(peer: Peer): void {
    for (const [id, f] of [...this.#forwards]) {
      if (f.service.id !== peer.id) continue
      this.#forwards.delete(id)
      f.reject(refusal(503, 'unavailable', 'the desktop service ended before it answered'))
    }
    for (const f of this.#functions.values()) f.services.delete(peer.id)
    const had = this.#provided.length
    for (let i = this.#provided.length - 1; i >= 0; i--) if (this.#provided[i]!.peer.id === peer.id) this.#provided.splice(i, 1)
    if (this.#services.get(peer.service ?? '')?.peer.id === peer.id) this.#services.delete(peer.service ?? '')
    if (this.#provided.length !== had) this.#tellShapes()
  }
}
