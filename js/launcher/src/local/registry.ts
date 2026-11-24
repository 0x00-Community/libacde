import { Manifest, type DesktopServiceGrant, type PackageManifest } from '@acde/api/manifest'
import type { DesktopManifest } from '@acde/api'
import { AppLookup, type Found } from '../app/app'
import { join, normalize } from '../../../page/src/sys/path'

export const PACKAGES_PREFIX = '/acde-packages/'

export interface LocalFiles {
  readText(path: string): string | null
  readDir(path: string): string[]
  isFile(path: string): boolean
}

export interface LocalPackage {
  readonly dir: string
  readonly name: string
  readonly manifest: PackageManifest
  readonly resources: Found['resources']
  readonly system: boolean
}

export interface LocalApp extends Found {
  readonly package: LocalPackage
}

export interface LocalService {
  readonly grant: DesktopServiceGrant
  readonly module: string
}

export interface LocalDesktop {
  readonly package: LocalPackage
  readonly desktop: DesktopManifest
  readonly services: readonly LocalService[]
}

export interface PackageRoot {
  readonly name: string
  readonly dir: string
  readonly system: boolean
}

export interface RootDirs {
  readonly dev: readonly string[]
  readonly system: readonly string[]
  readonly user: string | null
}

export interface ListedApp {
  readonly id: string
  readonly label: string
  readonly icon: string
  readonly script: string | null
}

function percentEncoded(segment: string): string {
  return [...new globalThis.TextEncoder().encode(segment)]
    .map((b) => (/[A-Za-z0-9\-._~]/.test(String.fromCharCode(b)) ? String.fromCharCode(b) : `%${b.toString(16).toUpperCase().padStart(2, '0')}`))
    .join('')
}

export class LocalRegistry {
  readonly apps: readonly LocalApp[]
  readonly refused: readonly string[]

  constructor(
    readonly roots: readonly PackageRoot[],
    readonly desktop: LocalDesktop,
    apps: readonly LocalApp[],
    refused: readonly string[],
    readonly files: LocalFiles,
  ) {
    this.apps = apps
    this.refused = refused
  }

  static roots(given: RootDirs): PackageRoot[] {
    const roots: PackageRoot[] = []
    const add = (dir: string, name: (n: number) => string, system: boolean) => {
      const at = normalize(dir)
      if (roots.some((r) => r.dir === at)) return
      const count = roots.filter((r) => r.name.startsWith(name(0).replace(/0$/, ''))).length
      roots.push({ name: name(count), dir: at, system })
    }
    for (const dir of given.dev) add(dir, (n) => `dev-${n}`, false)
    for (const dir of given.system) add(join(dir, 'acde', 'packages'), (n) => `system-${n}`, true)
    if (given.user) add(join(given.user, 'acde', 'packages'), (n) => (n === 0 ? 'user' : `user-${n}`), false)
    return roots
  }

  static scan(files: LocalFiles, given: RootDirs, desktopId: string | null): LocalRegistry {
    const roots = LocalRegistry.roots(given)
    const dirs: { dir: string; system: boolean }[] = []
    for (const root of roots) {
      const candidates = files.readText(join(root.dir, 'package.json')) !== null
        ? [root.dir]
        : files.readDir(root.dir).sort().map((name) => join(root.dir, name)).filter((dir) => files.readText(join(dir, 'package.json')) !== null)
      for (const dir of candidates) if (!dirs.some((d) => d.dir === dir)) dirs.push({ dir, system: root.system })
    }
    const read = dirs.map(({ dir, system }) => {
      const found = LocalRegistry.#read(files, dir)
      return found && { ...found, system }
    }).filter((p): p is { dir: string; name: string; manifest: PackageManifest; system: boolean } => p !== null)
    const owner = read.find((p) => (p.manifest.desktops ?? []).some((d) => desktopId === null || d.id === desktopId))
    if (!owner) {
      const where = roots.map((r) => r.dir).join(':') || 'none'
      throw Object.assign(new Error(`no desktop ${desktopId ?? ''} in ACDE_PATH or the package folders (${where})`), { status: 404 })
    }
    const refused: string[] = []
    const packages: LocalPackage[] = read.map((p) => ({ ...p, resources: AppLookup.resources(p.dir, p.manifest), system: p === owner }))
    const desktopPackage = packages[read.indexOf(owner)]!
    const desktop = LocalRegistry.#desktop(desktopPackage, desktopId, refused)
    const apps: LocalApp[] = []
    for (const pkg of packages) {
      for (const raw of pkg.manifest.apps ?? []) {
        const app = Manifest.app(raw)
        if (typeof app === 'string') {
          refused.push(`${pkg.dir}/package.json: ${app}`)
          continue
        }
        if (apps.some((a) => a.app.id === app.id)) {
          refused.push(`${pkg.dir}/package.json: app ${app.id} is already in ${apps.find((a) => a.app.id === app.id)!.dir}`)
          continue
        }
        apps.push({ dir: pkg.dir, app, resources: pkg.resources, package: pkg })
      }
    }
    return new LocalRegistry(roots, desktop, apps, refused, files)
  }

  static #read(files: LocalFiles, dir: string): { dir: string; name: string; manifest: PackageManifest } | null {
    const text = files.readText(join(dir, 'package.json'))
    if (text === null) return null
    try {
      const json = JSON.parse(text) as { name?: unknown; acde?: PackageManifest }
      if (!json.acde) return null
      return { dir, name: typeof json.name === 'string' ? json.name : dir, manifest: json.acde }
    } catch {
      return null
    }
  }

  static #desktop(pkg: LocalPackage, wanted: string | null, refused: string[]): LocalDesktop {
    const desktop = (pkg.manifest.desktops ?? []).find((d) => wanted === null || d.id === wanted)!
    const id = desktop.id
    const grants = Manifest.desktopServices(desktop.services)
    if (typeof grants === 'string') throw new Error(`${pkg.dir}/package.json: desktop ${id}: ${grants}`)
    const services: LocalService[] = []
    for (const grant of grants ?? []) {
      const service = (pkg.manifest.services ?? []).find((s) => s.name === grant.name)
      if (service) services.push({ grant, module: join(pkg.dir, service.module) })
      else refused.push(`${pkg.dir}/package.json: desktop ${id} lists service ${grant.name}, which the package does not declare`)
    }
    return { package: pkg, desktop, services }
  }

  app(id: string): LocalApp | undefined {
    return this.apps.find((a) => a.app.id === id)
  }

  byName(name: string): LocalApp | undefined {
    const wanted = name.trim().toLowerCase()
    return this.apps.find((a) => a.app.id.toLowerCase() === wanted) ?? this.apps.find((a) => a.app.label.toLowerCase() === wanted)
  }

  forType(mime: string): LocalApp | undefined {
    const [major] = mime.split('/')
    return this.apps.find((a) => a.app.mimeTypes.some((m: string) => m === mime || m === `${major}/*`))
  }

  forScheme(scheme: string): LocalApp | undefined {
    return this.apps.find((a) => a.app.schemes.includes(scheme))
  }

  packageResources(): Found['resources'] {
    return this.roots.map((root) => ({ prefix: `${PACKAGES_PREFIX}${root.name}/`, dirs: [join(root.dir, '/')] }))
  }

  listed(): ListedApp[] {
    return this.apps.map((a) => ({ id: a.app.id, label: a.app.label, icon: this.iconOf(a), script: a.app.entry ?? null }))
  }

  iconOf(app: LocalApp): string {
    const icon = app.app.icon
    if (!icon.startsWith('/')) return icon
    const file = this.#packageFile(app.package, icon)
    const root = file && this.roots.find((r) => file.startsWith(join(r.dir, '/')))
    if (!file || !root) return icon
    const relative = file.slice(join(root.dir, '/').length).split('/').map(percentEncoded).join('/')
    return `${PACKAGES_PREFIX}${root.name}/${relative}`
  }

  #packageFile(pkg: LocalPackage, path: string): string | null {
    const resource = pkg.resources.filter((r) => path.startsWith(r.prefix)).sort((a, b) => b.prefix.length - a.prefix.length)[0]
    if (!resource) return null
    const rest = path.slice(resource.prefix.length)
    if (rest.split('/').some((s) => s === '..' || s === '.')) return null
    return resource.dirs.map((dir) => join(dir, rest)).find((file) => this.files.isFile(file)) ?? null
  }
}
