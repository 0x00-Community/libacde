import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { parseArgs } from 'node:util'

const HERE = import.meta.dir
const STATE = process.env.WPT_STATE ?? join(HERE, '..', '..', 'logs', 'wpt')
const PASSING = new Set(['OK', 'PASS'])
const SLOW = new Set(['TIMEOUT', 'NOLOAD', 'HANG', 'CRASH'])
const BROKEN = new Set(['CRASH', 'HANG'])

type Sub = { name: string; status: number; message: string | null }
type Result = { url: string; kind?: string; status: string; message?: string | null; tests?: Sub[] }
export type Expectation = { status: string; fail?: string[]; flaky?: true | string[] }
type Expectations = Map<string, Expectation>

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    platform: { type: 'string', default: process.platform },
    dir: { type: 'string', default: '' },
    out: { type: 'string', default: '' },
    max: { type: 'string', default: '200' },
  },
})
const command = positionals[0]
const DIR = values.dir || join(HERE, 'expectations', values.platform!)

function top(url: string): string {
  const parts = url.split('?')[0]!.split('/').filter(Boolean)
  return parts[0] === 'css' && parts.length > 2 ? `css-${parts[1]}` : parts[0]!
}

function load(): Expectations {
  const map: Expectations = new Map()
  if (!existsSync(DIR)) return map
  for (const name of readdirSync(DIR))
    if (name.endsWith('.json')) for (const [url, e] of Object.entries(JSON.parse(readFileSync(join(DIR, name), 'utf8')) as Record<string, Expectation | string>)) map.set(url, typeof e === 'string' ? { status: e } : e)
  return map
}

function save(map: Expectations) {
  mkdirSync(DIR, { recursive: true })
  const files = new Map<string, [string, Expectation][]>()
  for (const [url, e] of map) {
    const name = top(url)
    if (!files.has(name)) files.set(name, [])
    files.get(name)!.push([url, e])
  }
  for (const name of readdirSync(DIR)) if (name.endsWith('.json') && !files.has(name.slice(0, -5))) writeFileSync(join(DIR, name), '{}\n')
  for (const [name, entries] of files) {
    entries.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0))
    const lines = entries.map(([url, e]) => {
      const compact = !e.fail?.length && !e.flaky ? JSON.stringify(e.status) : JSON.stringify({ status: e.status, ...(e.fail?.length && { fail: [...e.fail].sort() }), ...(e.flaky && { flaky: Array.isArray(e.flaky) ? [...e.flaky].sort() : true }) })
      return `  ${JSON.stringify(url)}: ${compact}`
    })
    writeFileSync(join(DIR, `${name}.json`), `{\n${lines.join(',\n')}\n}\n`)
  }
}

function results(label: string): Map<string, Result> {
  const map = new Map<string, Result>()
  const file = join(STATE, 'results', label, 'results.jsonl')
  if (!existsSync(file)) throw new Error(`${file}: no such run`)
  for (const line of readFileSync(file, 'utf8').split('\n')) if (line) map.set(JSON.parse(line).url, JSON.parse(line))
  return map
}

function expectationOf(r: Result): Expectation {
  const fail = (r.tests ?? []).filter((t) => t.status !== 0).map((t) => t.name)
  return { status: r.status, ...(fail.length && { fail }) }
}

function merge(runs: Map<string, Result>[]): Expectations {
  const out: Expectations = new Map()
  const urls = new Set(runs.flatMap((run) => [...run.keys()]))
  for (const url of urls) {
    const seen = runs.map((run) => run.get(url)).filter((r): r is Result => !!r)
    const first = expectationOf(seen[0]!)
    if (seen.some((r) => r.status !== seen[0]!.status)) {
      out.set(url, { ...first, flaky: true })
      continue
    }
    const failSets = seen.map((r) => new Set(expectationOf(r).fail ?? []))
    const names = new Set(seen.flatMap((r) => (r.tests ?? []).map((t) => t.name)))
    const flaky = [...names].filter((n) => failSets.some((s) => s.has(n)) && failSets.some((s) => !s.has(n)))
    const fail = [...names].filter((n) => failSets.every((s) => s.has(n)))
    out.set(url, { status: first.status, ...(fail.length && { fail }), ...(flaky.length && { flaky }) })
  }
  return out
}

function selectedOf(label: string): string[] | null {
  const file = join(STATE, 'results', label, 'selected.json')
  return existsSync(file) ? (JSON.parse(readFileSync(file, 'utf8')) as string[]) : null
}

function check(label: string) {
  const expected = load()
  const got = results(label)
  const regressions: string[] = []
  const improvements: string[] = []
  const fresh: string[] = []
  // a stopped worker or a lost shard must not read as "no regressions": every selected test needs a result
  const missing = (selectedOf(label) ?? []).filter((url) => !got.has(url))
  for (const [url, r] of got) {
    const e = expected.get(url)
    if (!e) {
      fresh.push(`${url}: ${r.status}`)
      continue
    }
    if (e.flaky === true) continue
    const flaky = new Set(Array.isArray(e.flaky) ? e.flaky : [])
    const said = r.message ? ` (${String(r.message).slice(0, 200)})` : ''
    if (PASSING.has(e.status) && r.status !== e.status) {
      regressions.push(`${url}: ${e.status} -> ${r.status}${said}`)
      continue
    }
    // a test that was failing and now takes the runtime down is worse, not the same
    if (BROKEN.has(r.status) && r.status !== e.status) {
      regressions.push(`${url}: ${e.status} -> ${r.status}${said}`)
      continue
    }
    if (!PASSING.has(e.status) && PASSING.has(r.status)) improvements.push(`${url}: ${e.status} -> ${r.status}`)
    if (e.status !== 'OK' || r.status !== 'OK') continue
    const fail = new Set(e.fail ?? [])
    const subs = new Map((r.tests ?? []).map((t) => [t.name, t]))
    for (const [name, t] of subs) {
      if (flaky.has(name)) continue
      if (t.status !== 0 && !fail.has(name)) regressions.push(`${url} | ${name}: ${String(t.message ?? `status ${t.status}`).slice(0, 200)}`)
      if (t.status === 0 && fail.has(name)) improvements.push(`${url} | ${name}`)
    }
  }
  const lines = [
    `# ${label} against ${DIR}`,
    '',
    `${got.size} tests run; ${regressions.length} regressions, ${improvements.length} improvements, ${fresh.length} without an expectation, ${missing.length} selected but not run.`,
    '',
    `## Regressions (${regressions.length})`,
    '',
    ...regressions.map((r) => `- ${r}`),
    '',
    `## Selected but not run (${missing.length}): the run stopped early or a shard is missing`,
    '',
    ...missing.map((r) => `- ${r}`),
    '',
    `## Improvements (${improvements.length}): run expectations update to keep them`,
    '',
    ...improvements.map((r) => `- ${r}`),
    '',
    `## Without an expectation (${fresh.length})`,
    '',
    ...fresh.map((r) => `- ${r}`),
  ]
  writeFileSync(join(STATE, 'results', label, 'check.md'), lines.join('\n') + '\n')
  console.log(lines[2])
  const max = Number(values.max)
  for (const r of regressions.slice(0, max)) console.log(`  regression: ${r}`)
  if (regressions.length > max) console.log(`  … ${regressions.length - max} more in ${join(STATE, 'results', label, 'check.md')}`)
  if (missing.length) console.log(`  ${missing.length} selected tests have no result, e.g. ${missing.slice(0, 3).join(', ')}`)
  process.exit(regressions.length || missing.length ? 1 : 0)
}

function update(labels: string[]) {
  const map = load()
  const merged = merge(labels.map(results))
  for (const [url, e] of merged) map.set(url, e)
  save(map)
  const flaky = [...merged.values()].filter((e) => e.flaky).length
  console.log(`${merged.size} expectations written to ${DIR} from ${labels.join(', ')} (${flaky} with flaky parts); ${map.size} in all`)
}

function gate() {
  const map = load()
  const urls = [...map].filter(([, e]) => e.flaky !== true && PASSING.has(e.status) && !SLOW.has(e.status)).map(([url]) => url).sort()
  const out = values.out || join(STATE, 'gate.json')
  writeFileSync(out, JSON.stringify(urls) + '\n')
  console.log(`${urls.length} of ${map.size} tests in the gate -> ${out}`)
}

function summary() {
  const map = load()
  const by = new Map<string, Map<string, number>>()
  for (const [url, e] of map) {
    const name = top(url)
    if (!by.has(name)) by.set(name, new Map())
    const row = by.get(name)!
    row.set(e.status, (row.get(e.status) ?? 0) + 1)
  }
  for (const [name, row] of [...by].sort()) console.log(`${name}: ${[...row].map(([s, n]) => `${s} ${n}`).join(', ')}`)
}

if (command === 'check' && positionals[1]) check(positionals[1])
else if (command === 'update' && positionals.length > 1) update(positionals.slice(1))
else if (command === 'gate') gate()
else if (command === 'summary') summary()
else {
  console.error('usage: scripts/wpt/wpt.sh expectations check LABEL | update LABEL [LABEL…] | gate [--out FILE] | summary   [--platform darwin|linux]')
  process.exit(2)
}
