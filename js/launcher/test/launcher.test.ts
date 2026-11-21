import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { bindNatives } from '../../page/src/sys/natives.ts'
import { AppLookup, webInit } from '../src/app/app.ts'
import { LaunchArgs } from '../src/args.ts'
import { route, unsupported } from '../src/app/calls.ts'
import { nativeArgs, rejectingCalls, sessionOf, windowGrant, windowSize, withoutUndefined, type Welcome } from '../src/manager/manager.ts'
import { WindowSnapshots } from '../src/app/events.ts'
import { backendUp, Callbacks, incomingCall, released, ServiceHost, shapeOf, withCallbacks } from '../src/manager/service.ts'

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

function serviceManager(welcome: Record<string, unknown>, connectBackend?: () => Promise<unknown>) {
  const events: { type: string; [field: string]: unknown }[][] = [[]]
  const box: { wake: (() => void) | null } = { wake: null }
  const manager = {
    welcome: { kind: 'service', name: 'assistant', ...welcome },
    call: async () => undefined,
    onEvents: (callback: () => void) => {
      box.wake = callback
    },
    pump: () => events.shift() ?? [],
    send: () => undefined,
    ...(connectBackend ? { connectBackend } : {}),
  }
  const host = new ServiceHost(manager as never, manager.welcome as never, {}, () => () => undefined)
  const push = (frames: { type: string; [field: string]: unknown }[]) => {
    events.push(frames)
    box.wake!()
  }
  return { host, push }
}

describe('the desktop backend reaches the service', () => {
  test('a service without a backend role has no backend', () => {
    assert.equal(serviceManager({}).host.backend, null)
  })

  test('up starts from the welcome, backend frames change it, and onChange hears only changes', async () => {
    const { host, push } = serviceManager({ backend: { role: 'agent', up: false } })
    const backend = host.backend!.api
    assert.equal(backend.role, 'agent')
    assert.equal(backend.up, false)
    const seen: boolean[] = []
    const stop = backend.onChange(({ up }) => void seen.push(up))
    const running = host.run()
    push([{ type: 'backend', up: true }, { type: 'backend', up: true }, { type: 'backend', up: false }])
    await new Promise((later) => setTimeout(later, 10))
    assert.deepEqual(seen, [true, false])
    assert.equal(stop(), true)
    push([{ type: 'backend', up: true }, { type: 'managerclosed' }])
    assert.equal(await running, 0)
    assert.deepEqual(seen, [true, false])
    assert.equal(backend.up, true)
  })

  test('connect goes to the manager, and fails as unsupported when the manager cannot', async () => {
    const link = { role: 'agent' }
    const { host } = serviceManager({ backend: { role: 'agent', up: true } }, async () => link)
    assert.equal(await host.backend!.api.connect(), link)
    const old = serviceManager({ backend: { role: 'agent', up: true } }).host
    await assert.rejects(old.backend!.api.connect(), { status: 501, code: 'unsupported' })
  })

  test('only backend frames carry up', () => {
    assert.equal(backendUp({ type: 'backend', up: true } as never), true)
    assert.equal(backendUp({ type: 'backend', up: 'yes' } as never), false)
    assert.equal(backendUp({ type: 'backend' } as never), false)
    assert.equal(backendUp({ type: 'release', up: true } as never), undefined)
  })
})

describe('a service hears when a caller lets go of a callback', () => {
  test('releaseSignal gives one AbortSignal per callback number; release aborts it and silences the callback', () => {
    const callbacks = new Callbacks()
    const sent: unknown[][] = []
    const first = callbacks.make(7, (args) => void sent.push(args))
    const again = callbacks.make(7, (args) => void sent.push(args))
    const other = callbacks.make(8, () => {})
    const signal = callbacks.signal(first)
    assert.equal(callbacks.signal(again), signal)
    assert.equal(signal.aborted, false)
    let heard = 0
    signal.addEventListener('abort', () => heard++)
    first('a')
    callbacks.release(7)
    first('b')
    assert.deepEqual(sent, [['a']])
    assert.equal(heard, 1)
    assert.equal(signal.aborted, true)
    assert.equal(callbacks.signal(again).aborted, true)
    assert.equal(callbacks.signal(other).aborted, false)
    assert.throws(() => callbacks.signal(() => {}), TypeError)
    assert.throws(() => callbacks.signal('x'), /not a callback a caller passed/)
  })
})

describe('a call the manager refuses at once', () => {
  test('becomes a rejected promise; the rest of the link is passed through as it is', async () => {
    const sent: object[] = []
    const link = {
      welcome: { kind: 'app' },
      call: (service: string, method: string, args?: readonly unknown[]) => {
        if (method === 'boom') throw new TypeError('undefined cannot be represented as a serde_json::Value')
        return Promise.resolve(`${service}.${method}(${JSON.stringify(args)})`)
      },
      onEvents: () => {},
      pump: () => [{ type: 'x' }],
      send: (frame: object) => void sent.push(frame),
    }
    const calls = rejectingCalls(link as never)
    const refused = calls.call('settings', 'boom', ['id'])
    assert.ok(refused instanceof Promise)
    await assert.rejects(refused, /serde_json::Value/)
    assert.equal(await calls.call('settings', 'enable', ['id', undefined]), 'settings.enable(["id"])')
    const message = 'lock.unlock: argument 1 is undefined; leave it out or pass null'
    await assert.rejects(calls.call('lock', 'unlock', [undefined, {}]), { name: 'TypeError', message })
    assert.equal(calls.welcome, link.welcome)
    assert.deepEqual(calls.pump(), [{ type: 'x' }])
    calls.send!({ ready: true })
    assert.deepEqual(sent, [{ ready: true }])
    assert.equal(calls.connectBackend, undefined)
  })
})

describe('arguments on their way to a native call', () => {
  test('trailing undefined ones are left out and undefined fields of plain objects dropped, at any depth', () => {
    assert.deepEqual(nativeArgs('s', 'm', ['a', undefined, undefined]), ['a'])
    assert.deepEqual(nativeArgs('s', 'm', [undefined]), [])
    assert.deepEqual(nativeArgs('s', 'm'), [])
    assert.deepEqual(nativeArgs('s', 'm', [{ a: 1, b: undefined, c: { d: undefined, e: [{ f: undefined, g: null }] } }]), [{ a: 1, c: { e: [{ g: null }] } }])
    const bytes = new Uint8Array([1])
    assert.equal(nativeArgs('s', 'm', [bytes])[0], bytes)
  })

  test('an undefined in the middle, or in an array, is a TypeError naming the call and where', () => {
    const message = 'lock.unlock: argument 2 is undefined; leave it out or pass null'
    assert.throws(() => nativeArgs('lock', 'unlock', ['pw', undefined, 1]), { name: 'TypeError', message })
    assert.throws(() => nativeArgs('s', 'm', [{ list: [1, undefined] }]), /s\.m: argument 1\.list\[1\] is undefined/)
  })

  test('a service answer is cleaned the same way, and an undefined it cannot drop names the call', () => {
    const answer = () => 'desk.list: the answer'
    assert.deepEqual(withoutUndefined({ desks: [{ id: 'a', note: undefined }], more: undefined }, answer), { desks: [{ id: 'a' }] })
    assert.throws(() => withoutUndefined({ desks: [undefined] }, answer), /desk\.list: the answer\.desks\[0\] is undefined/)
  })
})

describe('the window size the manager asks for', () => {
  test('two whole numbers from 1 to 16384, rounded; anything else is no size, and the manifest size stands', () => {
    assert.deepEqual(windowSize([640.4, 480.6]), [640, 481])
    assert.deepEqual(windowSize([1, 16384]), [1, 16384])
    for (const bad of [undefined, null, [640], [640, 480, 1], [0, 480], [640, 16385], ['640', 480], [Number.NaN, 480], { width: 640, height: 480 }]) {
      assert.equal(windowSize(bad), null, JSON.stringify(bad))
    }
  })

  test('the welcome outer size or size comes first, then the manifest size, else none (the window system default)', () => {
    assert.deepEqual(windowGrant({ outerSize: [700, 532] }, [400, 300]), { outerSize: [700, 532] })
    assert.deepEqual(windowGrant({ size: [640, 480] }, [400, 300]), { size: [640, 480] })
    assert.deepEqual(windowGrant({ outerSize: [0, 1] as never, size: [640, 480] }, null), { size: [640, 480] })
    assert.deepEqual(windowGrant(undefined, [400, 300]), { size: [400, 300] })
    assert.deepEqual(windowGrant({}, null), {})
  })
})

describe('a desktop page that starts late or reloads', () => {
  test('replay gives every open window as a first snapshot, closed and crashed ones left out', () => {
    const snapshots = new WindowSnapshots()
    const rect = { x: 0, y: 0, width: 10, height: 10 }
    const base = { type: 'window', appId: 'notes', title: 'Notes', state: 'normal', hidden: false, rect, active: false, desk: 0, key: 'w-1' }
    snapshots.toDesktop({ ...base, window: 3, first: true })
    snapshots.toDesktop({ ...base, window: 3, title: 'Draft', state: 'filled', previous: 'normal', first: false, changed: ['title', 'state'] })
    snapshots.toDesktop({ ...base, window: 4, first: true })
    snapshots.toDesktop({ ...base, window: 5, first: true })
    snapshots.toDesktop({ type: 'windowclose', window: 4 })
    snapshots.toDesktop({ type: 'windowcrash', window: 5, reason: 'killed' })
    assert.deepEqual(snapshots.replay(), [{ ...base, window: 3, title: 'Draft', state: 'filled', first: true }])
    assert.deepEqual(['window', 'windowclose', 'windowcrash', 'currentdesk'].map((type) => WindowSnapshots.FOLDED.has(type)), [true, true, true, false])
  })
})
