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
import { LocalOpener } from '../src/local/open.ts'

const env = { HOME: '/home/dev', XDG_STATE_HOME: '/state', ACDE_STORAGE: '/storage', ACDE_DEV_PLACES: '/docs' }
const made: string[] = []
bindNatives({
  host: { logEnabled: () => false, log: () => {} },
  sys: {
    platform: 'darwin', argv: [], pid: 4242, env: () => env, stderr: () => {}, homedir: () => '/home/dev', mkdir: (path: string) => void made.push(path), chmod: () => {},
    realpath: (path: string) => path.replace(/\/link$/, '/real'),
    lendPaths: (app: string, paths: readonly string[]) => paths.map((path) => ({ name: `${app}:${path}`, label: path.split('/').at(-1)!, kind: path.startsWith('/dirs/') ? 'directory' : 'file', writable: true })),
  },
} as never)

const app = (id: string, extra: object = {}) => ({ id, label: id.toUpperCase(), icon: 'apps', entry: '/index.html', ...extra })
const dev = (path: string[]) => ({ dev: path, system: [], user: null })
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
  apps: [app('settings', { capabilities: { windows: true, network: true, terminal: true } })],
})

const TREE = {
  '/work/desk/package.json': DESK,
  '/work/appa/package.json': pkg('appa', { apps: [app('appa', { requests: ['greet'], services: ['clock'], icon: '/img/a.svg', mimeTypes: ['text/markdown', 'image/*'], capabilities: { fs: { roots: ['documents'] } } })], assets: { '/img/': ['art'] } }),
  '/work/appb/package.json': pkg('appb', { apps: [app('appb', { capabilities: { open: ['appa'], terminal: true, windows: true, fs: { roots: ['documents', 'nowhere'] } } }), app('appa')] }),
}

describe('the local registry of acdev dev', () => {
  test('the desktop\'s package is the system one; apps from every package in ACDE_PATH; a second app with the same id is refused', () => {
    const registry = LocalRegistry.scan(files(TREE, ['/work/appa/art/a.svg']), dev(['/work']), 'desk')
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
    const registry = LocalRegistry.scan(files(TREE, ['/work/appa/art/a.svg']), dev(['/work']), 'desk')
    const listed = registry.listed().find((a) => a.id === 'appa')!
    assert.equal(listed.icon, '/acde-packages/dev-0/appa/art/a.svg')
    assert.deepEqual(registry.packageResources(), [{ prefix: '/acde-packages/dev-0/', dirs: ['/work/'] }])
  })

  test('roots as acdes names them: ACDE_PATH, then XDG_DATA_DIRS/acde/packages, then the user\'s; only the desktop\'s package is a system one', () => {
    const tree = {
      '/usr/share/acde/packages/dopame/package.json': pkg('dopame', { desktops: [{ id: 'dopame', name: 'Dopame', entry: 'i.html' }], apps: [app('files')] }),
      '/home/dev/.local/share/acde/packages/notes/package.json': pkg('notes', { apps: [app('notes')] }),
      '/usr/share/acde/packages/store/package.json': pkg('store', { apps: [app('store')] }),
      '/work/appa/package.json': TREE['/work/appa/package.json'],
    }
    const registry = LocalRegistry.scan(files(tree), { dev: ['/work/appa'], system: ['/usr/local/share', '/usr/share'], user: '/home/dev/.local/share' }, 'dopame')
    assert.deepEqual(registry.roots.map((r) => [r.name, r.dir]), [
      ['dev-0', '/work/appa'], ['system-0', '/usr/local/share/acde/packages'], ['system-1', '/usr/share/acde/packages'], ['user', '/home/dev/.local/share/acde/packages'],
    ])
    assert.deepEqual(registry.apps.map((a) => [a.app.id, a.package.system]), [['appa', false], ['files', true], ['store', false], ['notes', false]])
  })

  test('no desktop id takes the first desktop; an unknown one is a 404', () => {
    assert.equal(LocalRegistry.scan(files(TREE), dev(['/work']), null).desktop.desktop.id, 'desk')
    assert.throws(() => LocalRegistry.scan(files(TREE), dev(['/work']), 'other'), {
      message: 'no desktop other in ACDE_PATH or the package folders (/work): give acdev dev a desktop project (--desktop <folder>; '
        + 'acdev new <name> --template desktop makes one), or install a desktop package (acdev install in its project)',
      status: 404,
    } as never)
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
    this.window = this.seat.shell.grant(this.seat.grant?.() ?? { desktop: true }).window
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
  readonly events: ShellEvent[] = []
  constructor(readonly link: LocalLink, readonly welcome: Welcome, readonly provides: Record<string, unknown>, readonly generation: number) {}

  take(): ShellEvent[] {
    return this.events.splice(0)
  }

  start(): Promise<void> {
    this.link.send({ ready: { provides: this.provides } })
    return Promise.resolve()
  }

  run(): Promise<number> {
    return new Promise((resolve) => {
      this.link.onEvents(() => {
        for (const event of this.link.pump()) {
          this.events.push(event as ShellEvent)
          if (event.type === 'managerclosed') resolve(0)
        }
      })
    })
  }
}

const shape = (methods: string[]) => ({ methods, values: {}, sync: {}, callable: false })
const tick = () => new Promise((resolve) => setTimeout(resolve, 0))

function session() {
  const fake = new FakeShell()
  const published: string[][] = []
  const devtools = { open: () => ({ port: 9222 }), targets: (list: { title: string }[]) => void published.push(list.map((t) => t.title)) }
  const launchers: FakeLauncher[] = []
  const services: FakeService[] = []
  const registry = () => LocalRegistry.scan(files(TREE), dev(['/work']), 'desk')
  const written: Record<string, string> = {}
  const openFiles = {
    readText: (path: string) => written[path] ?? null,
    readDir: () => [],
    isDirectory: (path: string) => path === '/docs',
    exists: () => true,
    writeText: (path: string, text: string) => {
      written[path] = text
    },
  }
  const opener = new LocalOpener(openFiles, { data: [], config: '/cfg', configDirs: [], applications: [] })
  const handedToSystem: string[] = []
  const local = new LocalSession({
    shell: fake.shell,
    devtools,
    opener,
    openDefault: (path) => {
      handedToSystem.push(path)
      return Promise.resolve()
    },
    registry: registry(),
    rescan: registry,
    launcher: (given, link, seat) => {
      const launcher = new FakeLauncher(given, link, seat)
      launchers.push(launcher)
      return launcher
    },
    service: (link, welcome, generation) => {
      const service = new FakeService(link, welcome, welcome.name === 'clock' ? { clock: shape(['now', 'watch']), apps: shape(['x']) } : {}, generation)
      services.push(service)
      return service
    },
  })
  const running = local.run()
  return { fake, launchers, services, local, running, published, handedToSystem }
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
    assert.deepEqual((welcome.grants as unknown as { fs: unknown }).fs, { places: ['desktop', 'documents', 'downloads', 'music', 'pictures', 'videos'], paths: false })
    assert.equal(desktop.window, 1)
    local.end(0)
  })

  test('apps.open starts the app in a free window, tells the desktop, and a second open raises it and hands over the request', async () => {
    const { desktop, launchers, fake, local } = await started()
    const opened = (await desktop.link.call('apps', 'open', ['appa'])) as Record<string, unknown>
    const appa = launchers[1]!
    assert.deepEqual([opened['app'], opened['instance'], opened['window'], opened['pid']], ['appa', 1, appa.window, 4242])
    assert.deepEqual(appa.seat.grant!(), { mode: 'free' })
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

describe('what an app may do in the local session (as acdes checks it)', () => {
  test('a third-party app gets no privileged grant it declares and no file paths; a system app of the desktop gets what it declares', async () => {
    const { desktop, launchers, local } = await started()
    await desktop.link.call('apps', 'open', ['appb'])
    await desktop.link.call('apps', 'open', ['settings'])
    const grants = (i: number) => launchers[i]!.link.welcome.grants as unknown as Record<string, unknown>
    assert.deepEqual([grants(1)['terminal'], grants(1)['fs']], [false, { places: ['documents'], paths: false }])
    assert.deepEqual([grants(2)['terminal'], grants(2)['network'], grants(2)['fs']], [true, true, null])
    local.end(0)
  })

  test('shell modules are for the desktop and the system apps that declare them', async () => {
    const { desktop, launchers, local } = await started()
    await desktop.link.call('apps', 'open', ['appb'])
    const appb = launchers[1]!
    await assert.rejects(appb.link.call('windows', 'screenshot', [{ path: '/tmp/x.png' }]), { status: 403, message: /is not a system app/ } as never)
    await assert.rejects(appb.link.call('session', 'logout', []), { status: 403 } as never)
    await assert.rejects(appb.link.call('permissions', 'list', []), { status: 403 } as never)
    await assert.rejects(appb.link.call('apps', 'openPath', ['/x']), { status: 403, message: /openFiles/ } as never)
    await assert.rejects(appb.link.call('capabilities', 'request', ['camera']), { status: 403 } as never)
    await desktop.link.call('apps', 'open', ['settings'])
    assert.deepEqual(await launchers[2]!.link.call('windows', 'screenshot', [{ path: '/tmp/x.png' }]), { path: '/tmp/x.png', width: 10, height: 10 })
    await assert.rejects(launchers[2]!.link.call('session', 'logout', []), { status: 403, message: /declare "session"/ } as never)
    local.end(0)
  })

  test('an app names only the apps its capabilities.open lists, also with a link; opening by type needs openFiles', async () => {
    const { desktop, launchers, local } = await started()
    await desktop.link.call('apps', 'open', ['appb'])
    const appb = launchers[1]!
    await assert.rejects(appb.link.call('apps', 'open', ['settings', { kind: 'open-link', url: 'https://a.b' }]), { status: 403 } as never)
    await assert.rejects(appb.link.call('apps', 'open', [{ kind: 'open-link', url: 'https://a.b' }]), { status: 403, message: /openFiles/ } as never)
    local.end(0)
  })
})

describe('opening files in the local session (as acdes defaults.rs)', () => {
  test('openPath starts the app whose manifest lists the type, with an open-file request', async () => {
    const { desktop, launchers, local } = await started()
    const opened = (await desktop.link.call('apps', 'openPath', ['/docs/a.md'])) as Record<string, unknown>
    assert.equal(opened['app'], 'appa')
    assert.deepEqual(launchers[1]!.link.welcome.request, {
      kind: 'open-file', file: 'a.md', entry: { name: 'appa:/docs/a.md', label: 'a.md', kind: 'file', writable: true },
    })
    await desktop.link.call('apps', 'openPath', ['/docs/b.png'])
    assert.deepEqual(launchers[1]!.link.pump().filter((e) => e.type === 'request').map((e) => (e['request'] as { file: string }).file), ['b.png'])
    local.end(0)
  })

  test('nobody opens it: the desktop hands it to the system default; an app gets 404 no_app; an app names a program only if it is a system app', async () => {
    const { desktop, launchers, local, handedToSystem } = await started()
    assert.deepEqual(await desktop.link.call('apps', 'openPath', ['/docs/a.pdf']), { kind: 'native', id: null, window: 0, pid: 0 })
    assert.deepEqual(handedToSystem, ['/docs/a.pdf'])
    await desktop.link.call('apps', 'open', ['appb'])
    const appb = launchers[1]!
    await assert.rejects(appb.link.call('apps', 'openPath', ['/docs/a.pdf']), { status: 403, message: /openFiles/ } as never)
    await desktop.link.call('apps', 'open', ['settings'])
    await assert.rejects(launchers[2]!.link.call('apps', 'openPath', ['/docs/a.pdf']), { status: 403 } as never)
    await assert.rejects(desktop.link.call('apps', 'openPath', ['relative.md']), { status: 400 } as never)
    await assert.rejects(desktop.link.call('apps', 'openPath', ['/docs/a.md', { app: 'gedit.desktop' }]), { status: 404 } as never)
    local.end(0)
  })

  test('pages hand over only paths they can reach on the host: the dev places and folders lent to them; the dev channel is not limited', async () => {
    const { desktop, launchers, fake, local } = await started()
    await assert.rejects(desktop.link.call('apps', 'openPath', ['/etc/passwd.md']), { status: 403, message: /outside what this session gives pages/ } as never)
    await desktop.link.call('apps', 'open', ['appa', null, { root: '/dirs/usb' }])
    await assert.rejects(launchers[1]!.link.call('apps', 'openPath', ['/dirs/usb/a.md']), { status: 403, message: /openFiles/ } as never)
    await desktop.link.call('apps', 'open', ['appb'])
    await assert.rejects(launchers[2]!.link.call('apps', 'open', ['appa', { kind: 'open-file', path: '/docs/a.md' }]), { status: 403, message: /openFiles/ } as never)
    fake.emit({ type: 'devrequest', key: JSON.stringify({ id: 9, op: 'open', path: '/elsewhere/n.md' }) })
    for (let i = 0; i < 20 && !fake.responses.some((r) => r[0] === 9); i++) await tick()
    assert.match(fake.responses.find((r) => r[0] === 9)![2], /opened \/elsewhere\/n.md with appa/)
    local.end(0)
  })

  test('the desktop lends a folder: the app gets it as a root with the request, also when running; unlend takes it back', async () => {
    const { desktop, launchers, local } = await started()
    await desktop.link.call('apps', 'open', ['appa', { kind: 'greet' }, { root: { dir: '/dirs/usb', label: 'USB' } }])
    const appa = launchers[1]!
    assert.deepEqual(appa.link.welcome.request, { kind: 'greet', root: 'appa:/dirs/usb' })
    assert.deepEqual(appa.link.pump().filter((e) => e.type === 'lend').map((e) => e['name']), ['appa:/dirs/usb'])
    await desktop.link.call('apps', 'open', ['appa', { kind: 'greet' }, { root: '/dirs/other' }])
    assert.deepEqual(appa.link.pump().map((e) => [e.type, e['name'] ?? (e['request'] as { root?: string })?.root]), [['lend', 'appa:/dirs/other'], ['request', 'appa:/dirs/other']])
    assert.deepEqual(await desktop.link.call('apps', 'unlend', ['/dirs/usb']), { revoked: 1 })
    assert.deepEqual(appa.link.pump().map((e) => [e.type, e['name']]), [['unlend', 'appa:/dirs/usb']])
    await desktop.link.call('apps', 'open', ['appb'])
    await assert.rejects(launchers[2]!.link.call('apps', 'open', ['appa', null, { root: '/dirs/x' }]), { status: 403 } as never)
    await assert.rejects(desktop.link.call('apps', 'open', ['settings', null, { root: '/dirs/x' }]), { status: 409 } as never)
    local.end(0)
  })

  test('an open-file request by type, handlers and setDefault', async () => {
    const { desktop, launchers, local } = await started()
    await desktop.link.call('apps', 'open', [{ kind: 'open-file', path: '/docs/c.md' }])
    assert.equal((launchers[1]!.link.welcome.request as { file: string }).file, 'c.md')
    const listed = (await desktop.link.call('apps', 'handlers', ['/docs/c.md'])) as { mime: string; handlers: { id: string; default: boolean }[] }
    assert.deepEqual([listed.mime, listed.handlers.map((h) => [h.id, h.default])], ['text/markdown', [['appa', true]]])
    assert.equal(await desktop.link.call('apps', 'setDefault', ['text/markdown', 'appa']), 'appa')
    await assert.rejects(desktop.link.call('apps', 'setDefault', ['text/markdown', 'nope']), { status: 404 } as never)
    await assert.rejects(desktop.link.call('apps', 'setDefault', ['markdown', 'appa']), { status: 400 } as never)
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
    const call = clock.take().find((e) => e.type === 'call')!
    assert.deepEqual([call['service'], call['method'], call['caller']], ['clock', 'watch', { kind: 'app', app: 'appa', instance: 1 }])
    const global = ((call['args'] as unknown[])[0] as { __acdeFn: number }).__acdeFn
    clock.link.send({ invoke: { fn: global, args: ['tick'] } })
    assert.deepEqual(appa.link.pump().filter((e) => e.type === 'invoke').map((e) => [e['fn'], e['args']]), [[7, ['tick']]])
    clock.link.send({ id: call['id'], ok: 'watching' })
    assert.equal(await answer, 'watching')
    appa.link.send({ release: { fn: 7 } })
    assert.deepEqual(clock.take().filter((e) => e.type === 'release').map((e) => e['fn']), [global])
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

  test('an app window that goes away is remembered where it was, and a new grant for it (a reload) asks for that place', async () => {
    const { desktop, launchers, fake, local } = await started()
    await desktop.link.call('apps', 'open', ['appa'])
    const appa = launchers[1]!
    desktop.seen.length = 0
    fake.emit({ type: 'windowgone', window: appa.window, x: 100, y: 120, width: 640, height: 480 })
    assert.deepEqual(appa.seat.grant!(), { mode: 'free', rect: [100, 120, 640, 480] })
    assert.deepEqual(desktop.seen.map((e) => e.type), [])
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

  test('a rebuilt desktop: the services end and start again from a fresh module, then the desktop reloads', async () => {
    const { desktop, services, fake, local } = await started()
    const clock = services[0]!
    clock.take()
    fake.emit({ type: 'devreload' })
    for (let i = 0; i < 20 && services.length < 4; i++) await tick()
    assert.deepEqual(services.map((s) => [s.welcome.name, s.generation]), [['clock', 0], ['notes', 0], ['clock', 1], ['notes', 1]])
    for (let i = 0; i < 20 && !desktop.seen.some((e) => e.type === 'devreload'); i++) await tick()
    assert.ok(desktop.seen.some((e) => e.type === 'devreload'))
    await desktop.link.call('apps', 'open', ['appa'])
    const answer = desktop.link.call('clock', 'now', [])
    assert.ok(services[2]!.take().some((e) => e.type === 'call'))
    assert.deepEqual(services[0]!.take().filter((e) => e.type === 'call'), [])
    void answer.catch(() => null)
    local.end(0)
  })

  test('the one DevTools endpoint lists every page: each launcher publishes its own targets and the session merges them', async () => {
    const { desktop, launchers, published, local } = await started()
    await desktop.link.call('apps', 'open', ['appa'])
    const target = (title: string) => ({ id: title, kind: 'page' as const, title, url: '' })
    desktop.seat.devtools!.targets([target('desk')])
    launchers[1]!.seat.devtools!.targets([target('appa')])
    assert.deepEqual(published.at(-1), ['desk', 'appa'])
    launchers[1]!.end(0)
    await tick()
    assert.deepEqual(published.at(-1), ['desk'])
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
