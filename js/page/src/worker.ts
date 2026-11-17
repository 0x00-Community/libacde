import { install, installBackendTransport, observe } from '@dopame/acde-dom'
import type { AudioNative, BackendTransport, CaptureNative, DeviceNative, DomNative, GpuNative, HostNative, StorageNative, WebglNative } from '@dopame/acde-dom'
import type { WebInit } from './contract'
import { BluetoothModule, type BluetoothNatives } from './bluetooth'
import { DevicesModule, type DevicesNatives } from './devices'
import { Frame } from './frame'
import { Watched, type WatchNatives } from './watched'
import { FsModule } from './fsmodule'
import { Host } from './host'
import type { ImeEvent, NativeIme } from './ime'
import type { NativeSurface } from './native'
import { DevHot } from './devhot'
import { DevToolsPage } from './devtools'
import { CssDomain } from './devtools/css'
import { DomDomain } from './devtools/dom'
import { PageDom } from './devtools/pagedom'
import { NetworkDomain } from './devtools/network'
import { PageDomain } from './devtools/page'
import { OverlayDomain } from './devtools/overlay'
import { DevRoutes, type DevRoute } from './devroutes'
import { DevState, type DevSnapshot } from './devstate'
import { Level, Log } from './log'
import { PageSource } from './page'
import { PageCalls, type FolderPicker } from './pagecalls'
import { PageInspect } from './pageinspect'
import { Pinned } from './pinned'
import { RemoteBridge, WindowList, WindowSelf } from './remote'
import { Permits } from './broker/access/approval/permits'
import type { Approval } from './broker/access/approval/permissions'
import type { CapabilityStatus } from '@dopame/acde-api/manifest'
import { Rpc } from './rpc'
import { OfficePage } from './office'
import { readFileSync, readTextOr } from './sys/fs'
import { bindNatives, type Natives } from './sys/natives'
import { process } from './sys/process'
import { StartupTimeline } from './timeline'
import { WebAppPage } from './webapp'
import { HostStrings } from './language'
import { SourceMaps } from './devlog'

interface WebNatives extends Natives {
  readonly host: HostNative & {
    onTaskEnd(fn: () => void): void
    setReporter(fn: (error: unknown, promise?: Promise<unknown>) => void): void
    requestFrame(): void
    readSourceMap?(url: string): Uint8Array | null
  }
  readonly port: { post(message: unknown): void; onMessage(fn: (message: unknown) => void): void }
  readonly init: WebInit
  readonly env: Record<string, string>
  createDocument(
    width: number,
    height: number,
    scale: number,
    options: { resources: { prefix: string; dirs: string[] }[]; dark: boolean },
  ): DomNative
  claimSurface(token: bigint, options?: { frame?: number }): NativeSurface
  readonly Ime?: new (callback: (event: ImeEvent) => void) => NativeIme
  readonly gpu?: GpuNative
  readonly webgl?: WebglNative
  readonly audio?: AudioNative
  readonly capture?: CaptureNative
  readonly device?: DeviceNative
  readonly storage?: StorageNative
  readonly backend?: Omit<BackendTransport, 'port'>
  readonly fs?: Parameters<typeof FsModule.typed>[0]
  readonly terminal?: object
  readonly network?: object
  readonly system?: object
  readonly bluetooth?: BluetoothNatives
  readonly sound?: WatchNatives
  readonly devices?: DevicesNatives
  readonly power?: WatchNatives
  readonly input?: WatchNatives
  readonly display?: WatchNatives
  readonly office?: object
  readonly picker?: FolderPicker
}

export default async function boot(natives: WebNatives): Promise<void> {
  bindNatives(natives)
  const { host, port, init } = natives
  Host.label = init.name

  let language = init.languages?.language
  const look = { resources: init.resources, dark: init.dark === true }
  const dom = natives.createDocument(init.width, init.height, init.scale, look)
  const surface = natives.claimSurface(init.token, { frame: init.kind === 'window' ? Frame.height(init.app) : 0 })
  const title = (text: string) => surface.setTitle(text || (init.app?.label ?? init.name))
  title('')
  host.requestFrame = () => surface.requestFrame()

  const htmlEntry = init.kind === 'window' && !!init.app && PageSource.isHtml(init.app.entry)
  if (htmlEntry) {
    const read = host.readAsset.bind(host)
    const entry = `${PageSource.ORIGIN}${init.app!.entry}`
    host.readAsset = (url) => read(url) ?? (/^http:\/\/acde\.local\/(?:[?#].*)?$/.test(url) ? read(entry) : null)
  }
  const maps = new SourceMaps(
    (url) => host.readAsset(url),
    (url) => host.readSourceMap?.(url) ?? null,
  )
  const report = host.reportError.bind(host)
  host.reportError = (error) => {
    const stack = (error as { stack?: unknown } | null)?.stack
    const mapped = typeof stack === 'string' ? maps.stack(stack) : null
    if (!mapped || mapped === stack) return report(error)
    const copy = new Error(String((error as { message?: unknown }).message ?? ''))
    Object.defineProperty(copy, 'stack', { value: mapped })
    report(copy, error)
  }
  const permits = new Permits(init.tier?.approvals, init.app?.label ?? init.kind)
  const rpc = new Rpc((message) => port.post(message))
  const access = {
    supported: (name: string) => init.tier?.capabilities?.statuses.find((s) => s.name === name)?.state !== 'unsupported',
    request: (name: string) => rpc.call('capabilities', 'request', [name]) as Promise<CapabilityStatus>,
  }
  const page = install(globalThis, dom, host, {
    url: (htmlEntry ? init.url : undefined) ?? `${PageSource.ORIGIN}/`,
    ...init.languages,
    ...init.navigator,
    ...(natives.storage && { storage: natives.storage }),
    gpu: natives.gpu,
    webgl: natives.webgl,
    audio: natives.audio,
    ...(natives.capture && { capture: permits.capture(natives.capture, access) }),
    ...(natives.device && { device: permits.device(natives.device, access) }),
    ...(init.screen && { screen: init.screen }),
  })
  const routes = natives.env['ACDE_DEV'] ? new DevRoutes(init.devRoutes ?? [], (id) => port.post({ devroute: id })) : null
  const dialogs: { accept: boolean; text?: string }[] = []
  let network: NetworkDomain | null = null
  if (routes) {
    observe({
      get network() {
        return network?.observer ?? null
      },
      dialog: (event) => {
        const answer = dialogs.shift() ?? null
        const accept = answer ? answer.accept : event.type === 'alert'
        const text = event.type === 'prompt' && accept ? (answer?.text ?? event.defaultValue) : null
        port.post({ devlog: 'dialog', time: Log.clock(), dialog: { type: event.type, message: event.message, defaultValue: event.defaultValue, answered: !!answer, accept, text } })
        return answer
      },
      console: (level, text) => port.post({ devlog: 'console', time: Log.clock(), level, text: maps.stack(text) }),
      error: (kind, text) => port.post({ devlog: 'error', time: Log.clock(), kind, text: maps.stack(text) }),
      request: (event) => port.post({ devlog: 'request', time: Log.clock(), event }),
      route: (_kind, method, url) => routes.match(method, url),
    })
  }
  host.onTaskEnd(() => page.endTask())
  host.setReporter((error, promise) => {
    if (promise) page.unhandledRejection(promise, error)
    else page.reportError(error)
  })

  const output = WindowSelf.output(init.screen?.width ?? 0, init.screen?.height ?? 0, init.scale, init.screen?.workArea)
  const self = init.kind === 'window' && init.app ? new WindowSelf(rpc, surface, globalThis as never, init.request ?? null, output) : null
  const windows = new WindowList(rpc, init.windows)
  const bridge = new RemoteBridge(rpc, init, self, windows, permits)
  if (natives.backend) installBackendTransport({ ...natives.backend, port: init.kind === 'window' && init.app?.backend ? init.app.backend.port : null })
  const web = new Host(page, dom, init.scale, natives.Ime, windows)
  const debug = !!natives.env['ACDE_DEBUG_DIR'] || !!natives.env['ACDE_DEV']
  const inspect = new PageInspect(page, dom, web, surface.contentOffset)
  if (natives.env['ACDE_DEV']) {
    const href = () => String((page.window as { location?: { href?: unknown } }).location?.href ?? '')
    const source = PageDom.source(page, dom)
    DevToolsPage.install(host, (devtools) => {
      network = new NetworkDomain(devtools, href)
      return [
        PageDomain.create(devtools.frameId, href),
        DomDomain.create(source, (session, method, params) => devtools.emitTo(session, 'DOM', method, params)),
        CssDomain.create(source),
        OverlayDomain.create(PageDom.overlay(page, dom.highlight?.bind(dom))),
        network.domain(),
      ]
    })
  }
  rpc.serve(async (service, method, args) => {
    if (service === 'window' && self) return self.serve(method, args)
    if (service === 'inspect') return inspect.serve(method, args)
    if (service === 'dev' && method === 'state' && natives.env['ACDE_DEV']) return DevState.capture(page.window as never)
    if (service === 'dev' && method === 'clock' && natives.env['ACDE_DEV']) return await page.clock(args[0] as never)
    if (service === 'debug' && method === 'eval' && debug) return await (0, eval)(String(args[0]))
    if (service === 'debug' && method === 'evalOn' && debug) {
      const element = page.nodeOf(Number(args[0]))
      if (!element) throw new Error(`no element ${String(args[0])} in this page (it is gone)`)
      const fn = (0, eval)(`(${String(args[1])})`) as (element: unknown, arg: unknown) => unknown
      if (typeof fn !== 'function') throw new Error('evalOn: code is a function of the element')
      return await fn(element, args[2])
    }
    if (service === 'debug' && method === 'stats' && debug) return dom.stats(Boolean(args[0]))
    throw new Error(`${init.name} serves no ${service}.${method}`)
  })
  let devhot = (): void => {}
  port.onMessage((message) => {
    if (rpc.receive(message)) return
    const m = message as {
      type?: string
      dark?: boolean
      reduced?: boolean
      language?: string
      languages?: string[]
      width?: number
      height?: number
      scale?: number
      workArea?: { x: number; y: number; width: number; height: number }
      routes?: DevRoute[]
      capability?: string
      approval?: Approval
      status?: CapabilityStatus
    }
    switch (m.type) {
      case 'devroutes':
        return routes?.set(m.routes ?? [])
      case 'devdialog': {
        const d = m as { accept?: unknown; text?: unknown }
        if (routes) dialogs.push({ accept: d.accept !== false, ...(typeof d.text === 'string' && { text: d.text }) })
        return
      }
      case 'screen':
        if (m.width && m.height) page.setScreen({ width: m.width, height: m.height })
        if (m.width && m.height) self?.moved(WindowSelf.output(m.width, m.height, m.scale ?? self.output.scale, m.workArea))
        return
      case 'languages':
        language = m.language
        return page.setLanguages({ language: m.language, languages: m.languages })
      case 'colorscheme':
        dom.setDark?.(m.dark === true)
        return page.mediaChanged()
      case 'devhot':
        return devhot()
      case 'permit':
        return m.capability && m.approval ? bridge.permit(m.capability, m.approval, m.status ?? null) : undefined
      default:
        web.deliver([m] as never)
    }
  })
  if (self) {
    self.emit('activate')
    self.onVisible = (visible) => page.setVisible(visible)
  }
  const privateDirectory = natives.storage?.privateDirectory ? () => natives.storage!.privateDirectory!() : null
  const notify = access.supported('notifications')
  const setup = { picker: natives.picker ?? null, label: init.app?.label ?? null, language: () => language, privateDirectory, notify }
  const calls = new PageCalls(page, rpc, init.name, setup)
  web.handOver = calls
  bridge.handOver = calls
  if (self) self.handOver = calls
  web.onActivation = (active) => self?.emit(active ? 'activate' : 'deactivate')
  web.onGrab = (event) => self?.emit(event.kind === 'lost' ? 'grablost' : 'grab', event)
  web.onState = (state, previous) => self?.changed(state, previous)
  web.onFrameButton = (key) => {
    if (!self) return
    if (key === 'close') void self.api.close()
    else if (key === 'minimize') void self.api.minimize()
    else if (key === 'fullscreen' || key === 'restore') self.api.requestState(key === 'fullscreen' ? 'fullscreen' : 'normal')
  }
  if (init.kind === 'desktop') {
    web.onFirstFrame = () => void rpc.call('page', 'shown').catch(() => undefined)
    const pinned = new Pinned(rpc, surface, dom, page as never)
    web.onPanelMove = (id, extent) => pinned.extend(id, extent)
    bridge.value['desktop'] = { ...(bridge.value['desktop'] as object), pin: (element: Element, options?: { margin?: number }) => pinned.pin(element, options) }
  }
  if (bridge.value['desktop']) bridge.value['desktop'] = { ...(bridge.value['desktop'] as object), ...Frame.desktop(surface) }
  if (natives.fs) {
    const typed = FsModule.typed(natives.fs)
    bridge.value['fs'] = { ...typed, roots: async () => HostStrings.roots(await (typed as unknown as { roots(): Promise<unknown> }).roots(), language) }
  }
  if (natives.terminal) bridge.value['terminal'] = natives.terminal
  if (natives.network) bridge.value['network'] = natives.network
  if (natives.system) bridge.value['system'] = natives.system
  if (natives.bluetooth) bridge.value['bluetooth'] = BluetoothModule.wrap(natives.bluetooth)
  if (natives.sound) bridge.value['audio'] = Watched.module('audio', natives.sound)
  if (natives.devices) bridge.value['devices'] = DevicesModule.wrap(natives.devices)
  if (natives.power) bridge.value['power'] = Watched.typed('power', natives.power, ['changed', 'prompt', 'critical'])
  if (natives.input) bridge.value['input'] = Watched.module('input', natives.input)
  if (natives.display) bridge.value['display'] = Watched.module('display', natives.display)
  if (natives.office) {
    const editorPage = async (setup: { api: string; config: unknown }, theme: 'light' | 'dark') => OfficePage.write(setup, theme)
    const office = natives.office as { state(...args: unknown[]): Promise<unknown>; open(path: string, writable: boolean): Promise<unknown> }
    const state = (root?: unknown, ...rest: unknown[]) => {
      if (root === undefined || root === null || typeof root === 'string') return office.state(root, ...rest)
      const held = calls.held(root)
      if (!held || held.kind !== 'file') return Promise.reject(new TypeError('acde:office state: not a file handle this page was given'))
      return office.open(held.path, held.writable)
    }
    const labelled = async (...args: Parameters<typeof state>) => {
      const result = (await state(...args)) as { roots?: unknown } | null
      return result && typeof result === 'object' && 'roots' in result ? { ...result, roots: HostStrings.roots(result.roots, language) } : result
    }
    bridge.value['office'] = { ...natives.office, state: labelled, editorPage }
  }
  bridge.define(host)
  const app = self ? new WebAppPage(page, rpc, init.name) : null
  StartupTimeline.mark('page load started')
  const htmlPage = htmlEntry ? PageSource.htmlFor(init.app!.entry, init.url, init.resources) : null
  const served = htmlEntry && PageSource.isServer(init.app!.entry)
  const html =
    init.kind === 'desktop'
      ? init.standalone
        ? standalone(init.standalone)
        : readFileSync(init.page!, 'utf8')
      : served
        ? await PageSource.served(page.window as never, init.url ?? `${PageSource.ORIGIN}/`)
        : htmlPage
          ? PageSource.html(htmlPage, init.resources)
          : PageSource.window(init.app!, init.theme ?? '')
  if (natives.env['ACDE_DEV']) {
    for (const code of init.devInit ?? []) {
      try {
        ;(0, eval)(code)
      } catch (error) {
        page.reportError(error)
      }
    }
  }
  await page.loadPage(html)
  if (init.kind === 'window' && Frame.height(init.app) > 0) Frame.followScheme(page.window as never, surface, (run) => void setTimeout(run, 50))
  if (init.devState) {
    DevState.restore(page.window as never, init.devState as DevSnapshot, (restored, missed) =>
      Log.write(Level.Info, 'acde_host', `[${init.name}] acde dev: state restored (${restored} kept${missed ? `, ${missed} not found` : ''})`),
    )
  }
  const reread = !natives.env['ACDE_DEV']
    ? null
    : init.kind === 'desktop'
      ? init.standalone
        ? null
        : () => readFileSync(init.page!, 'utf8')
      : !served && htmlPage
        ? () => PageSource.html(htmlPage, init.resources)
        : null
  const parser = (page.window as never as { DOMParser: ConstructorParameters<typeof DevHot>[1] }).DOMParser
  const hot = reread
    ? new DevHot(page.window.document as never, parser, page.window.location.href, {
        read: (src) => {
          const path = new URL(src, PageSource.ORIGIN).pathname
          const file = PageSource.file(path, init.resources ?? [])
          return file ? readTextOr(file, null) : null
        },
        liveEdit: (url, source) => (host.liveEdit ? host.liveEdit(url, source) : 'no live edit here'),
      }, html)
    : null
  devhot = () => {
    let why: string | null = 'no page source to reread'
    try {
      if (hot && reread) why = hot.apply(reread())
    } catch (error) {
      why = (error as Error).message
    }
    Log.write(Level.Info, 'acde_host', `[${init.name}] acde dev: ${why === null ? 'hot swapped in place' : `reloading (${why})`}`)
    if (why !== null) void rpc.call('page', 'devreload').catch(() => undefined)
  }
  title(page.window.document.title ?? '')
  if (self) self.onTitle = title
  if (app) app.onTitle = title
  app?.loaded(htmlEntry)
  StartupTimeline.mark(init.kind === 'desktop' ? 'desktop loaded' : 'loaded')
  await web.run(surface)
  web.close()
  process.exit(0)
}

function standalone(app: string): string {
  return `<!doctype html><html lang="zh-Hans"><head><meta charset="utf-8"><title>${PageSource.escape(app)}</title><style>
html, body { margin: 0; width: 100%; height: 100%; overflow: hidden; background: #2b2733; }
</style></head><body></body></html>`
}
