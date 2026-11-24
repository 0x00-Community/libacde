import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { SHELL_MODULES } from '@acde/api/manifest'
import { bindNatives } from '../../page/src/sys/natives.ts'
import type { ShellEvent } from '../src/app/events.ts'
import type { Session, Welcome } from '../src/manager/manager.ts'
import type { Grant, GrantOptions, LauncherShell } from '../src/shell.ts'
import { ShellHub } from '../src/local/hub.ts'
import type { LocalLink } from '../src/local/link.ts'
import { LocalRegistry, type LocalFiles } from '../src/local/registry.ts'
import { LocalSession, type Seat, type SeatedLauncher, type ServiceRunner } from '../src/local/session.ts'

const env = { HOME: '/home/dev', XDG_STATE_HOME: '/state', ACDE_STORAGE: '/storage' }
const made: string[] = []
bindNatives({
  host: { logEnabled: () => false, log: () => {} },
  sys: { platform: 'darwin', argv: [], pid: 4242, env: () => env, stderr: () => {}, homedir: () => '/home/dev', mkdir: (path: string) => void made.push(path), chmod: () => {} },
} as never)

const app = (id: string, extra: object = {}) => ({ id, label: id.toUpperCase(), icon: 'apps', entry: '/index.html', ...extra })
const pkg = (name: string, acde: object) => JSON.stringify({ name, acde: { apiLevel: 1, ...acde } })

function files(given: Record<string, string>, extra: readonly string[] = []): LocalFiles {
  return {
    readText: (path) => given[path] ?? null,
    readDir: (dir) => [...new Set(Object.keys(given).filter((p) => p.startsWith(`${dir}/`)).map((p) => p.slice(dir.length + 1).split('/')[0]!))],
    isFile: (path) => path in given || extra.includes(path),
  }
}

const DESK = pkg('desk', {
  desktops: [{ id: 'desk', name: 'Desk', entry: 'index.html', services: ['clock', { name: 'notes', modules: ['windows'] }, 'missing'] }],
  services: [{ name: 'clock', module: 'services/clock.js' }, { name: 'notes', module: 'services/notes.js' }],
  apps: [app('settings', { capabilities: { windows: true, network: true } })],
})

const TREE = {
  '/work/desk/package.json': DESK,
  '/work/appa/package.json': pkg('appa', { apps: [app('appa', { requests: ['greet'], services: ['clock'], icon: '/img/a.svg' })], assets: { '/img/': ['art'] } }),
  '/work/appb/package.json': pkg('appb', { apps: [app('appb', { capabilities: { open: ['appa'] } }), app('appa')] }),
}

describe('the local registry of acdev dev', () => {
  test('the desktop\'s package is the system one; apps from every package in ACDE_PATH; a second app with the same id is refused', () => {
    const registry = LocalRegistry.scan(files(TREE, ['/work/appa/art/a.svg']), ['/work'], 'desk')
    assert.equal(registry.desktop.desktop.id, 'desk')
    assert.deepEqual(new Map(registry.apps.map((a) => [a.app.id, a.package.system])), new Map([['appa', false], ['appb', false], ['settings', true]]))
    assert.ok(registry.refused.some((line) => line.includes('app appa is already in /work/appa')))
    assert.ok(registry.refused.some((line) => line.includes('lists service missing')))
    assert.deepEqual(registry.desktop.services.map((s) => [s.grant.name, s.grant.modules, s.module]), [
      ['clock', [], '/work/desk/services/clock.js'],
      ['notes', ['windows'], '/work/desk/services/notes.js'],
    ])
  })

  test('an icon file of a package is listed under /acde-packages/<root>/, which the desktop gets as a resource', () => {
    const registry = LocalRegistry.scan(files(TREE, ['/work/appa/art/a.svg']), ['/work'], 'desk')
    const listed = registry.listed().find((a) => a.id === 'appa')!
    assert.equal(listed.icon, '/acde-packages/dev-0/appa/art/a.svg')
    assert.deepEqual(registry.packageResources(), [{ prefix: '/acde-packages/dev-0/', dirs: ['/work/'] }])
  })

  test('no desktop id takes the first desktop; an unknown one is a 404', () => {
    assert.equal(LocalRegistry.scan(files(TREE), ['/work'], null).desktop.desktop.id, 'desk')
    assert.throws(() => LocalRegistry.scan(files(TREE), ['/work'], 'other'), { message: 'no desktop other in ACDE_PATH (/work)', status: 404 } as never)
  })
})

class FakeShell {
  readonly events: ShellEvent[] = []
  readonly responses: [number, boolean, string][] = []
  readonly released: number[] = []
  readonly focused: number[] = []
  #wake: (() => void) | null = null
  #next = 1 << 20
  rows: { id: number; window: number; focus: boolean }[] = []

  get shell(): LauncherShell {
    return {
      pump: () => this.events.splice(0),
      onEvents: (callback: () => void) => {
        this.#wake = callback
      },
      grant: (options: GrantOptions): Grant => {
        const window = options.desktop ? 1 : this.#next++
        this.rows.push({ id: window, window, focus: false })
        return { window, token: BigInt(window) }
      },
      release: (window: number) => {
        this.released.push(window)
        this.rows = this.rows.filter((r) => r.window !== window)
      },
      focus: (window: number) => void this.focused.push(window),
      windows: () => this.rows as never,
      devRespond: (id: number, ok: boolean, payload: string) => void this.responses.push([id, ok, payload]),
      screenshot: () => Promise.resolve({ width: 10, height: 10 }),
      close: () => {},
    } as unknown as LauncherShell
  }

  emit(event: ShellEvent): void {
    this.events.push(event)
    this.#wake?.()
  }
}

class FakeLauncher implements SeatedLauncher {
  window = 0
  readonly seen: ShellEvent[] = []
  #end: ((code: number) => void) | null = null
  readonly ended: Promise<number>

  constructor(readonly session: Session, readonly link: LocalLink, readonly seat: Seat) {
    this.ended = new Promise((resolve) => {
      this.#end = resolve
    })
    seat.shell.onEvents(() => this.seen.push(...seat.shell.pump()))
  }

  start(): void {
    this.window = this.seat.shell.grant(this.seat.grant ?? { desktop: true }).window
  }

  run(): Promise<number> {
    return this.ended
  }

  end(code = 0): void {
    this.#end?.(code)
  }

  close(): Promise<boolean> {
    this.end(0)
    return Promise.resolve(true)
  }

  devAnswer(request: Record<string, unknown>): Promise<unknown> {
    if (request['op'] !== 'webs') return Promise.resolve({ op: request['op'] })
    const name = this.session.kind === 'desktop' ? 'desktop' : `${this.session.found!.app.id}#${this.session.instance}`
    return Promise.resolve({ webs: [{ name, app: this.session.found?.app.id ?? null, kind: this.session.kind, window: this.window, title: this.session.label }] })
  }

  page() {
    return { window: this.window, rpc: null, record: { kind: 'web' as const, app: this.session.found?.app.id ?? null, title: this.session.label } }
  }
}

class FakeService implements ServiceRunner {
  #end: (() => void) | null = null
  constructor(readonly link: LocalLink, readonly welcome: Welcome, readonly provides: Record<string, unknown>) {}

  start(): Promise<void> {
    this.link.send({ ready: { provides: this.provides } })
    return Promise.resolve()
  }

  run(): Promise<number> {
    return new Promise((resolve) => {
      this.#end = () => resolve(0)
    })
  }
}

const shape = (methods: string[]) => ({ methods, values: {}, sync: {}, callable: false })
const tick = () => new Promise((resolve) => setTimeout(resolve, 0))

function session() {
  const fake = new FakeShell()
  const launchers: FakeLauncher[] = []
  const services: FakeService[] = []
  const registry = () => LocalRegistry.scan(files(TREE), ['/work'], 'desk')
  const local = new LocalSession({
    shell: fake.shell,
    registry: registry(),
    rescan: registry,
    launcher: (given, link, seat) => {
      const launcher = new FakeLauncher(given, link, seat)
      launchers.push(launcher)
      return launcher
    },
    service: (link, welcome) => {
      const service = new FakeService(link, welcome, welcome.name === 'clock' ? { clock: shape(['now', 'watch']), apps: shape(['x']) } : {})
      services.push(service)
      return service
    },
  })
  const running = local.run()
  return { fake, launchers, services, local, running }
}

async function started() {
  const s = session()
  for (let i = 0; i < 20 && s.launchers.length === 0; i++) await tick()
  const desktop = s.launchers[0]!
  return { ...s, desktop }
}

describe('the local session of acdev dev: a desktop and its apps in one host window', () => {
  test('services start first; the desktop gets every shell module, the services that are ready, and the package roots', async () => {
    const { desktop, local, services } = await started()
    assert.deepEqual(services.map((s) => [s.welcome.kind, s.welcome.name, s.welcome.modules]), [['service', 'clock', []], ['service', 'notes', ['windows']]])
    const welcome = desktop.link.welcome
    assert.deepEqual([welcome.kind, welcome.level, welcome.tier.shell], ['desktop', 'desktop', [...SHELL_MODULES]])
    assert.deepEqual(Object.keys(welcome.services ?? {}), ['clock'])
    assert.ok(welcome.resources.some((r) => r.prefix === '/acde-packages/dev-0/'))
    assert.equal((welcome.grants as unknown as { fs: unknown }).fs, true)
    assert.equal(desktop.window, 1)
    local.end(0)
  })

  test('apps.open starts the app in a free window, tells the desktop, and a second open raises it and hands over the request', async () => {
    const { desktop, launchers, fake, local } = await started()
    const opened = (await desktop.link.call('apps', 'open', ['appa'])) as Record<string, unknown>
    const appa = launchers[1]!
    assert.deepEqual([opened['app'], opened['instance'], opened['window'], opened['pid']], ['appa', 1, appa.window, 4242])
    assert.deepEqual(appa.seat.grant, { mode: 'free' })
    assert.deepEqual([appa.link.welcome.kind, appa.link.welcome.level, appa.link.welcome.tier.shell], ['app', 'third-party', []])
    const told = desktop.link.pump()
    assert.ok(told.some((e) => e.type === 'windowopen' && e['window'] === appa.window && e['appId'] === 'appa' && e['web'] === 'appa#1'))
    const again = (await desktop.link.call('apps', 'open', ['appa', { kind: 'greet', name: 'hi' }])) as Record<string, unknown>
    assert.equal(again['running'], true)
    assert.equal(launchers.length, 2)
    assert.deepEqual(appa.link.pump().filter((e) => e.type === 'request').map((e) => e['request']), [{ kind: 'greet', name: 'hi' }])
    assert.deepEqual(fake.focused, [appa.window])
    local.end(0)
  })

  test('a request needs a kind the app accepts; apps open others only as their capabilities.open allows', async () => {
    const { desktop, launchers, local } = await started()
    await assert.rejects(desktop.link.call('apps', 'open', ['appa', { name: 'x' }]), { status: 400, code: 'untyped_request' } as never)
    await assert.rejects(desktop.link.call('apps', 'open', ['appa', { kind: 'other' }]), { status: 409, code: 'not_accepted' } as never)
    await assert.rejects(desktop.link.call('apps', 'open', ['nope']), { status: 404 } as never)
    await desktop.link.call('apps', 'open', ['appb'])
    const appb = launchers[1]!
    await appb.link.call('apps', 'open', ['appa'])
    await assert.rejects(appb.link.call('apps', 'open', ['settings']), { status: 403 } as never)
    local.end(0)
  })

  test('the system apps of the desktop\'s package get the shell modules they declare', async () => {
    const { desktop, launchers, local } = await started()
    await desktop.link.call('apps', 'open', ['settings'])
    const welcome = launchers[1]!.link.welcome
    assert.deepEqual([welcome.level, welcome.tier.shell], ['system', ['windows']])
    local.end(0)
  })

  test('when an app ends its window is released and the desktop hears windowclose; the next start is instance 2', async () => {
    const { desktop, launchers, fake, local } = await started()
    await desktop.link.call('apps', 'open', ['appa'])
    const appa = launchers[1]!
    desktop.link.pump()
    appa.end(0)
    await tick()
    assert.deepEqual(fake.released, [appa.window])
    assert.ok(desktop.link.pump().some((e) => e.type === 'windowclose' && e['window'] === appa.window))
    const next = (await desktop.link.call('apps', 'open', ['appa'])) as Record<string, unknown>
    assert.equal(next['instance'], 2)
    local.end(0)
  })

  test('window titles and menus of apps reach the desktop as windowtitle and windowmenus', async () => {
    const { desktop, launchers, local } = await started()
    await desktop.link.call('apps', 'open', ['appa'])
    const appa = launchers[1]!
    desktop.link.pump()
    await appa.link.call('window', 'setTitle', ['Draft'])
    await appa.link.call('window', 'setMenus', [[{ id: 'file', label: 'File', items: [] }]])
    const told = desktop.link.pump()
    assert.deepEqual(told.map((e) => e.type), ['windowtitle', 'windowmenus'])
    assert.equal(told[0]!['title'], 'Draft')
    await desktop.link.call('windows', 'invokeMenu', [{ window: appa.window }, 'file', 'open'])
    assert.deepEqual(appa.link.pump().filter((e) => e.type === 'menuinvoke').map((e) => [e['menu'], e['item']]), [['file', 'open']])
    local.end(0)
  })
})

describe('desktop services in the local session', () => {
  test('a call goes to the providing service with its callbacks renumbered, and the answer comes back', async () => {
    const { desktop, launchers, services, local } = await started()
    await desktop.link.call('apps', 'open', ['appa'])
    const appa = launchers[1]!
    const clock = services[0]!
    const answer = appa.link.call('clock', 'watch', [{ __acdeFn: 7 }])
    const call = clock.link.pump().find((e) => e.type === 'call')!
    assert.deepEqual([call['service'], call['method'], call['caller']], ['clock', 'watch', { kind: 'app', app: 'appa', instance: 1 }])
    const global = ((call['args'] as unknown[])[0] as { __acdeFn: number }).__acdeFn
    clock.link.send({ invoke: { fn: global, args: ['tick'] } })
    assert.deepEqual(appa.link.pump().filter((e) => e.type === 'invoke').map((e) => [e['fn'], e['args']]), [[7, ['tick']]])
    clock.link.send({ id: call['id'], ok: 'watching' })
    assert.equal(await answer, 'watching')
    appa.link.send({ release: { fn: 7 } })
    assert.deepEqual(clock.link.pump().filter((e) => e.type === 'release').map((e) => e['fn']), [global])
    local.end(0)
  })

  test('a reserved name a service provides is dropped; an app that does not list the service is refused', async () => {
    const { desktop, launchers, local } = await started()
    assert.equal(Object.hasOwn(desktop.link.welcome.services ?? {}, 'apps'), false)
    await desktop.link.call('apps', 'open', ['appb'])
    await assert.rejects(launchers[1]!.link.call('clock', 'now', []), { status: 403, message: /list that service's name in the services of APPB/ } as never)
    await assert.rejects(launchers[1]!.link.call('weather', 'now', []), { status: 403 } as never)
    local.end(0)
  })

  test('a service calls only the shell modules its grant lists', async () => {
    const { services, local } = await started()
    const notes = services[1]!
    await assert.rejects(notes.link.call('packages', 'refresh', []), { status: 403 } as never)
    await assert.rejects(notes.link.call('windows', 'focus', [1]), { status: 501 } as never)
    assert.ok(Array.isArray(await notes.link.call('apps', 'list', [])))
    local.end(0)
  })
})

describe('host window events and dev requests in the local session', () => {
  test('per-window events go to the window\'s owner; the rest to the desktop; app shortcuts to the app', async () => {
    const { desktop, launchers, fake, local } = await started()
    await desktop.link.call('apps', 'open', ['appa'])
    const appa = launchers[1]!
    fake.emit({ type: 'windowcloserequest', window: appa.window })
    fake.emit({ type: 'shortcut', window: appa.window, key: 'mod+s', title: 'app' })
    fake.emit({ type: 'windowactivate', window: appa.window })
    fake.emit({ type: 'shortcut', window: 1, key: 'mod+space', title: 'desktop' })
    assert.deepEqual(appa.seen.map((e) => e.type), ['windowcloserequest', 'shortcut'])
    assert.deepEqual(desktop.seen.map((e) => e.type), ['windowactivate', 'shortcut'])
    local.end(0)
  })

  test('webs answers for every page; a request naming a web goes to that page; reload of one app goes to it', async () => {
    const { desktop, launchers, fake, local } = await started()
    await desktop.link.call('apps', 'open', ['appa'])
    const appa = launchers[1]!
    fake.emit({ type: 'devrequest', key: JSON.stringify({ id: 3, op: 'webs' }) })
    await tick()
    const [id, ok, payload] = fake.responses[0]!
    assert.deepEqual([id, ok], [3, true])
    assert.deepEqual((JSON.parse(payload) as { webs: { name: string }[] }).webs.map((w) => w.name), ['desktop', 'appa#1'])
    fake.emit({ type: 'devrequest', key: JSON.stringify({ id: 4, op: 'eval', web: 'appa', code: '1' }) })
    await tick()
    assert.deepEqual(appa.seen.filter((e) => e.type === 'devrequest').map((e) => JSON.parse(String(e['key'])).id), [4])
    fake.emit({ type: 'devrequest', key: JSON.stringify({ id: 5, op: 'eval', web: 'zzz', code: '1' }) })
    await tick()
    assert.equal(fake.responses.find((r) => r[0] === 5)?.[1], false)
    fake.emit({ type: 'devreload', key: 'appa' })
    assert.ok(appa.seen.some((e) => e.type === 'devreload'))
    fake.emit({ type: 'devrequest', key: JSON.stringify({ id: 6, op: 'open', name: 'APPB' }) })
    await tick()
    assert.match(fake.responses.find((r) => r[0] === 6)![2], /opened appb/)
    local.end(0)
  })

  test('quit from acdev ends the session; ending closes the apps', async () => {
    const { desktop, launchers, fake, running } = await started()
    await desktop.link.call('apps', 'open', ['appa'])
    fake.emit({ type: 'quit' })
    assert.equal(await running, 0)
    assert.equal(await launchers[1]!.ended, 0)
  })
})

test('ShellHub: a view owns the windows it was granted until it releases them', () => {
  const fake = new FakeShell()
  const routed: [string, boolean][] = []
  const hub = new ShellHub(fake.shell, (event, owner) => routed.push([event.type, owner !== null]))
  const view = hub.view()
  const { window } = view.shell.grant({ mode: 'free' })
  assert.equal(hub.owner(window), view)
  fake.emit({ type: 'windowcloserequest', window })
  view.shell.release!(window)
  assert.equal(hub.owner(window), null)
  fake.emit({ type: 'windowcloserequest', window })
  assert.deepEqual(routed, [['windowcloserequest', true], ['windowcloserequest', false]])
})
