import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { DesktopLookup } from '../src/app/app.ts'
import { route } from '../src/app/calls.ts'
import { FakeLock } from '../src/app/lock.ts'
import { LaunchArgs } from '../src/args.ts'

const failure = (call: () => unknown) => {
  try {
    call()
  } catch (error) {
    const { code, status, retryAfter } = error as { code?: string; status?: number; retryAfter?: number }
    return { code, status, retryAfter }
  }
  return null
}

describe('the lock page preview of acdev dev', () => {
  test('lock <desktop> parses; without a desktop it says what it needs', () => {
    assert.equal(LaunchArgs.parse(['lock', 'dopame']).kind, 'lock')
    assert.equal(LaunchArgs.parse(['lock', 'dopame']).app, 'dopame')
    assert.throws(() => LaunchArgs.parse(['lock']), /lock needs a desktop id or a folder/)
  })

  test('the stand-in acde:lock answers as the lock page checks expect', () => {
    const lock = new FakeLock()
    assert.deepEqual(lock.answer('user', []), { name: 'dev', fullName: 'acdev developer', avatar: null })
    assert.equal(lock.answer('unlock', ['right-password']), undefined)
    assert.deepEqual(failure(() => lock.answer('unlock', ['wrong-password'])), { code: 'auth', status: 403, retryAfter: undefined })
    assert.deepEqual(failure(() => lock.answer('unlock', ['busy-password'])), { code: 'busy', status: 409, retryAfter: undefined })
    assert.deepEqual(failure(() => lock.answer('unlock', ['throttled-password'])), { code: 'throttled', status: 429, retryAfter: 3 })
    assert.deepEqual(failure(() => lock.answer('unlock', ['anything else'])), { code: 'auth', status: 403, retryAfter: undefined })
    assert.equal(lock.answer('unlock', ['right-password', { remember: 'round' }]), undefined)
    assert.equal(lock.answer('cancel', []), undefined)
    assert.deepEqual(failure(() => lock.answer('other', [])), { code: 'not_found', status: 404, retryAfter: undefined })
  })

  test('without a manager lock calls stay in the launcher; with one they go to the manager', () => {
    assert.equal(route('lock', 'unlock', ['x'], { window: 0, desktop: true, managed: false }), 'lock')
    assert.equal(route('lock', 'unlock', ['x'], { window: 0, desktop: true, managed: true }), 'manager')
  })

  test('the lock page is found by desktop id in ACDE_PATH or by folder', () => {
    const pkg = JSON.stringify({ acde: { apiLevel: 1, apps: [], desktops: [{ id: 'desk', name: 'Desk', entry: 'index.html', lock: 'lock.html' }] } })
    const files = { readText: (path: string) => (path === '/pkgs/desk/package.json' ? pkg : null), readDir: (dir: string) => (dir === '/pkgs' ? ['desk'] : []) }
    const lookup = new DesktopLookup(files)
    assert.equal(lookup.find('desk', ['/pkgs'], '/').desktop.lock, 'lock.html')
    assert.equal(lookup.find('/pkgs/desk', [], '/').dir, '/pkgs/desk')
    assert.throws(() => lookup.find('other', ['/pkgs'], '/'), /no desktop other in ACDE_PATH/)
  })
})
