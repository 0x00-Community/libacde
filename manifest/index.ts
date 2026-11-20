import type { AppBackend, AppManifest as AppEntry, Capabilities, PackageManifest } from '../acde-api/src/index'
import { API_LEVEL } from './level.ts'
import { FS_ROOTS, SHARED_PLACES } from './places.ts'

export type { Capabilities, PackageManifest } from '../acde-api/src/index'
export type { CapabilityState, CapabilityStatus, PlatformSupport, RunningMode, WindowFeatures } from '../acde-api/src/index'
export { API_LEVEL, FS_ROOTS, SHARED_PLACES }
export type { FsRootName, PlaceCapability, SharedPlace } from '../acde-api/src/index'
export { PLATFORM_SUPPORT, RUNNING_MODES, platformSupport } from './platforms.ts'

export const ICON_NAMES = [
  'files',
  'browser',
  'editor',
  'office',
  'term',
  'projects',
  'dash',
  'team',
  'dopa',
  'settings',
  'store',
  'app',
  'timer',
  'cards',
  'list',
  'chart',
  'calc',
  'music',
  'video',
  'photo',
  'paint',
  'game',
  'chat',
  'mail',
  'calendar',
  'code',
  'book',
  'map',
  'weather',
] as const

export type IconName = (typeof ICON_NAMES)[number]

export const SHELL_MODULES = ['windows', 'packages', 'session', 'desktops', 'inspect', 'desktop', 'setDefault', 'permissions', 'voice'] as const

export const APP_MODULES = ['launch', 'apps', 'open', 'openPath', 'handlers', 'window', 'services'] as const

export type ShellModule = (typeof SHELL_MODULES)[number]

export const SYSTEM_CAPABILITIES = [
  'fs', 'terminal', 'network', 'system', 'bluetooth', 'audio', 'devices', 'power', 'input', 'display',
  'office', 'launch', 'slots', 'open', 'openFiles', 'camera', 'microphone', 'idleDetection',
] as const

export const WEB_CAPABILITIES = ['notifications'] as const

export const CAPABILITY_MODULES = ['fs', 'terminal', 'network', 'system', 'bluetooth', 'audio', 'devices', 'power', 'input', 'display', 'office'] as const

export const APPROVAL_CAPABILITIES = ['camera', 'microphone', 'idleDetection'] as const

export interface ImportRule {
  readonly allowed: boolean
  readonly missing: string | null
  readonly reason: string | null
  readonly unverified: string | null
}

export const RESERVED_NAMES: ReadonlySet<string> = new Set([...SYSTEM_CAPABILITIES, ...WEB_CAPABILITIES, ...SHELL_MODULES, ...APP_MODULES, 'capabilities'])

export interface DesktopServiceGrant {
  readonly name: string
  readonly modules: ShellModule[]
}

export interface ConfiguredService extends DesktopServiceGrant {
  readonly module: string
}

export type AppManifest = Required<Omit<AppEntry, 'backend' | 'size'>> & {
  readonly backend: Required<AppBackend> | null
  readonly size: readonly [number, number] | null
}

export interface Findings {
  readonly errors: string[]
  readonly warnings: string[]
}

// biome-ignore lint/complexity/noStaticOnlyClass: moved unchanged from @acde/api's validate.ts
export class Manifest {
  static readonly HEAP_MB = 256
  static readonly MIN_HEAP_MB = 32
  static readonly CAPABILITIES: ReadonlySet<string> = new Set([...SYSTEM_CAPABILITIES, ...SHELL_MODULES])
  static readonly #SHELL: ReadonlySet<string> = new Set(SHELL_MODULES)
  static readonly #PLACES: ReadonlySet<string> = new Set(SHARED_PLACES)
  static readonly #ROOTS: ReadonlySet<string> = new Set(FS_ROOTS)
  static readonly #MIME = /^[\w.+-]+\/([\w.+-]+|\*)$/
  static readonly #ICON_NAMES: ReadonlySet<string> = new Set(ICON_NAMES)
  static readonly #ICON_IMAGE = /^\/(?!\/)[^\s\0?#]*\.(svg|png)$/i

  static places(caps: Capabilities): string[] {
    const fs = caps.fs
    return typeof fs === 'object' && fs && Array.isArray(fs.roots) ? fs.roots.filter((root) => Manifest.#ROOTS.has(root)) : []
  }

  static place(capability: string): string | null {
    const name = capability.startsWith('fs.') ? capability.slice(3) : ''
    return Manifest.#ROOTS.has(name) ? name : null
  }

  static #roots(fs: unknown): string[] {
    if (typeof fs !== 'object' || !fs) return []
    const shared = FS_ROOTS.join(', ')
    const roots = (fs as { roots?: unknown }).roots
    if (!Array.isArray(roots)) return [`fs.roots lists shared places (${shared})`]
    return roots.filter((root) => !Manifest.#ROOTS.has(root)).map((root) => {
      const named = Manifest.#placeIn(root)
      const fix = named
        ? `: write "${named}", the runtime finds that folder through the system (also when it was moved)`
        : ` (${shared}); any other folder is one the user picks (showDirectoryPicker() with "fs": true)`
      return `fs.roots ${JSON.stringify(root)} is not a shared place${fix}`
    })
  }

  static #placeIn(root: unknown): string | null {
    if (typeof root !== 'string') return null
    const last = root.split(/[\\/]+/).filter(Boolean).at(-1)?.toLowerCase() ?? ''
    return Manifest.#PLACES.has(last) ? last : null
  }

  static iconName(icon: string): icon is IconName {
    return Manifest.#ICON_NAMES.has(icon)
  }

  static iconImage(icon: string): boolean {
    return Manifest.#ICON_IMAGE.test(icon)
  }

  static apiLevel(raw: unknown): number | null | string {
    if (raw === undefined) return null
    return Number.isInteger(raw) && (raw as number) > 0 ? (raw as number) : 'acde.apiLevel must be a positive whole number (the contract level)'
  }

  static shellModules(caps: Capabilities): ShellModule[] {
    return SHELL_MODULES.filter((name) => !!caps[name])
  }

  static desktopServices(raw: unknown): DesktopServiceGrant[] | null | string {
    if (raw === undefined || raw === null) return null
    if (!Array.isArray(raw)) return 'services lists service names or {name, modules}'
    const grants: DesktopServiceGrant[] = []
    for (const entry of raw as unknown[]) {
      if (typeof entry === 'string' && entry) {
        grants.push({ name: entry, modules: [] })
        continue
      }
      const { name, modules = [] } = (typeof entry === 'object' && entry !== null ? entry : {}) as { name?: unknown; modules?: unknown }
      if (typeof name !== 'string' || !name) return 'services lists service names or {name, modules}'
      if (!Array.isArray(modules) || modules.some((m) => !Manifest.#SHELL.has(m)))
        return `service ${JSON.stringify(name)}: modules lists shell modules (${SHELL_MODULES.join(', ')})`
      grants.push({ name, modules: [...(modules as ShellModule[])] })
    }
    return grants
  }

  static configServices(raw: unknown): ConfiguredService[] | string {
    if (raw === undefined || raw === null) return []
    if (typeof raw !== 'object' || Array.isArray(raw)) return 'services maps each service name to its module or {module, modules}'
    const services: ConfiguredService[] = []
    for (const [name, entry] of Object.entries(raw)) {
      const at = `service ${JSON.stringify(name)}`
      if (typeof entry === 'string' && entry) {
        services.push({ name, module: entry, modules: [] })
        continue
      }
      const fields = (typeof entry === 'object' && entry !== null && !Array.isArray(entry) ? entry : {}) as Record<string, unknown>
      const { module, modules, ...rest } = fields
      if (typeof module !== 'string' || !module) return `${at}: a service is its module or {module, modules}`
      const [unknown] = Object.keys(rest)
      if (unknown !== undefined) return `${at}: ${JSON.stringify(unknown)} is not a field of a service (module, modules)`
      const grants = Manifest.desktopServices([{ name, modules }])
      if (!Array.isArray(grants)) return grants ?? `${at}: a service is its module or {module, modules}`
      services.push({ name, module, modules: grants[0]!.modules })
    }
    return services
  }

  static readonly UNVERSIONED = `no acde.apiLevel: a package built before contract levels; rebuild it with the current acde (level ${API_LEVEL})`

  static refusal(level: number): string | null {
    return level === API_LEVEL ? null : `needs level ${level}, runtime is level ${API_LEVEL}`
  }

  static heapMb(value: unknown, fallback: number): number {
    const mb = Number(value)
    return Number.isFinite(mb) && mb >= Manifest.MIN_HEAP_MB ? mb : fallback
  }

  static app(raw: unknown): AppManifest | string {
    const incomplete = 'an app needs id, label, icon and entry'
    if (typeof raw !== 'object' || raw === null) return incomplete
    const field = (key: string) => (raw as Record<string, unknown>)[key]
    const text = (key: string) => (typeof field(key) === 'string' && field(key) ? (field(key) as string) : null)
    const id = text('id'),
      label = text('label'),
      icon = text('icon'),
      entry = text('entry')
    if (!id || !label || !icon || !entry || !/^[\w-]+$/.test(id) || !entry.startsWith('/')) return incomplete
    const backend = Manifest.#backend(field('backend'))
    if (typeof backend === 'string') return `app ${id}: ${backend}`
    if (entry.endsWith('/') && !backend)
      return `app ${id}: an entry ending in / is a page the backend renders: it needs a backend`
    const capabilities =
      typeof field('capabilities') === 'object' && field('capabilities') !== null
        ? (field('capabilities') as Capabilities)
        : {}
    const types = field('mimeTypes')
    if (
      types !== undefined &&
      (!Array.isArray(types) || types.some((t) => typeof t !== 'string' || !Manifest.#MIME.test(t)))
    )
      return `app ${id}: mimeTypes must be MIME types (type/subtype, or type/* for all of a type)`
    const mimeTypes = (types as string[] | undefined) ?? []
    const schemes = Manifest.#schemes(field('schemes'))
    if (typeof schemes === 'string') return `app ${id}: ${schemes}`
    const requests = Manifest.#requests(field('requests'))
    if (typeof requests === 'string') return `app ${id}: ${requests}`
    const size = Manifest.#size(field('size'))
    if (size === undefined) return `app ${id}: size must be [width, height] in whole logical pixels`
    const services = Manifest.#services(field('services'))
    if (typeof services === 'string') return `app ${id}: ${services}`
    const optional = Manifest.#optional(field('optional'), capabilities)
    if (typeof optional === 'string') return `app ${id}: ${optional}`
    return {
      id,
      label,
      icon,
      entry,
      capabilities,
      heapMb: Manifest.heapMb(field('heapMb'), Manifest.HEAP_MB),
      multiple: field('multiple') === true,
      frame: field('frame') !== false,
      mimeTypes,
      schemes,
      requests,
      backend,
      size,
      optional,
      services,
    }
  }

  // biome-ignore lint/complexity/noExcessiveCognitiveComplexity: moved unchanged from @acde/api's validate.ts
  static importRule(name: string, app: AppManifest | null, origin: { readonly system: boolean }): ImportRule {
    const allowed: ImportRule = { allowed: true, missing: null, reason: null, unverified: null }
    if (!app) return allowed
    const caps = app.capabilities
    const declared = `the capabilities of ${app.label}`
    const refuse = (missing: string | null, reason: string): ImportRule => ({ allowed: false, missing, reason, unverified: null })
    if (Manifest.#SHELL.has(name)) {
      if (!origin.system) {
        const why = `only the desktop, its services and system apps may import it, and ${app.label} is not a system app`
        return refuse(null, `acde:${name} is a shell module: ${why}`)
      }
      return caps[name] ? allowed : refuse(name, `acde:${name} is a shell module: declare "${name}" in ${declared}`)
    }
    if (name === 'launch') return caps.launch || caps.slots ? allowed : refuse('launch', `acde:launch needs "launch" or "slots" in ${declared}`)
    if (name === 'openPath' || name === 'handlers') return caps.openFiles ? allowed : refuse('openFiles', `acde:${name} needs "openFiles" in ${declared}`)
    if ((CAPABILITY_MODULES as readonly string[]).includes(name)) return caps[name] ? allowed : refuse(name, `acde:${name} needs "${name}" in ${declared}`)
    if ((APP_MODULES as readonly string[]).includes(name) || name === 'capabilities') return allowed
    if (app.services.includes(name)) return allowed
    if (app.services.length > 0) {
      const listed = app.services.map((service) => JSON.stringify(service)).join(', ')
      const one = app.services.length === 1 ? 'the service' : 'one of the services'
      return { ...allowed, unverified: `acde:${name} resolves only if ${one} ${app.label} lists (${listed}) provides the "${name}" namespace` }
    }
    return refuse(null, `acde:${name} is not a system module; if a desktop service provides it, list that service's name in the services of ${app.label}`)
  }

  static reserved(name: string): string | null {
    return RESERVED_NAMES.has(name) ? `service ${JSON.stringify(name)} has the name of a system module (acde:${name}); system module names are reserved` : null
  }

  // biome-ignore lint/complexity/noExcessiveCognitiveComplexity: moved unchanged from @acde/api's validate.ts
  static check(
    manifest: PackageManifest,
    isDir: (path: string) => boolean,
    resolve: (url: string) => boolean,
    isFile?: (path: string) => boolean,
  ): Findings {
    const findings: Findings = { errors: [], warnings: [] }
    const { errors, warnings } = findings
    const apps = manifest.apps ?? []
    const level = Manifest.apiLevel(manifest.apiLevel)
    if (level === null) errors.push(Manifest.UNVERSIONED)
    else if (typeof level === 'string') errors.push(level)
    else if (Manifest.refusal(level)) errors.push(`acde.apiLevel ${level}: ${Manifest.refusal(level)}`)
    if (apps.length === 0 && (manifest.desktops ?? []).length === 0) errors.push('acde.apps is empty: no app')
    for (const [prefix, dirs] of Object.entries(manifest.assets ?? {})) {
      if (!prefix.startsWith('/') || !prefix.endsWith('/'))
        errors.push(`assets ${JSON.stringify(prefix)}: a prefix is /…/`)
      for (const dir of dirs.filter((d) => !isDir(d)))
        errors.push(`assets ${JSON.stringify(prefix)}: no directory ${JSON.stringify(dir)}`)
    }
    const services = new Set((manifest.services ?? []).map((service) => service.name))
    for (const name of services) {
      const reserved = Manifest.reserved(name)
      if (reserved) errors.push(reserved)
    }
    for (const desktop of manifest.desktops ?? []) {
      const at = `desktop ${JSON.stringify(desktop?.id)}`
      if (desktop && typeof desktop === 'object' && 'frame' in desktop) errors.push(`${at}: frame is not a desktop field (only apps have a frame)`)
      for (const field of ['entry', 'lock'] as const) {
        const path: unknown = desktop?.[field]
        if (field === 'lock' && path === undefined) continue
        const example = `desktop/${field === 'lock' ? 'lock' : 'index'}.html`
        if (!Manifest.packagePath(path)) errors.push(`${at}: ${field} must be a path inside the package, such as ${example}`)
        else if (isFile && !isFile(path)) errors.push(`${at}: no ${field} ${path}`)
      }
      const grants = Manifest.desktopServices(desktop?.services)
      if (typeof grants === 'string') errors.push(`${at}: ${grants}`)
      for (const grant of Array.isArray(grants) ? grants : []) {
        if (!services.has(grant.name)) warnings.push(`${at}: no service ${JSON.stringify(grant.name)} in acde.services`)
      }
    }
    const system = (manifest.desktops ?? []).length > 0
    const shellFindings = errors
    const seen = new Set<string>()
    for (const raw of apps) {
      const app = Manifest.app(raw)
      if (typeof app === 'string') {
        errors.push(app)
        continue
      }
      const at = `app ${JSON.stringify(app.id)}`
      if (seen.has(app.id)) errors.push(`${at}: twice`)
      seen.add(app.id)
      if (!app.entry.endsWith('/') && !resolve(app.entry))
        errors.push(`${at}: entry ${JSON.stringify(app.entry)} is in no assets directory`)
      if (app.backend && !resolve(app.backend.entry))
        errors.push(`${at}: backend.entry ${JSON.stringify(app.backend.entry)} is in no assets directory`)
      if (Manifest.iconImage(app.icon)) {
        if (!resolve(app.icon)) errors.push(`${at}: icon ${JSON.stringify(app.icon)} is in no assets directory`)
      } else if (!Manifest.iconName(app.icon)) {
        warnings.push(
          `${at}: icon ${JSON.stringify(app.icon)} is neither an icon name (${ICON_NAMES.join(', ')}) ` +
            `nor an image (.svg or .png) under the assets, such as /app/${app.id}/icon.svg`,
        )
      }
      if (raw.heapMb !== undefined && Manifest.heapMb(raw.heapMb, 0) === 0)
        warnings.push(`${at}: heapMb below ${Manifest.MIN_HEAP_MB} is ignored (${Manifest.HEAP_MB} then)`)
      if (!system) {
        for (const name of Manifest.shellModules(app.capabilities))
          shellFindings.push(`${at}: acde:${name} is a shell module: only apps of a desktop package (system apps) may declare it`)
      }
      for (const finding of Manifest.#roots(app.capabilities.fs)) shellFindings.push(`${at}: ${finding}`)
      for (const name of Object.keys(app.capabilities).filter((k) => !Manifest.CAPABILITIES.has(k))) {
        if (services.has(name)) errors.push(`${at}: capability ${JSON.stringify(name)} is a desktop service: list it under services`)
        else shellFindings.push(`${at}: capability ${JSON.stringify(name)} is not a system capability; list desktop services under services`)
      }
    }
    return findings
  }

  static readonly SYSTEM_REQUESTS: readonly string[] = ['open-file', 'open-link']

  static #requests(raw: unknown): string[] | string {
    if (raw === undefined || raw === null) return []
    const valid = (s: unknown) => typeof s === 'string' && /^[a-z][a-z0-9-]*$/.test(s) && !Manifest.SYSTEM_REQUESTS.includes(s)
    const system = Manifest.SYSTEM_REQUESTS.join(' and ')
    if (!Array.isArray(raw) || !raw.every(valid)) return `requests lists private request kinds (lower-case words such as task); ${system} are system requests`
    return [...(raw as string[])]
  }

  static #schemes(raw: unknown): string[] | string {
    if (raw === undefined || raw === null) return []
    const valid = (s: unknown) => typeof s === 'string' && /^[a-z][a-z0-9+.-]*$/.test(s) && s !== 'file'
    if (!Array.isArray(raw) || !raw.every(valid)) return 'schemes lists lower-case URL schemes without the colon (https, mailto); file is not one'
    return [...(raw as string[])]
  }

  static packagePath(raw: unknown): raw is string {
    if (typeof raw !== 'string' || !raw || raw.startsWith('/') || raw.includes('\\') || raw.includes('\0')) return false
    return raw.split('/').every((part) => part !== '..')
  }

  static #services(raw: unknown): string[] | string {
    if (raw === undefined || raw === null) return []
    if (!Array.isArray(raw) || raw.some((name) => typeof name !== 'string' || !name)) return 'services lists desktop service names'
    for (const name of raw as string[]) {
      const reserved = Manifest.reserved(name)
      if (reserved) return reserved
    }
    return [...(raw as string[])]
  }

  static #optional(raw: unknown, capabilities: Capabilities): string[] | string {
    if (raw === undefined || raw === null) return []
    if (!Array.isArray(raw) || raw.some((name) => typeof name !== 'string' || !name)) return 'optional lists capability names'
    const missing = (raw as string[]).find((name) => !capabilities[name])
    if (missing) return `optional ${JSON.stringify(missing)} is not in capabilities (optional lists declared capabilities that the app can run without)`
    return [...(raw as string[])]
  }

  static #size(raw: unknown): readonly [number, number] | null | undefined {
    if (raw === undefined || raw === null) return null
    return Array.isArray(raw) && raw.length === 2 && raw.every((n) => Number.isInteger(n) && n > 0)
      ? [raw[0], raw[1]]
      : undefined
  }

  static #backend(raw: unknown): Required<AppBackend> | null | string {
    if (raw === undefined || raw === null) return null
    if (typeof raw !== 'object' || Array.isArray(raw)) return 'backend must be an object {entry, port?}'
    const { entry, port = 3000 } = raw as { entry?: unknown; port?: unknown }
    if (
      typeof entry !== 'string' ||
      !entry.startsWith('/') ||
      entry.startsWith('//') ||
      /[\s\0?#]/.test(entry) ||
      !/\.m?js$/.test(entry)
    )
      return 'backend.entry must be a same-origin URL of a .js module under the assets (/app/<id>/backend/server.js)'
    if (typeof port !== 'number' || !Number.isInteger(port) || port < 0 || port > 65535)
      return 'backend.port must be a port number (0 to 65535)'
    return { entry, port }
  }
}
