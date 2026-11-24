import { API_LEVEL } from '@acde/api/manifest'
import type { WebGrants } from '../../page/src/sys/natives'
import { sys } from '../../page/src/sys/natives'
import { readDirOr, readTextOr } from '../../page/src/sys/fs'
import { xdgHome } from '../../page/src/sys/os'
import { join } from '../../page/src/sys/path'
import { process } from '../../page/src/sys/process'
import { AppLookup, DesktopLookup, type Found } from './app/app'
import type { LaunchArgs } from './args'
import type { Session } from './manager/manager'

function filesFor(found: Found): WebGrants['fs'] {
  const fs = found.app.capabilities.fs
  if (!fs) return null
  const declared = typeof fs === 'object' ? [...fs.roots] : []
  const places = process.env['ACDE_DEV_PLACES'] ? declared : []
  return { places, paths: false }
}

function grantsFor(found: Found): WebGrants {
  const { app } = found
  const root = process.env['ACDE_STORAGE'] ?? join(xdgHome('data'), 'acde', 'storage')
  return {
    app: app.id, fs: filesFor(found), terminal: false, network: !!app.capabilities.network, system: false, bluetooth: false, audio: !!app.capabilities.audio,
    devices: false, power: false, input: false, display: false, office: false, camera: false, microphone: false,
    storage: { key: `app-${app.id}`, dir: root ? join(root, `app-${app.id}`) : null, legacy: null },
  }
}

export function standalone(args: LaunchArgs): Session {
  if (args.kind === 'lock') return lockPreview(args)
  if (args.kind !== 'app') throw new Error(`${args.kind} needs the session manager (ACDE_MANAGER_FD); without it only app <id|dir> and lock <desktop> run`)
  const path = (process.env['ACDE_PATH'] ?? '').split(':').filter(Boolean)
  const found = new AppLookup({ readText: (file) => readTextOr(file, null), readDir: readDirOr }).find(args.app, path, sys().cwd())
  const { app } = found
  const grants = grantsFor(found)
  return {
    kind: 'app', found, page: null, resources: found.resources, label: app.label, heapMb: app.heapMb,
    tier: null, grants, request: null, services: {}, own: false, lock: false, confirm: false, greeter: false,
  }
}

function lockPreview(args: LaunchArgs): Session {
  const path = (process.env['ACDE_PATH'] ?? '').split(':').filter(Boolean)
  const found = new DesktopLookup({ readText: (file) => readTextOr(file, null), readDir: readDirOr }).find(args.app, path, sys().cwd())
  const entry = found.desktop.lock
  if (!entry) throw new Error(`desktop ${found.desktop.id} has no lock page (lock in its package.json)`)
  const grants: WebGrants = {
    app: null, fs: null, terminal: false, network: false, system: false, bluetooth: false, audio: false, devices: false, power: false,
    input: false, display: false, office: false, camera: false, microphone: false, storage: { key: 'lock', dir: null, legacy: null },
  }
  return {
    kind: 'desktop', found: null, page: join(found.dir, entry), resources: found.resources, label: `${found.desktop.name} (lock)`,
    heapMb: found.desktop.heapMb, tier: { level: API_LEVEL, shell: [], refused: {} }, grants, request: null, services: {},
    own: false, lock: true, confirm: false, greeter: false,
  }
}
