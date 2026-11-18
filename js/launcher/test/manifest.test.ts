import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { API_LEVEL, Manifest, type PackageManifest } from '@dopame/acde-api/manifest'

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
