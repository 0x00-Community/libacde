import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { basename, dirname, join, relative, resolve } from 'node:path'

const HERE = dirname(new URL(import.meta.url).pathname)
const ROOT = resolve(HERE, '..')
const TEXTS = join(HERE, 'notices', 'texts')
const RULE = '='.repeat(80)
const LICENSE_FILE = /^(licen[cs]e|copying|copyright|notice|unlicense|authors)([-._ ].*)?$/i
const LICENSE_DIR = /^licen[cs]es?$/i
const PREFERRED = ['MIT', 'Apache-2.0', 'BSD-3-Clause', 'BSD-2-Clause', 'ISC', 'Zlib', 'BSL-1.0', 'Unicode-3.0', '0BSD', 'Unlicense', 'MPL-2.0']
const OWN_IMPORTS = /^(node:|bun:|acde:|@dopame\/)/
const TYPE_ONLY = /^\s*(?:import|export)\s+(?:type\s[^;]*?|\{(?:\s*type\s+[\w$]+(?:\s+as\s+[\w$]+)?\s*,?)+\s*\})\s*from\s*'[^']*'/gm
const BARE_IMPORT = /(?:from\s+|import\s*\(\s*)'((?:@[a-z0-9][a-z0-9._-]*\/)?[a-z0-9][a-z0-9._-]*)(?:\/[^']*)?'/g

export interface Options {
  readonly cargo: readonly string[]
  readonly vendor: readonly string[]
  readonly own: readonly string[]
  readonly npm: readonly string[]
  readonly sources: readonly string[]
  readonly extra: readonly string[]
  readonly out: string
  readonly title: string
  readonly check: boolean
}

interface LicenseText {
  readonly label: string
  readonly text: string
}

interface Entry {
  readonly name: string
  readonly version: string
  readonly kind: 'crate' | 'npm' | 'component'
  readonly license: string
  readonly url: string
  readonly texts: readonly LicenseText[]
  readonly note: string | null
  readonly mplSource: string | null
}

interface Component {
  readonly name: string
  readonly within: string
  readonly url: string
  readonly license: string
  readonly copyright: readonly string[]
  readonly source?: string
}

interface NpmManifest {
  readonly license?: string | { type?: string }
  readonly author?: string | { name?: string }
  readonly repository?: string | { url?: string }
  readonly homepage?: string
}

interface Locked {
  readonly name: string
  readonly version: string
  readonly source: string | null
}

export class Problems {
  readonly list: string[] = []

  add(problem: string): void {
    this.list.push(problem)
  }
}

export const Spdx = {
  terms(expression: string): string[][] {
    const normal = expression.replace(/\//g, ' OR ').replace(/[()]/g, ' ').replace(/\s+/g, ' ').trim()
    return normal.split(' OR ').map((alternative) => alternative.split(' AND ').map((term) => term.trim()).filter(Boolean))
  },

  elected(expression: string): string[] {
    const alternatives = Spdx.terms(expression)
    const rank = (terms: string[]) => Math.max(...terms.map((term) => (PREFERRED.includes(term) ? PREFERRED.indexOf(term) : PREFERRED.length)))
    return [...alternatives].sort((a, b) => rank(a) - rank(b))[0] ?? []
  },

  text(id: string): string | null {
    const file = join(TEXTS, `${id.replace(/ /g, '-')}.txt`)
    return existsSync(file) ? readFileSync(file, 'utf8') : null
  },

  isMplOnly(expression: string): boolean {
    return Spdx.elected(expression).includes('MPL-2.0')
  },
}

export const Toml = {
  field(text: string, table: string, key: string): string | null {
    const body = Toml.table(text, table)
    const found = body?.match(new RegExp(`^${key.replace('.', '\\.')}\\s*=\\s*"([^"]*)"`, 'm'))
    return found?.[1] ?? null
  },

  list(text: string, table: string, key: string): string[] {
    const found = Toml.table(text, table)?.match(new RegExp(`^${key}\\s*=\\s*\\[([^\\]]*)\\]`, 'm'))
    return found ? [...found[1]!.matchAll(/"((?:[^"\\]|\\.)*)"/g)].map((m) => m[1]!.replace(/\\"/g, '"')) : []
  },

  inherits(text: string, key: string): boolean {
    return new RegExp(`^${key}(\\.workspace\\s*=\\s*true|\\s*=\\s*\\{\\s*workspace\\s*=\\s*true)`, 'm').test(Toml.table(text, 'package') ?? '')
  },

  table(text: string, table: string): string | null {
    const start = text.search(new RegExp(`^\\[${table.replace('.', '\\.')}\\]\\s*$`, 'm'))
    if (start < 0) return null
    const rest = text.slice(start).split('\n').slice(1)
    const end = rest.findIndex((line) => /^\[/.test(line))
    return (end < 0 ? rest : rest.slice(0, end)).join('\n')
  },
}

export const CargoLock = {
  read(file: string): Locked[] {
    const blocks = readFileSync(file, 'utf8').split('[[package]]').slice(1)
    return blocks.map((block) => ({
      name: block.match(/^name = "([^"]+)"/m)?.[1] ?? '',
      version: block.match(/^version = "([^"]+)"/m)?.[1] ?? '',
      source: block.match(/^source = "([^"]+)"/m)?.[1] ?? null,
    }))
  },
}

export const LicenseFiles = {
  in(dir: string): LicenseText[] {
    if (!existsSync(dir)) return []
    const out: LicenseText[] = []
    for (const name of readdirSync(dir).sort()) {
      const path = join(dir, name)
      const info = statSync(path)
      if (info.isFile() && LICENSE_FILE.test(name)) out.push({ label: name, text: readFileSync(path, 'utf8') })
      else if (info.isDirectory() && LICENSE_DIR.test(name)) out.push(...LicenseFiles.in(path).map((t) => ({ ...t, label: `${name}/${t.label}` })))
    }
    return out
  },

  fallback(expression: string, holders: readonly string[], problems: Problems, who: string): LicenseText[] {
    const terms = Spdx.elected(expression)
    if (terms.length === 0) problems.add(`${who}: no license file and no license field`)
    const lines = holders.length > 0 ? `${holders.map((holder) => `Copyright (c) ${holder}`).join('\n')}\n\n` : ''
    return terms.map((term) => {
      const text = Spdx.text(term)
      if (text === null) problems.add(`${who}: no license file, and scripts/notices/texts has no text for ${term}`)
      return { label: `${term} (the package has no license file; text for its license field)`, text: `${lines}${text ?? ''}` }
    })
  },
}

export class Registry {
  readonly #home = process.env['CARGO_HOME'] || join(homedir(), '.cargo')

  dir(locked: Locked): string | null {
    const src = join(this.#home, 'registry', 'src')
    const prefix = Registry.prefix(locked.source ?? '')
    if (!prefix || !existsSync(src)) return null
    for (const index of readdirSync(src).filter((name) => name.startsWith(`${prefix}-`))) {
      const dir = join(src, index, `${locked.name}-${locked.version}`)
      if (existsSync(join(dir, 'Cargo.toml'))) return dir
    }
    return null
  }

  static prefix(source: string): string | null {
    if (source === 'registry+https://github.com/rust-lang/crates.io-index' || source === 'sparse+https://index.crates.io/') return 'index.crates.io'
    const url = /^(?:sparse|registry)\+\w+:\/\/([^/:]+)/.exec(source)
    return url?.[1] ?? null
  }

  static page(locked: Locked): string {
    return Registry.prefix(locked.source ?? '') === 'index.crates.io' ? `https://crates.io/crates/${locked.name}/${locked.version}` : (locked.source ?? '')
  }
}

export class Crates {
  readonly #registry = new Registry()

  constructor(readonly options: Options, readonly problems: Problems) {}

  entries(): Entry[] {
    const seen = new Map<string, Locked>()
    for (const file of this.options.cargo) for (const locked of CargoLock.read(file)) seen.set(`${locked.name}@${locked.version}`, locked)
    const own = new Set(this.options.own)
    return [...seen.values()].filter((locked) => !own.has(locked.name)).flatMap((locked) => this.#entry(locked) ?? [])
  }

  #entry(locked: Locked): Entry | null {
    const dir = locked.source === null ? this.#vendored(locked.name) : this.#registry.dir(locked)
    const who = `crate ${locked.name} ${locked.version}`
    if (!dir) {
      this.problems.add(locked.source === null
        ? `${who}: a path dependency in no --vendor directory and not --own`
        : `${who}: not in the local cargo registry (${locked.source}); run cargo fetch --locked first`)
      return null
    }
    const toml = readFileSync(join(dir, 'Cargo.toml'), 'utf8')
    const workspace = locked.source === null ? this.#workspaceToml(dir) : null
    const inherited = (key: string) => (workspace && Toml.inherits(toml, key) ? Toml.field(workspace.text, 'workspace.package', key) : null)
    const field = (key: string) => Toml.field(toml, 'package', key) ?? inherited(key)
    const license = field('license') ?? ''
    const own = LicenseFiles.in(dir)
    const fromWorkspace = own.length === 0 && workspace ? LicenseFiles.in(workspace.dir) : []
    const found = [...own, ...fromWorkspace]
    const texts = found.length > 0 ? found : LicenseFiles.fallback(license, Toml.list(toml, 'package', 'authors'), this.problems, who)
    const note = found.length > 0 ? null : 'The package carries no license file; the text follows its license field.'
    const mplSource = Spdx.isMplOnly(license) && locked.source !== null ? `${Registry.page(locked)} (unmodified)` : null
    const url = field('repository') ?? field('homepage') ?? Registry.page(locked)
    return { name: locked.name, version: locked.version, kind: 'crate', license, url, texts, note, mplSource }
  }

  #vendored(name: string): string | null {
    for (const root of this.options.vendor) {
      const found = Crates.find(root, (dir) => Toml.field(readFileSync(join(dir, 'Cargo.toml'), 'utf8'), 'package', 'name') === name)
      if (found) return found
    }
    return null
  }

  #workspaceToml(dir: string): { dir: string; text: string } | null {
    for (let at = dirname(dir); at !== dirname(at); at = dirname(at)) {
      const file = join(at, 'Cargo.toml')
      if (existsSync(file) && /^\[workspace\]/m.test(readFileSync(file, 'utf8'))) return { dir: at, text: readFileSync(file, 'utf8') }
      if (this.options.vendor.some((root) => resolve(root) === at)) break
    }
    return null
  }

  static find(root: string, match: (dir: string) => boolean): string | null {
    if (!existsSync(root)) return null
    if (existsSync(join(root, 'Cargo.toml')) && match(root)) return root
    for (const name of readdirSync(root).filter((n) => !n.startsWith('.') && n !== 'target' && n !== 'node_modules')) {
      const path = join(root, name)
      if (statSync(path).isDirectory()) {
        const found = Crates.find(path, match)
        if (found) return found
      }
    }
    return null
  }
}

export class NpmPackages {
  constructor(readonly options: Options, readonly problems: Problems) {}

  entries(): Entry[] {
    const shipped = new Map<string, Entry>()
    for (const dir of this.options.npm) for (const entry of this.#closure(dir)) shipped.set(`${entry.name}@${entry.version}`, entry)
    this.#checkSources(new Set([...shipped.values()].map((entry) => entry.name)))
    return [...shipped.values()]
  }

  #closure(dir: string): Entry[] {
    const manifest = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8')) as Record<string, Record<string, string> | undefined>
    const lock = NpmPackages.lock(join(dir, 'bun.lock'))
    const queue: [string, string][] = Object.keys({ ...manifest['dependencies'], ...manifest['optionalDependencies'] }).map((name) => ['', name])
    const done = new Map<string, Entry>()
    while (queue.length > 0) {
      const [parent, name] = queue.shift()!
      const key = parent && lock.has(`${parent}/${name}`) ? `${parent}/${name}` : name
      if (done.has(key) || this.options.own.includes(name)) continue
      const locked = lock.get(key)
      if (!locked) {
        this.problems.add(`npm ${name}: not in ${join(dir, 'bun.lock')}`)
        continue
      }
      const entry = this.#entry(dir, key, locked.version)
      if (entry) done.set(key, entry)
      for (const dep of locked.deps) queue.push([key, dep])
    }
    return [...done.values()]
  }

  #entry(dir: string, key: string, version: string): Entry | null {
    const at = join(dir, 'node_modules', ...key.split('/').flatMap((part, i, all) => (i > 0 && !all[i - 1]!.startsWith('@') ? ['node_modules', part] : [part])))
    const file = join(at, 'package.json')
    const name = key.split('/').slice(-(key.split('/').at(-2)?.startsWith('@') ? 2 : 1)).join('/')
    if (!existsSync(file)) {
      this.problems.add(`npm ${name} ${version}: not installed in ${relative(ROOT, at) || at}`)
      return null
    }
    const pkg = JSON.parse(readFileSync(file, 'utf8')) as NpmManifest
    const license = typeof pkg.license === 'object' ? (pkg.license?.type ?? '') : (pkg.license ?? '')
    const found = LicenseFiles.in(at)
    const author = typeof pkg.author === 'object' ? pkg.author?.name : pkg.author
    const texts = found.length > 0 ? found : LicenseFiles.fallback(license, author ? [author] : [], this.problems, `npm ${name} ${version}`)
    const repository = typeof pkg.repository === 'object' ? pkg.repository?.url : pkg.repository
    const url = pkg.homepage ?? repository ?? `https://www.npmjs.com/package/${name}/v/${version}`
    const note = found.length > 0 ? null : 'The package carries no license file; the text follows its license field.'
    const mplSource = Spdx.isMplOnly(license) ? `https://www.npmjs.com/package/${name}/v/${version} (unmodified)` : null
    return { name, version, kind: 'npm', license, url, texts, note, mplSource }
  }

  #checkSources(shipped: Set<string>): void {
    const own = new Set([...this.options.own, ...this.options.npm.map((dir) => NpmPackages.name(dir))])
    for (const dir of this.options.sources) {
      for (const file of NpmPackages.files(dir)) {
        for (const [, spec] of NpmPackages.runtimeCode(readFileSync(file, 'utf8')).matchAll(BARE_IMPORT)) {
          if (OWN_IMPORTS.test(spec!) || shipped.has(spec!) || own.has(spec!)) continue
          this.problems.add(`${relative(ROOT, file)} imports ${spec}, which is not among the shipped npm dependencies`)
        }
      }
    }
  }

  static name(dir: string): string {
    return (JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8')) as { name?: string }).name ?? ''
  }

  static runtimeCode(source: string): string {
    return source.replace(TYPE_ONLY, '')
  }

  static files(dir: string): string[] {
    if (!existsSync(dir)) return []
    const names = readdirSync(dir, { recursive: true, encoding: 'utf8' })
    return names.filter((name) => /\.(ts|tsx|js|mjs)$/.test(name) && !name.includes('node_modules')).map((name) => join(dir, name))
  }

  static lock(file: string): Map<string, { version: string; deps: string[] }> {
    const text = readFileSync(file, 'utf8').replace(/,(\s*[}\]])/g, '$1')
    const packages = (JSON.parse(text) as { packages?: Record<string, unknown[]> }).packages ?? {}
    return new Map(Object.entries(packages).map(([key, value]) => {
      const spec = String(value[0] ?? '')
      const object = value.find((item) => typeof item === 'object' && item !== null && !Array.isArray(item)) ?? {}
      const meta = object as Record<string, Record<string, string> | undefined>
      const deps = Object.keys({ ...meta['dependencies'], ...meta['optionalDependencies'] })
      return [key, { version: spec.slice(spec.lastIndexOf('@') + 1), deps }]
    }))
  }
}

export class Components {
  constructor(readonly options: Options, readonly problems: Problems) {}

  entries(present: Set<string>): Entry[] {
    const out: Entry[] = []
    for (const file of this.options.extra) {
      const listed = (JSON.parse(readFileSync(file, 'utf8')) as { components: Component[] }).components
      for (const component of listed.filter((c) => present.has(c.within))) {
        const texts = LicenseFiles.fallback(component.license, [], this.problems, `component ${component.name}`)
        const copyright = component.copyright.join('\n')
        const label = `${component.license} (inside the ${component.within} crate)`
        out.push({
          name: component.name, version: `in ${component.within}`, kind: 'component', license: component.license, url: component.url,
          texts: texts.map((t) => ({ label, text: `${copyright}\n\n${t.text}` })), note: null, mplSource: component.source ?? null,
        })
      }
    }
    return out
  }
}

export const ModifiedMpl = {
  files(roots: readonly string[]): LicenseText[] {
    const out: LicenseText[] = []
    for (const root of roots) {
      for (const name of readdirSync(root, { recursive: true, encoding: 'utf8' }).sort()) {
        if (/(^|\/)(target|node_modules|\.git)(\/|$)/.test(name) || !/\.(css|rs|js|ts|html)$/.test(name)) continue
        const text = readFileSync(join(root, name), 'utf8')
        const mpl = /Mozilla Public[\s*]+License,? v(ersion)?\.? ?2\.0/.test(text.slice(0, 600))
        if (mpl && /acrender:/.test(text)) out.push({ label: join(basename(root), name), text })
      }
    }
    return out
  },
}

export const Notices = {
  render(title: string, entries: readonly Entry[], modified: readonly LicenseText[]): string {
    const sorted = [...entries].sort((a, b) => `${a.kind}:${a.name}@${a.version}`.localeCompare(`${b.kind}:${b.name}@${b.version}`))
    const ids = new Map<string, number>()
    const bodies: string[] = []
    const index = sorted.map((entry) => {
      const refs = entry.texts.map((t) => {
        const body = `${t.label}\n\n${t.text.trim()}`
        if (!ids.has(body)) {
          ids.set(body, ids.size + 1)
          bodies.push(body)
        }
        return `[${ids.get(body)}]`
      })
      const lines = [entry.note, entry.mplSource ? `MPL-2.0 Source Code Form: ${entry.mplSource}` : null].filter(Boolean)
      const extra = lines.map((line) => `\n    ${line}`).join('')
      return `${entry.name} ${entry.version} (${entry.url})\n    License: ${entry.license || 'see the text'}; text: ${refs.join(' ')}${extra}`
    })
    const head = [
      `Third-party software notices for ${title}`,
      '',
      `${title} includes the software listed below. Each entry names the license under which it is used and points to the license text(s)`,
      'further down. Generated by scripts/notices.ts from the lock files; do not edit by hand.',
    ]
    const texts = bodies.map((body, i) => `${RULE}\n[${i + 1}] ${body}`)
    const mpl = modified.length === 0 ? [] : [
      RULE,
      'Modified files covered by the Mozilla Public License 2.0',
      '',
      'The files below were changed for this product. Their Source Code Form, as changed, follows in full.',
      ...modified.map((file) => `\n--- ${file.label} ---\n${file.text.trimEnd()}`),
    ]
    return `${[...head, '', RULE, 'Components', '', ...index.flatMap((line) => [line, '']), ...texts, ...mpl].join('\n').trimEnd()}\n`
  },
}

export function build(options: Options): { text: string; problems: string[] } {
  const problems = new Problems()
  const crates = new Crates(options, problems).entries()
  const npm = new NpmPackages(options, problems).entries()
  const components = new Components(options, problems).entries(new Set(crates.map((entry) => entry.name)))
  const text = Notices.render(options.title, [...crates, ...npm, ...components], ModifiedMpl.files(options.vendor))
  return { text, problems: problems.list }
}

export function libacde(check: boolean): Options {
  const at = (path: string) => join(ROOT, path)
  return {
    cargo: [at('Cargo.lock')], vendor: [at('vendor/blitz')], own: ['libacde', 'acde-scene', 'acss'], npm: [at('js')],
    sources: [at('js/dom/src'), at('js/page/src'), at('js/launcher/src')], extra: [at('scripts/notices/extra.json')],
    out: at('THIRD-PARTY-NOTICES.txt'), title: 'libacde', check,
  }
}

export function parse(argv: readonly string[]): Options {
  const check = argv.includes('--check')
  const given = argv.filter((arg) => arg !== '--check' && arg !== '--libacde')
  if (given.length === 0) return libacde(check)
  const lists: Record<string, string[]> = { cargo: [], vendor: [], own: [], npm: [], sources: [], extra: [] }
  let out = ''
  let title = 'this product'
  for (let i = 0; i < given.length; i++) {
    const flag = given[i]!
    const value = given[++i]
    if (value === undefined) throw new Error(`notices: ${flag} needs a value`)
    const key = flag.replace(/^--/, '')
    if (key === 'out') out = resolve(value)
    else if (key === 'title') title = value
    else if (key === 'own') lists['own']!.push(...value.split(','))
    else if (key in lists) lists[key]!.push(resolve(value))
    else throw new Error(`notices: unknown option ${flag}`)
  }
  if (!out) throw new Error('notices: --out FILE is needed')
  const [cargo, vendor, own, npm, sources, extra] = (['cargo', 'vendor', 'own', 'npm', 'sources', 'extra'] as const).map((key) => lists[key]!)
  return { cargo: cargo!, vendor: vendor!, own: own!, npm: npm!, sources: sources!, extra: extra!, out, title, check }
}

function main(): number {
  const options = parse(process.argv.slice(2))
  const { text, problems } = build(options)
  for (const problem of problems) process.stderr.write(`notices: ${problem}\n`)
  if (options.check) {
    const current = existsSync(options.out) ? readFileSync(options.out, 'utf8') : null
    if (current !== text) problems.push(`${relative(process.cwd(), options.out)} is not what the lock files give: run bun scripts/notices.ts`)
    if (current !== text) process.stderr.write(`notices: ${problems.at(-1)}\n`)
    if (problems.length === 0) process.stdout.write(`notices: ${relative(process.cwd(), options.out)} covers every dependency\n`)
    return problems.length > 0 ? 1 : 0
  }
  writeFileSync(options.out, text)
  process.stdout.write(`notices: wrote ${relative(process.cwd(), options.out)} (${text.length} bytes)\n`)
  return problems.length > 0 ? 1 : 0
}

if ((import.meta as { main?: boolean }).main) process.exit(main())
