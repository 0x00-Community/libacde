import { mkdirSync, readdirSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'

const ENTRIES = [['page/src/worker.ts', 'page.js'], ['page/src/webworker.ts', 'webworker.js'], ['launcher/src/main.ts', 'launcher.js']] as const

const [out = join(import.meta.dir, 'dist'), version = '0.0.0-dev'] = process.argv.slice(2)
const level = readFileSync(join(import.meta.dir, '../acde-api/api-level'), 'utf8').trim()
const lowest = readFileSync(join(import.meta.dir, '../acde-api/min-api-level'), 'utf8').trim()

const DECLARED = /\b(class|function)\s*\*?\s*([A-Za-z_$][\w$]*)/g
const RENAMED = /\b(class|function)\s+([A-Za-z_$][\w$]*?)(\d+)\b/g

const SOURCES = ['dom/src', 'page/src', 'launcher/src', '../manifest']

function declared(): Set<string> {
  const names = new Set<string>()
  for (const dir of SOURCES) {
    for (const file of readdirSync(resolve(import.meta.dir, dir), { recursive: true, encoding: 'utf8' })) {
      if (!/\.[cm]?[jt]sx?$/.test(file)) continue
      for (const [, , name] of readFileSync(resolve(import.meta.dir, dir, file), 'utf8').matchAll(DECLARED)) names.add(name!)
    }
  }
  return names
}

function exposed(code: string, stem: string, bundled: string): boolean {
  return code.includes(`{${stem}:${bundled},`) || code.includes(`,${stem}:${bundled},`) || code.includes(`,${stem}:${bundled}}`) || code.includes(`{${stem}:${bundled}}`)
}

function renamed(code: string, names: ReadonlySet<string>): string[] {
  const found = new Set<string>()
  for (const [, kind, stem, suffix] of code.matchAll(RENAMED)) {
    const bundled = stem! + suffix!
    if (!names.has(stem!) || names.has(bundled)) continue
    if (kind === 'function' && !exposed(code, stem!, bundled)) continue
    found.add(`${kind} ${stem} (bundled as ${bundled})`)
  }
  return [...found]
}

mkdirSync(resolve(out), { recursive: true })
const names = declared()
for (const [entry, name] of ENTRIES) {
  const file = join(resolve(out), name)
  const built = Bun.spawnSync([
    process.execPath, 'build', entry, '--format', 'esm', '--target', 'browser', '--minify-whitespace', '--minify-syntax', '--keep-names',
    '--conditions', 'source', '--define', `ACDEV_API_LEVEL=${level}`, '--define', `ACDEV_MIN_API_LEVEL=${lowest}`,
    '--define', `ACDEV_RUNTIME_VERSION="${version}"`,
    '--outfile', file,
  ], { cwd: import.meta.dir, stdout: 'inherit', stderr: 'inherit' })
  if (built.exitCode !== 0) process.exit(built.exitCode ?? 1)
  const lost = renamed(readFileSync(file, 'utf8'), names)
  if (lost.length) {
    console.error(`${name}: bun renamed ${lost.length} declaration(s), so their .name is wrong in pages:\n  ${lost.join('\n  ')}`)
    console.error('A module refers to the name without binding it (a bare global, or a value import used only as a type in an import cycle),')
    console.error('or two modules declare it. Write globalThis.<name>, import it with `type`, or rename one; see docs/agents/runtime/layout.md "js".')
    process.exit(1)
  }
}
