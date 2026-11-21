import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { API_LEVEL, MIN_API_LEVEL, Manifest, type PackageManifest } from '@acde/api/manifest'

const files = new Set(['desktop/index.html', 'desktop/lock.html'])

function errors(desktop: Record<string, unknown>, isFile: ((path: string) => boolean) | undefined = (path) => files.has(path)): string[] {
  const manifest = { apiLevel: API_LEVEL, desktops: [{ id: 'dopame', name: 'Dopame', ...desktop }] } as unknown as PackageManifest
  return Manifest.check(manifest, () => true, () => true, isFile).errors
}

describe('the lock entry of a desktop', () => {
  test('is optional, and checked as the entry is: a path inside the package that is there', () => {
    assert.deepEqual(errors({ entry: 'desktop/index.html' }), [])
    assert.deepEqual(errors({ entry: 'desktop/index.html', lock: 'desktop/lock.html' }), [])
    assert.deepEqual(errors({ entry: 'desktop/index.html', lock: 'desktop/missing.html' }), ['desktop "dopame": no lock desktop/missing.html'])
    assert.deepEqual(errors({ entry: 'desktop/gone.html' }), ['desktop "dopame": no entry desktop/gone.html'])
  })

  test('outside the package or not a string is refused, with or without a file check', () => {
    for (const lock of ['/etc/lock.html', '../lock.html', 'desktop/../../lock.html', '', 3]) {
      const found = errors({ entry: 'desktop/index.html', lock }, undefined)
      assert.equal(found.length, 1, String(lock))
      assert.match(found[0]!, /desktop "dopame": lock must be a path inside the package/)
    }
  })
})

describe('the backend of a desktop', () => {
  const entry = { entry: 'desktop/index.html' }

  test('is a systemd unit template ending in @, without a slash', () => {
    assert.deepEqual(errors({ ...entry, backend: { unit: 'acdes-session-agent@' } }), [])
    for (const backend of [{ unit: 'acdes-session-agent' }, { unit: 'a/b@' }, { unit: '@' }, { unit: 3 }, 'acdes-session-agent@', []]) {
      const found = errors({ ...entry, backend })
      assert.equal(found.length, 1, JSON.stringify(backend))
      assert.match(found[0]!, /desktop "dopame": backend is \{unit\}/)
    }
  })

  test('a service role is a name, and a role without a desktop backend is a warning', () => {
    const manifest = (desktop: Record<string, unknown>) => {
      const desktops = [{ id: 'dopame', name: 'Dopame', ...entry, ...desktop }]
      return { apiLevel: API_LEVEL, services: [{ name: 'assistant' }], desktops } as unknown as PackageManifest
    }
    const check = (desktop: Record<string, unknown>) =>
      Manifest.check(manifest(desktop), () => true, () => true, (path) => files.has(path))
    assert.deepEqual(check({ backend: { unit: 'acdes-session-agent@' }, services: [{ name: 'assistant', backend: 'agent' }] }), { errors: [], warnings: [] })
    assert.deepEqual(check({ services: [{ name: 'assistant', backend: 'agent' }] }).warnings, [
      'desktop "dopame": service "assistant" has a backend role, but the desktop has no backend',
    ])
    assert.match(check({ services: [{ name: 'assistant', backend: '' }] }).errors[0]!, /service "assistant": backend is the name of its role/)
    assert.deepEqual(Manifest.desktopServices([{ name: 'assistant', backend: 'agent' }, 'files']), [
      { name: 'assistant', modules: [], backend: 'agent' },
      { name: 'files', modules: [] },
    ])
  })
})

describe('the contract levels a runtime takes', () => {
  test('every level from the lowest supported to the current one, and no other', () => {
    assert.ok(MIN_API_LEVEL >= 1 && MIN_API_LEVEL <= API_LEVEL)
    for (let level = MIN_API_LEVEL; level <= API_LEVEL; level++) {
      assert.equal(Manifest.supports(level), true)
      assert.equal(Manifest.refusal(level), null)
    }
    assert.equal(Manifest.supports(API_LEVEL + 1), false)
    assert.equal(Manifest.supports(MIN_API_LEVEL - 1), false)
    const runtime = MIN_API_LEVEL === API_LEVEL ? `is level ${API_LEVEL}` : `supports levels ${MIN_API_LEVEL}–${API_LEVEL}`
    assert.equal(Manifest.refusal(API_LEVEL + 1), `needs level ${API_LEVEL + 1}, runtime ${runtime}`)
  })
})
