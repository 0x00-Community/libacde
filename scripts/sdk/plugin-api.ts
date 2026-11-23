import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join, normalize, relative, resolve } from 'node:path'

const ROOT = resolve(import.meta.dir, '../..')
const COPY = join(ROOT, 'acde-api/vendor/plugin-api')
const STAMP = 'SYNCED.json'
const ENTRIES = ['src/tools.ts']
const SKIPPED = new Set(['.gitignore', 'AGENTS.md'])
const SOURCE = resolve(process.env['PLUGIN_API_DIR'] ?? join(ROOT, '../plugin-api'))
const IMPORT = /\bfrom\s+['"](\.{1,2}\/[^'"]+)['"]/g

interface Stamp {
  readonly commit: string
}

function git(args: readonly string[]): string {
  const run = Bun.spawnSync(['git', '-C', SOURCE, ...args], { stdout: 'pipe', stderr: 'pipe' })
  if (run.exitCode !== 0) throw new Error(`git ${args.join(' ')}: ${run.stderr.toString().trim()}`)
  return run.stdout.toString()
}

const show = (commit: string, path: string): string => git(['show', `${commit}:${path}`])

function resolved(from: string, specifier: string, tracked: ReadonlySet<string>): string {
  const base = normalize(join(dirname(from), specifier))
  const found = [base, `${base}.ts`, join(base, 'index.ts')].find((path) => tracked.has(path))
  if (!found) throw new Error(`${from} imports ${specifier}, which plugin-api does not track`)
  return found
}

function needed(commit: string): string[] {
  const tracked = new Set(git(['ls-tree', '-r', '--name-only', commit]).split('\n').filter((path) => path && !SKIPPED.has(path)))
  const files = new Set<string>()
  const pending = [...ENTRIES]
  for (let path = pending.pop(); path !== undefined; path = pending.pop()) {
    if (files.has(path)) continue
    if (!tracked.has(path)) throw new Error(`plugin-api does not track ${path}`)
    files.add(path)
    for (const [, specifier] of show(commit, path).matchAll(IMPORT)) pending.push(resolved(path, specifier!, tracked))
  }
  return [...files].sort()
}

function copied(dir = COPY): string[] {
  if (!existsSync(dir)) return []
  return readdirSync(dir)
    .flatMap((name) => {
      const path = join(dir, name)
      return statSync(path).isDirectory() ? copied(path) : [relative(COPY, path)]
    })
    .filter((path) => path !== STAMP)
}

function sync(): void {
  if (git(['status', '--porcelain']).trim()) throw new Error(`${SOURCE} has uncommitted changes; commit them first`)
  const commit = git(['rev-parse', 'HEAD']).trim()
  const files = needed(commit)
  for (const path of copied()) if (!files.includes(path)) rmSync(join(COPY, path))
  for (const path of files) {
    mkdirSync(dirname(join(COPY, path)), { recursive: true })
    writeFileSync(join(COPY, path), show(commit, path))
  }
  writeFileSync(join(COPY, STAMP), `${JSON.stringify({ commit } satisfies Stamp, null, 2)}\n`)
  console.log(`plugin-api: copied ${files.length} files from ${commit.slice(0, 7)}`)
}

function drift(commit: string): string[] {
  const files = needed(commit)
  const changed = files.filter((path) => {
    const here = join(COPY, path)
    return !existsSync(here) || readFileSync(here, 'utf8') !== show(commit, path)
  })
  const extra = copied().filter((path) => !files.includes(path))
  return [...changed.map((path) => `changed or missing: ${path}`), ...extra.map((path) => `not needed from plugin-api: ${path}`)]
}

function check(): number {
  const stamp = JSON.parse(readFileSync(join(COPY, STAMP), 'utf8')) as Stamp
  if (!existsSync(SOURCE)) {
    console.log(`plugin-api: ${SOURCE} is not here; skipped the check`)
    return 0
  }
  const found = drift(stamp.commit)
  if (!found.length) {
    console.log(`plugin-api: the copy matches ${stamp.commit.slice(0, 7)}`)
    return 0
  }
  console.error('plugin-api: acde-api/vendor/plugin-api was edited directly; change plugin-api, then run sync')
  for (const line of found) console.error(`  ${line}`)
  return 1
}

const command = process.argv[2]
if (command === 'sync') sync()
else if (command === 'check') process.exitCode = check()
else {
  console.error('usage: bun scripts/sdk/plugin-api.ts sync | check')
  process.exitCode = 2
}
