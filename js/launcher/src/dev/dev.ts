import { DevLog, type DevPost, type DevQuery } from '../../../page/src/devlog'
import { DevRoutes, type DevRoute } from '../../../page/src/devroutes'
import { isAbsolute } from '../../../page/src/sys/path'
import type { ConsoleLevel } from '@acde/dom'
import type { InspectArg } from '@acde/api'
import type { Inspect, InspectInput } from './inspect'

export interface DevTarget {
  readonly name: string
  readonly app: string | null
  readonly kind: 'app' | 'desktop'
  readonly window: number
  readonly title: string
  readonly frame: number
  readonly lock?: boolean
  call(service: string, method: string, args: readonly unknown[]): Promise<unknown>
  post(message: Record<string, unknown>): void
}

export interface DevShell {
  devRespond?(id: number, ok: boolean, payload: string): void
  screenshot(path: string): Promise<{ width: number; height: number }>
  capture?(window: number): Promise<{ width: number; height: number; png: string }>
  inject?(event: { type: 'move'; x: number; y: number }): void
  answerFolder?(path: string | null): void
  answerOpen?(paths: string[] | null): void
  answerSave?(path: string | null): void
  drop?(window: number, paths: string[], x?: number, y?: number): void
  readClipboard?(): string
  writeClipboard?(text: string): void
  resizeScreen?(width: number, height: number): void
}

export interface DevToolsPort {
  port(): { port: number }
}

export interface DevHost {
  readonly shell: DevShell
  readonly devtools?: DevToolsPort | null
  page(): DevTarget | null
  inspect(): Inspect
  reload(): Promise<void>
  respond?(id: number, ok: boolean, payload: string): void
  place?(windows: unknown): void
  readonly production?: boolean
}

interface DevInit {
  readonly id: number
  readonly web: string | null
  readonly code: string
  readonly once: boolean
}

type Request = Record<string, unknown> & { readonly id: number; readonly op: string }

export class DevChannel {
  static readonly LOCK = 'lock'
  readonly #inflight = new Set<Promise<void>>()
  static readonly STATE_MS = 500
  static readonly SINGLE = 'a single-app session (acdev dev --app) has no other apps'
  static readonly PRODUCTION: ReadonlySet<string> = new Set(['webs', 'snapshot', 'query', 'act', 'run'])
  readonly log = new DevLog()
  readonly #host: DevHost
  #routes: DevRoute[] = []
  #nextRoute = 1
  #inits: DevInit[] = []
  #nextInit = 1
  #dark = false
  #emulated: boolean | null = null

  constructor(host: DevHost, dark = false) {
    this.#host = host
    this.#dark = dark
  }

  get dark(): boolean {
    return this.#emulated ?? this.#dark
  }

  follow(dark: boolean): void {
    if (this.#dark === dark) return
    this.#dark = dark
    if (this.#emulated === null) this.#host.page()?.post({ type: 'colorscheme', dark })
  }

  take(post: unknown): boolean {
    const route = (post as { devroute?: unknown } | null)?.devroute
    if (typeof route === 'number') {
      this.#used(route)
      return true
    }
    if (!DevLog.isPost(post)) return false
    const page = this.#host.page()
    this.log.take({ name: page?.name ?? '', app: page?.app ?? null }, post as DevPost)
    return true
  }

  gone(name: string): void {
    this.log.gone(name)
  }

  async snapshot(): Promise<unknown> {
    const page = this.#host.page()
    if (!page) return null
    const late = new Promise((later) => setTimeout(() => later(null), DevChannel.STATE_MS))
    return Promise.race([page.call('dev', 'state', []).catch(() => null), late])
  }

  prepare(name: string, app: string | null): { devRoutes: DevRoute[]; devInit: string[] } {
    const mine = DevChannel.#mine(name, app)
    const used = this.#inits.filter((i) => mine(i.web))
    this.#inits = this.#inits.filter((i) => !(i.once && used.includes(i)))
    return { devRoutes: this.#routesFor(name, app), devInit: used.map((i) => i.code) }
  }

  #routesFor(name: string, app: string | null): DevRoute[] {
    const mine = DevChannel.#mine(name, app)
    return this.#routes.filter((r) => mine(r.web))
  }

  static #mine(name: string, app: string | null): (web: string | null) => boolean {
    return (web) => web === null || web === name || (app !== null && web === app)
  }

  request(line: string): Promise<void> {
    const pending = this.#request(line)
    this.#inflight.add(pending)
    void pending.finally(() => this.#inflight.delete(pending))
    return pending
  }

  async settle(ms: number): Promise<void> {
    if (!this.#inflight.size) return
    let timer: ReturnType<typeof setTimeout> | undefined
    const late = new Promise<void>((done) => {
      timer = setTimeout(done, ms)
    })
    await Promise.race([Promise.allSettled([...this.#inflight]), late])
    clearTimeout(timer)
  }

  async #request(line: string): Promise<void> {
    let id: number | null = null
    let ok = true
    let payload: string
    try {
      const request = JSON.parse(line) as Request
      id = request.id
      payload = JSON.stringify(await this.answer(request))
    } catch (error) {
      ok = false
      payload = (error as Error)?.message ?? String(error)
    }
    if (id === null) return
    if (this.#host.respond) this.#host.respond(id, ok, payload)
    else this.#host.shell.devRespond?.(id, ok, payload)
  }

  #guard(op: string, args: Record<string, unknown>): string | null {
    const guard = typeof args['guard'] === 'string' && args['guard'] !== '' ? args['guard'] : null
    if (this.#host.production && guard === null) throw new Error(`${op}: a production session needs a guard (the selector of what only the user may act on)`)
    return guard
  }

  async answer(request: Request): Promise<unknown> {
    const { type: _type, id: _id, op, windows, ...args } = request
    if (this.#host.production && !DevChannel.PRODUCTION.has(op)) {
      throw new Error(`${op}: not available in a production session (${[...DevChannel.PRODUCTION].join(', ')})`)
    }
    if (windows !== undefined) this.#host.place?.(windows)
    const inspect = () => this.#host.inspect().api
    switch (op) {
      case 'webs':
        return { webs: this.#webs() }
      case 'eval':
        return this.#eval(args)
      case 'shot': {
        const page = this.#web(args['web'])
        if (!this.#host.shell.capture) throw new Error('shot: this backend cannot read a window back (host windows only)')
        return this.#host.shell.capture(page.window)
      }
      case 'snapshot':
        return inspect().snapshot(args)
      case 'act': {
        const guard = this.#guard(op, args)
        return inspect().act(String(args['ref'] ?? ''), args['action'] as never, (args['arg'] ?? null) as InspectArg, guard)
      }
      case 'run': {
        const guard = this.#guard(op, args)
        return inspect().run(args['steps'] as never, guard)
      }
      case 'query':
        return inspect().query(args as never)
      case 'input':
        return inspect().input(this.#web(args['web']).window, args['events'] as readonly InspectInput[])
      case 'open':
        throw new Error(`open: ${DevChannel.SINGLE}`)
      case 'init':
        return this.#init(args)
      case 'uninit':
        return this.#uninit(args)
      case 'reload': {
        const page = this.#web(args['web'])
        await this.#host.reload()
        return { web: page.name }
      }
      case 'resize':
        return this.#resize(args)
      case 'dialog': {
        if (args['text'] !== undefined && typeof args['text'] !== 'string') throw new Error('dialog: text is the string a prompt answers')
        const page = this.#web(args['web'])
        const accept = args['accept'] !== false
        page.post({ type: 'devdialog', accept, ...(args['text'] !== undefined && { text: args['text'] }) })
        return { web: page.name, accept }
      }
      case 'clock':
        return this.#web(args['web']).call('dev', 'clock', [DevChannel.#clock(args)])
      case 'console':
        return this.log.console({ ...DevChannel.#query(args), ...(args['level'] !== undefined && { level: args['level'] as ConsoleLevel | 'errors' }) })
      case 'network':
        return this.log.requests({ ...DevChannel.#query(args), failed: args['failed'] === true })
      case 'screen': {
        if (typeof args['path'] !== 'string' || !isAbsolute(args['path'])) throw new Error('screen: path is an absolute file path for the PNG')
        const size = await this.#host.shell.screenshot(args['path'])
        return { ...size, path: args['path'] }
      }
      case 'emulate':
        return this.#emulate(args['colorScheme'])
      case 'choose':
        return this.#choose(args)
      case 'drop':
        return this.#drop(args)
      case 'permissions':
        return { pending: [], decisions: [] }
      case 'devtools':
        if (!this.#host.devtools) throw new Error('devtools: this session serves no DevTools endpoint')
        return this.#host.devtools.port()
      case 'answer':
        throw Object.assign(new Error(`answer: no prompt ${String(args['prompt'])} (a single-app session asks no one)`), { status: 404 })
      case 'revoke': {
        const { app, capability } = args
        if (typeof app !== 'string' || !app || typeof capability !== 'string' || !capability) {
          throw new Error('revoke: app is an app id and capability is the approved capability (as in permissions)')
        }
        return { app, capability, revoked: false }
      }
      case 'route':
        return this.#route(args)
      case 'unroute':
        return this.#unroute(args)
      case 'pointer':
        return this.#pointer(args)
      case 'clipboard':
        return this.#clipboard(args)
    }
    throw new Error(`no op ${String(op)} in a single-app session`)
  }

  #webs(): object[] {
    const page = this.#host.page()
    if (!page) return []
    const { name, app, kind, window, title, lock } = page
    return [{ name, app, kind, window, title, ...(lock && { lock }) }]
  }

  #web(wanted: unknown): DevTarget {
    const page = this.#host.page()
    const name = wanted === undefined || wanted === null ? '' : String(wanted)
    const desktop = name === 'desktop' && page?.kind === 'desktop' && page.name !== DevChannel.LOCK
    const matches = !!page && (name === '' || name === page.name || name === page.app || desktop)
    if (!matches) {
      const known = page ? page.name : 'none'
      throw Object.assign(new Error(`no web ${name} (webs: ${known}; a web name, an app id or desktop)`), { status: 404 })
    }
    return page!
  }

  async #eval(args: Record<string, unknown>): Promise<{ value: unknown }> {
    if (typeof args['code'] !== 'string') throw new Error('eval: code is a string')
    let value: unknown
    try {
      const page = this.#web(args['web'])
      const node = args['node']
      value = node === undefined || node === null
        ? await page.call('debug', 'eval', [args['code']])
        : await page.call('debug', 'evalOn', [Number(node), args['code'], args['arg'] ?? null])
    } catch (error) {
      throw new Error(error instanceof Error ? `${error.name}: ${error.message}` : String(error))
    }
    try {
      return { value: JSON.parse(JSON.stringify(value) ?? 'null') as unknown }
    } catch (error) {
      throw new Error(`eval: the value is not JSON: ${(error as Error).message}`)
    }
  }

  #init(args: Record<string, unknown>): { id: number } {
    if (typeof args['code'] !== 'string' || !args['code']) throw new Error('init: code is the script to run before the page')
    const web = args['web'] === undefined || args['web'] === null ? null : String(args['web'])
    const init = { id: this.#nextInit++, web, code: args['code'], once: args['once'] === true }
    this.#inits.push(init)
    return { id: init.id }
  }

  #uninit(args: Record<string, unknown>): { removed: number; left: number } {
    const id = args['init']
    const before = this.#inits.length
    this.#inits = id === undefined || id === null ? [] : this.#inits.filter((i) => i.id !== id)
    return { removed: before - this.#inits.length, left: this.#inits.length }
  }

  #resize(args: Record<string, unknown>): { width: number; height: number } {
    const page = this.#web(args['web'])
    const width = Number(args['width'])
    const height = Number(args['height'])
    if (!Number.isInteger(width) || !Number.isInteger(height) || width < 1 || height < 1) {
      throw new Error('resize: width and height are whole CSS pixels of the page (without the title bar)')
    }
    if (!this.#host.shell.resizeScreen) throw new Error('resize: only host windows (acdev dev) can change the size of a single app')
    this.#host.shell.resizeScreen(width, height + page.frame)
    return { width, height }
  }

  #emulate(scheme: unknown): { colorScheme: 'light' | 'dark'; emulated: boolean } {
    if (scheme !== undefined) {
      if (scheme !== null && scheme !== 'light' && scheme !== 'dark') {
        throw Object.assign(new Error(`emulate: colorScheme is light, dark or null, not ${JSON.stringify(scheme)}`), { status: 400 })
      }
      this.#emulated = scheme === null ? null : scheme === 'dark'
      this.#host.page()?.post({ type: 'colorscheme', dark: this.dark })
    }
    return { colorScheme: this.dark ? 'dark' : 'light', emulated: this.#emulated !== null }
  }

  #choose(args: Record<string, unknown>): Record<string, unknown> {
    const kinds = (['folder', 'open', 'save'] as const).filter((kind) => args[kind] !== undefined)
    if (kinds.length !== 1) throw new Error('choose: give exactly one of folder, open or save')
    const kind = kinds[0]!
    const answer = args[kind]
    const path = (value: unknown) => typeof value === 'string' && isAbsolute(value)
    const shell = this.#host.shell
    if (!shell.answerFolder || !shell.answerOpen || !shell.answerSave) {
      throw new Error('choose: this backend asks the portal, it cannot be answered beforehand (host windows only)')
    }
    if (kind === 'open') {
      if (answer !== null && !(Array.isArray(answer) && answer.length > 0 && answer.every(path))) {
        throw new Error('choose: open is a non-empty list of absolute file paths, or null to cancel')
      }
      shell.answerOpen(answer as string[] | null)
    } else {
      if (answer !== null && !path(answer)) throw new Error(`choose: ${kind} is an absolute path, or null to cancel`)
      if (kind === 'folder') shell.answerFolder(answer as string | null)
      else shell.answerSave(answer as string | null)
    }
    return { [kind]: answer }
  }

  #drop(args: Record<string, unknown>): Record<string, unknown> {
    const files = args['files']
    if (!Array.isArray(files) || !files.length || !files.every((f) => typeof f === 'string' && isAbsolute(f))) {
      throw new Error('drop: files is a non-empty list of absolute paths')
    }
    const point = ['x', 'y'].map((k) => args[k]).filter((v) => typeof v === 'number' && Number.isFinite(v)) as number[]
    if (point.length === 1 || (point.length === 0 && (args['x'] !== undefined || args['y'] !== undefined))) {
      throw new Error('drop: x and y are page coordinates given together, or leave both out for the middle of the page')
    }
    if (!this.#host.shell.drop) throw new Error('drop: this backend takes drops from the system only (host windows only)')
    const page = this.#web(args['web'])
    this.#host.shell.drop(page.window, files as string[], ...point)
    return { web: page.name, window: page.window, files }
  }

  #route(args: Record<string, unknown>): { id: number } {
    const route = DevRoutes.parse(args, this.#nextRoute++)
    this.#routes.push(route)
    this.#push()
    return { id: route.id }
  }

  #unroute(args: Record<string, unknown>): { removed: number; left: number } {
    const id = args['route']
    if (id !== undefined && id !== null && !Number.isInteger(id)) {
      throw new Error('unroute: route is the id route answered, or leave it out to remove every route')
    }
    const before = this.#routes.length
    this.#routes = id === undefined || id === null ? [] : this.#routes.filter((r) => r.id !== id)
    this.#push()
    return { removed: before - this.#routes.length, left: this.#routes.length }
  }

  #used(id: number): void {
    const route = this.#routes.find((r) => r.id === id)
    if (!route || route.times === null) return
    if (route.times > 1) {
      this.#routes = this.#routes.map((r) => (r.id === id ? { ...r, times: route.times! - 1 } : r))
      return
    }
    this.#routes = this.#routes.filter((r) => r.id !== id)
    this.#push()
  }

  #push(): void {
    const page = this.#host.page()
    if (page) page.post({ type: 'devroutes', routes: this.#routesFor(page.name, page.app) })
  }

  #pointer(args: Record<string, unknown>): { x: number; y: number } {
    const { x, y } = args
    if (typeof x !== 'number' || typeof y !== 'number' || !Number.isFinite(x) || !Number.isFinite(y)) {
      throw new Error('pointer: x and y are numbers in screen coordinates')
    }
    if (!this.#host.shell.inject) throw new Error('pointer: this backend takes no injected input')
    this.#host.shell.inject({ type: 'move', x, y })
    return { x, y }
  }

  #clipboard(args: Record<string, unknown>): { text: string } {
    const shell = this.#host.shell
    if (!shell.readClipboard || !shell.writeClipboard) throw new Error('clipboard: this backend has no clipboard of its own to read (host windows only)')
    if (args['text'] !== undefined) {
      if (typeof args['text'] !== 'string') throw new Error('clipboard: text is a string')
      shell.writeClipboard(args['text'])
    }
    return { text: shell.readClipboard() }
  }

  static #clock(args: Record<string, unknown>): Record<string, unknown> {
    const action = String(args['action'] ?? '')
    const number = (key: string) => {
      const value = args[key]
      if (typeof value !== 'number' || !Number.isFinite(value)) throw new Error(`clock ${action}: ${key} is a number of milliseconds`)
      return value
    }
    switch (action) {
      case 'install':
        return args['time'] === undefined || args['time'] === null ? { action } : { action, time: number('time') }
      case 'fastForward':
      case 'runFor':
        return { action, ms: number('ms') }
      case 'pauseAt':
      case 'setFixedTime':
      case 'setSystemTime':
        return { action, time: number('time') }
      case 'resume':
      case 'state':
        return { action }
    }
    throw new Error(`clock: action is install, fastForward, runFor, pauseAt, resume, setFixedTime, setSystemTime or state, not ${JSON.stringify(action)}`)
  }

  static #query(args: Record<string, unknown>): DevQuery {
    return {
      ...(typeof args['web'] === 'string' && { web: args['web'] }),
      ...(typeof args['since'] === 'number' && { since: args['since'] }),
      ...(typeof args['limit'] === 'number' && { limit: args['limit'] }),
    }
  }
}
