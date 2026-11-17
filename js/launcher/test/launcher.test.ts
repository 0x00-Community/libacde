import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { bindNatives } from '../../page/src/sys/natives.ts'
import { AppLookup, webInit } from '../src/app.ts'
import { LaunchArgs } from '../src/args.ts'
import { answer, unsupported } from '../src/calls.ts'

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

  test('arguments: app with an id or folder, the size, scale, seconds and a screenshot; other commands are refused', () => {
    const args = LaunchArgs.parse(['app', 'hello', '--size', '640x480', '--scale', '1.5', '--shot', '/tmp/s.png'])
    assert.deepEqual([args.app, args.width, args.height, args.scale, args.seconds, args.shot], ['hello', 640, 480, 1.5, null, '/tmp/s.png'])
    assert.throws(() => LaunchArgs.parse(['desktop', 'dopame']), /desktop is not supported/)
    assert.throws(() => LaunchArgs.parse(['app', 'x', '--size', 'big']), /--size big: WxH/)
  })
})

describe('the page calls the single-app launcher answers', () => {
  test('ready, the title and shown are acknowledged; the window closing itself, or by its own id, closes it', () => {
    assert.equal(answer('window', 'ready', [], 1), 'acknowledge')
    assert.equal(answer('window', 'close', [], 1), 'close')
    assert.equal(answer('windows', 'close', [1], 1), 'close')
    assert.equal(answer('windows', 'close', [2], 1), 'unsupported')
    assert.equal(answer('apps', 'list', [], 1), 'unsupported')
    assert.match(unsupported('apps', 'list').message, /apps\.list is not supported/)
  })
})
