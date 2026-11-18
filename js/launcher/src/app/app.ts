import { API_LEVEL, Manifest, type AppManifest, type PackageManifest } from '@dopame/acde-api/manifest'
import type { WebInit, WebTier } from '../../../page/src/contract'
import type { Languages } from '../../../page/src/language'
import { PageSource } from '../../../page/src/page'
import { isAbsolute, join, resolve } from '../../../page/src/sys/path'

export interface Found {
  readonly dir: string
  readonly app: AppManifest
  readonly resources: { prefix: string; dirs: string[] }[]
}

export interface Files {
  readText(path: string): string | null
  readDir(path: string): string[]
}

export interface Place {
  readonly width: number
  readonly height: number
  readonly scale: number
  readonly languages: Languages
  readonly navigator: { readonly userAgent: string; readonly platform: string }
}

export class AppLookup {
  constructor(readonly files: Files) {}

  find(target: string, path: readonly string[], cwd: string): Found {
    const dir = isAbsolute(target) || target.startsWith('.') ? resolve(cwd, target) : null
    if (dir) return this.#first(dir, null) ?? this.#fail(`${dir}: no acde app in its package.json`)
    for (const root of path) {
      const found = this.#first(root, target) ?? this.files.readDir(root).map((name) => this.#first(join(root, name), target)).find(Boolean)
      if (found) return found
    }
    return this.#fail(`no app ${target} in ACDE_PATH (${path.join(':') || 'unset'})`)
  }

  #first(dir: string, id: string | null): Found | null {
    const text = this.files.readText(join(dir, 'package.json'))
    if (text === null) return null
    const manifest = (JSON.parse(text) as { acde?: PackageManifest }).acde
    for (const raw of manifest?.apps ?? []) {
      const app = Manifest.app(raw)
      if (typeof app === 'string') throw new Error(`${dir}/package.json: ${app}`)
      if (id === null || app.id === id) return { dir, app, resources: AppLookup.resources(dir, manifest!) }
    }
    return null
  }

  static resources(dir: string, manifest: PackageManifest): { prefix: string; dirs: string[] }[] {
    const assets = Object.entries((manifest as { assets?: Record<string, unknown> }).assets ?? {})
    const listed = assets
      .filter(([prefix, dirs]) => /^\/[\w./-]*\/$/.test(prefix) && Array.isArray(dirs))
      .map(([prefix, dirs]) => ({ prefix, dirs: AppLookup.#relative(dirs as unknown[]).map((d) => join(dir, d, '/')) }))
    return listed.some((r) => r.prefix === '/') ? listed : [...listed, { prefix: '/', dirs: [join(dir, '/')] }]
  }

  static #relative(dirs: unknown[]): string[] {
    return dirs.filter((d): d is string => typeof d === 'string' && !isAbsolute(d))
  }

  #fail(message: string): never {
    throw Object.assign(new Error(message), { status: 404 })
  }
}

export interface Given {
  readonly tier?: WebTier | null
  readonly request?: Record<string, unknown> | null
  readonly services?: WebInit['services']
}

function common(token: bigint, window: number, place: Place, services: WebInit['services'] = {}) {
  return {
    token,
    window,
    width: place.width * place.scale,
    height: place.height * place.scale,
    scale: place.scale,
    dark: false,
    languages: place.languages,
    navigator: place.navigator,
    services,
  }
}

export function webInit(found: Found, token: bigint, place: Place, given: Given = {}, window = 1): WebInit {
  const { app } = found
  return {
    kind: 'window',
    name: `${app.id}#${window}`,
    ...common(token, window, place, given.services),
    resources: PageSource.isHtml(app.entry) ? PageSource.rooted(app.entry, found.resources) : found.resources,
    app,
    theme: '',
    request: given.request ?? null,
    url: null,
    tier: given.tier ?? { level: API_LEVEL, shell: [], refused: {} },
  }
}

export function desktopInit(
  page: string, resources: Found['resources'], token: bigint, place: Place, tier: WebTier, services: WebInit['services'] = {},
): WebInit {
  return { kind: 'desktop', name: 'desktop', ...common(token, 0, place, services), resources, page, tier }
}
