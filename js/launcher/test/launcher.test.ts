import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { bindNatives } from '../../page/src/sys/natives.ts'
import { AppLookup, webInit } from '../src/app/app.ts'
import { LaunchArgs } from '../src/args.ts'
import { route, unsupported } from '../src/app/calls.ts'
import { sessionOf, type Welcome } from '../src/manager/manager.ts'
import { WindowSnapshots } from '../src/app/events.ts'
import { incomingCall, released, ServiceHost, shapeOf, withCallbacks } from '../src/manager/service.ts'

bindNatives({ host: {}, sys: { platform: 'linux', argv: [], pid: 1 } } as never)

const pkg = (apps: unknown[], extra: object = {}) => JSON.stringify({ name: 'demo', acde: { apiLevel: 1, apps, ...extra } })
const app = (id: string, entry = '/index.html') => ({ id, label: id.toUpperCase(), icon: 'apps', entry })

function lookup(files: Record<string, string>): AppLookup {
  return new AppLookup({
    readText: (path) => files[path] ?? null,
    readDir: (dir) => [...new Set(Object.keys(files).filter((p) => p.startsWith(`${dir}/`)).map((p) => p.slice(dir.length + 1).split('/')[0]!))],
  })
}

describe('the single-app launcher', () => {
  test('a directory gives its first app; an id is looked up in ACDE_PATH and the packages under it', () => {
    const apps = lookup({ '/w/hello/package.json': pkg([app('hello'), app('other')]), '/p/x/package.json': pkg([app('notes')]) })
    assert.equal(apps.find('./hello', [], '/w').app.id, 'hello')
    assert.equal(apps.find('notes', ['/p'], '/').dir, '/p/x')
    assert.throws(() => apps.find('missing', ['/p'], '/'), { message: 'no app missing in ACDE_PATH (/p)' })
    assert.throws(() => apps.find('/nowhere', [], '/'), { message: '/nowhere: no acde app in its package.json' })
  })

  test('assets map URL prefixes to package folders, and / falls back to the package itself', () => {
    assert.deepEqual(AppLookup.resources('/p', { assets: { '/img/': ['art'], bad: ['x'] } } as never), [
      { prefix: '/img/', dirs: ['/p/art/'] },
      { prefix: '/', dirs: ['/p/'] },
    ])
  })

  test('the web gets one window for the app, sized by the scale, with no services and the lowest tier', () => {
    const found = lookup({ '/w/package.json': pkg([app('hello')]) }).find('/w', [], '/')
    const navigator = { userAgent: '', platform: 'Linux x86_64' }
    const init = webInit(found, 7n, { width: 800, height: 600, scale: 2, languages: { language: 'en-US' }, navigator })
    assert.deepEqual([init.kind, init.name, init.token, init.window, init.width, init.height], ['window', 'hello#1', 7n, 1, 1600, 1200])
    assert.deepEqual([init.services, init.tier.shell, init.app?.id], [{}, [], 'hello'])
  })

  test('arguments: app with an id or folder, or desktop; the size, scale, seconds and a screenshot; other commands are refused', () => {
    const args = LaunchArgs.parse(['app', 'hello', '--size', '640x480', '--scale', '1.5', '--shot', '/tmp/s.png'])
    assert.deepEqual([args.app, args.width, args.height, args.scale, args.seconds, args.shot], ['hello', 640, 480, 1.5, null, '/tmp/s.png'])
    assert.equal(LaunchArgs.parse(['desktop']).kind, 'desktop')
    assert.throws(() => LaunchArgs.parse(['sessions']), /sessions is not supported/)
    assert.throws(() => LaunchArgs.parse(['app']), /app needs an id or a folder/)
    assert.throws(() => LaunchArgs.parse(['app', 'x', '--size', 'big']), /--size big: WxH/)
  })
})

describe('where the single-app launcher sends a page call', () => {
  const alone = { window: 1, desktop: false, managed: false }
  const managed = { ...alone, managed: true }
  const desktop = { window: 0, desktop: true, managed: true }

  test('without a manager: ready, the title and shown are acknowledged; closing itself closes; lock calls go to the preview; the rest is unsupported', () => {
    assert.equal(route('window', 'ready', [], alone), 'acknowledge')
    assert.equal(route('window', 'close', [], alone), 'close')
    assert.equal(route('windows', 'close', [1], alone), 'close')
    assert.equal(route('windows', 'close', [2], alone), 'unsupported')
    assert.equal(route('apps', 'list', [], alone), 'unsupported')
    assert.match(unsupported('apps', 'list').message, /apps\.list is not supported/)
    assert.equal(route('session', 'logout', [], alone), 'session')
    assert.equal(route('session', 'lock', [], alone), 'session')
    assert.equal(route('lock', 'unlock', ['pw'], alone), 'lock')
    assert.equal(route('desktops', 'list', [], alone), 'session')
  })

  test('with a manager: window state, reload and capabilities stay here, everything else goes to the manager', () => {
    assert.equal(route('window', 'minimize', [], managed), 'minimize')
    assert.equal(route('window', 'reload', [], managed), 'reload')
    assert.equal(route('capabilities', 'get', ['network'], managed), 'capabilities')
    assert.equal(route('window', 'ready', [], managed), 'manager')
    assert.equal(route('page', 'shown', [], managed), 'manager')
    assert.equal(route('apps', 'open', ['notes'], managed), 'manager')
    assert.equal(route('panel', 'open', [], managed), 'manager')
    assert.equal(route('session', 'poweroff', [], managed), 'manager')
    assert.equal(route('desktops', 'choose', ['dopame'], managed), 'manager')
    assert.equal(route('page', 'open', [{}], managed), 'page', 'the file pickers stay in the app process')
    assert.equal(route('page', 'save', [{}], managed), 'page')
    assert.equal(route('page', 'notify', [{}], managed), 'manager', 'a notification goes to the desktop')
  })

  test('the desktop answers panels and windows itself', () => {
    assert.equal(route('panel', 'open', [[0, 0, 10, 10]], desktop), 'panel')
    assert.equal(route('windows', 'close', [3], desktop), 'windows')
    assert.equal(route('apps', 'list', [], desktop), 'manager')
  })
})

describe('the session from the manager\'s welcome', () => {
  const tier = { level: 1, shell: [], refused: {} }
  const base = { api: 1, level: 'system', dir: '/p', resources: [{ prefix: '/', dirs: ['/p/'] }], tier, grants: {} as never, request: null }

  test('an app takes its manifest, tier, grants and request from the welcome', () => {
    const app = { id: 'notes', label: 'Notes', icon: 'apps', entry: '/index.html', capabilities: {} } as never
    const session = sessionOf({ ...base, kind: 'app', app, request: { kind: 'open-file' } } as Welcome, 'app')
    assert.deepEqual([session.kind, session.found?.dir, session.label, session.request], ['app', '/p', 'Notes', { kind: 'open-file' }])
  })

  test('an app keeps the instance number the manager gave it, and only a whole positive one', () => {
    const app = { id: 'files', label: 'Files', icon: 'apps', entry: '/index.html', capabilities: {} } as never
    const second = sessionOf({ ...base, kind: 'app', app, instance: 2 } as Welcome, 'app')
    const odd = sessionOf({ ...base, kind: 'app', app, instance: 0 } as Welcome, 'app')
    assert.deepEqual([second.instance, odd.instance], [2, undefined])
  })

  test('a desktop loads its entry from the package folder; the wrong kind is refused', () => {
    const welcome = { ...base, kind: 'desktop', desktop: { id: 'dopame', name: 'Dopame', entry: 'desktop.html' } } as Welcome
    assert.deepEqual([sessionOf(welcome, 'desktop').page, sessionOf(welcome, 'desktop').found], ['/p/desktop.html', null])
    assert.throws(() => sessionOf(welcome, 'app'), /started this process as desktop, not app/)
  })

  test('the lock page loads the lock entry of the desktop, or its entry when it has none', () => {
    const desktop = { id: 'dopame', name: 'Dopame', entry: 'desktop.html', lock: 'lock.html' }
    const locked = sessionOf({ ...base, kind: 'desktop', lock: true, desktop } as Welcome, 'desktop')
    assert.deepEqual([locked.page, locked.lock], ['/p/lock.html', true])
    const plain = sessionOf({ ...base, kind: 'desktop', lock: true, desktop: { id: 'lock', name: 'Lock', entry: 'index.html' } } as Welcome, 'desktop')
    assert.deepEqual([plain.page, plain.lock], ['/p/index.html', true])
    assert.deepEqual(sessionOf({ ...base, kind: 'desktop', desktop } as Welcome, 'desktop').page, '/p/desktop.html')
  })
})

describe('window system events on their way to the desktop page', () => {
  test('winman snapshots pass as they are', () => {
    const snapshots = new WindowSnapshots()
    const rect = { x: 0, y: 0, width: 10, height: 10 }
    const snapshot = { type: 'window', window: 3, appId: 'notes', title: 'Notes', state: 'normal', hidden: false, rect, active: true, desk: 0, first: true }
    assert.equal(snapshots.toDesktop(snapshot), snapshot)
    const crash = { type: 'windowcrash', window: 3, reason: 'killed' }
    assert.equal(snapshots.toDesktop(crash), crash)
  })

  test('a host window\'s old events become snapshots that say what changed', () => {
    const snapshots = new WindowSnapshots()
    const open = snapshots.toDesktop({ type: 'windowopen', window: 3, title: 'Notes', appId: 'notes', instance: 7, web: true })
    assert.deepEqual(open, {
      type: 'window', window: 3, appId: 'notes', title: 'Notes', state: 'normal', hidden: false, rect: { x: 0, y: 0, width: 0, height: 0 },
      active: false, desk: 0, instance: 7, web: true, first: true,
    })
    const moved = snapshots.toDesktop({ type: 'windowgeometry', window: 3, x: 10, y: 20, width: 300, height: 200 })
    assert.deepEqual([moved['rect'], moved['changed'], moved['first']], [{ x: 10, y: 20, width: 300, height: 200 }, ['rect'], false])
    const again = snapshots.toDesktop({ type: 'windowgeometry', window: 3, x: 10, y: 20, width: 300, height: 200 })
    assert.deepEqual(again['changed'], [], 'the same rect again is no change')
    const filled = snapshots.toDesktop({ type: 'windowstate', window: 3, state: 'filled', previous: 'normal' })
    assert.deepEqual([filled['state'], filled['previous'], filled['changed']], ['filled', 'normal', ['state']])
    const unknown = { type: 'windowtitle', window: 9, title: 'x' }
    assert.equal(snapshots.toDesktop(unknown), unknown)
  })
})

describe('the service mode', () => {
  test('a namespace shape lists methods along the prototype chain, plain values, sync results and whether it is callable', () => {
    class Base {
      greet(): string {
        return 'hi'
      }
    }
    const api = Object.assign(new Base(), { count: 3, label: 'x', nested: { a: 1 }, own: () => 'mine' })
    const shape = shapeOf(api, ['own'])
    assert.deepEqual([...shape.methods].sort(), ['greet', 'own'])
    assert.deepEqual(shape.values, { count: 3, label: 'x' })
    assert.deepEqual(shape.sync, { own: 'mine' })
    assert.equal(shape.callable, false)
    assert.equal(shapeOf(() => 1, []).callable, true)
  })

  test('function markers in the arguments become callbacks, deep in arrays and plain objects', () => {
    const seen: number[] = []
    const args = withCallbacks([{ __acdeFn: 7 }, { on: { __acdeFn: 8 } }, 'text'], (fn) => () => void seen.push(fn)) as [() => void, { on: () => void }, string]
    args[0]()
    args[1].on()
    assert.deepEqual([seen, args[2]], [[7, 8], 'text'])
  })

  test("calls and releases are read from the manager's frames", () => {
    const frame = { type: 'call', id: 4, service: 'desks', method: 'list', args: [1] }
    assert.deepEqual(incomingCall(frame as never), { id: 4, service: 'desks', method: 'list', args: [1] })
    assert.equal(incomingCall({ type: 'services' } as never), null)
    assert.equal(released({ type: 'release', fn: 9 } as never), 9)
    assert.equal(released({ type: 'x', release: { fn: 3 } } as never), 3)
  })
})

describe('a service process stays up while its manager channel is open', () => {
  test('the event loop is held from run() until managerclosed, then let go', async () => {
    const events: { type: string }[][] = [[]]
    let wake: (() => void) | null = null
    const sent: object[] = []
    const manager = {
      welcome: { kind: 'service', name: 'desks' },
      call: async () => undefined,
      onEvents: (callback: () => void) => {
        wake = callback
      },
      pump: () => events.shift() ?? [],
      send: (frame: object) => void sent.push(frame),
    }
    const held: string[] = []
    const host = new ServiceHost(manager as never, manager.welcome as never, {}, () => {
      held.push('held')
      return () => void held.push('released')
    })
    const running = host.run()
    await new Promise((later) => setTimeout(later, 10))
    assert.deepEqual(held, ['held'])
    events.push([{ type: 'managerclosed' }])
    wake!()
    assert.equal(await running, 0)
    assert.deepEqual(held, ['held', 'released'])
  })
})

