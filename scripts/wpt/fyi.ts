import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { parseArgs } from 'node:util'
import type { Test } from './prepare'
import type { Result, Sub } from './run'

const STATE = process.env.WPT_STATE ?? join(import.meta.dir, '..', '..', 'logs', 'wpt')
const REVISION = readFileSync(join(import.meta.dir, 'revision'), 'utf8').trim()
const { values } = parseArgs({ options: { products: { type: 'string', default: 'chrome,firefox,safari,servo' }, refresh: { type: 'boolean', default: false } } })
const products = values.products.split(',').filter(Boolean)

type Run = { id: number; browser_name: string; browser_version: string; os_name: string; os_version: string; full_revision_hash: string; time_start: string; raw_results_url: string }
type Report = { results: { test: string; status: string; message?: string | null; duration?: number; subtests: { name: string; status: string; message?: string | null }[] }[] }

const SUBTEST: Record<string, number> = { PASS: 0, FAIL: 1, TIMEOUT: 2, NOTRUN: 3, PRECONDITION_FAILED: 4 }

const api = await fetch(`https://wpt.fyi/api/runs?sha=${REVISION}&products=${products.join(',')}`)
if (!api.ok) throw new Error(`wpt.fyi runs for ${REVISION}: HTTP ${api.status}`)
const listed = (await api.json()) as Run[]
const runs = products.flatMap((p) => listed.filter((r) => r.browser_name === p).slice(0, 1))
const missing = products.filter((p) => !runs.some((r) => r.browser_name === p))
if (missing.length) {
  const hint = 'pick a revision with runs of every product (see docs/agents/dev/testing.md)'
  throw new Error(`wpt.fyi has no run of ${missing.join(', ')} at ${REVISION}: ${hint}`)
}

const manifest = join(STATE, 'tests.json')
const kinds = new Map(existsSync(manifest) ? (JSON.parse(readFileSync(manifest, 'utf8')) as Test[]).map((t) => [t.url, t.kind]) : [])
const cache = join(STATE, 'fyi')
mkdirSync(cache, { recursive: true })

for (const run of runs) {
  if (run.full_revision_hash !== REVISION) throw new Error(`run ${run.id} is at ${run.full_revision_hash}, not ${REVISION}`)
  const label = `fyi-${run.browser_name}`
  const file = join(cache, `${run.browser_name}-${run.id}.json.gz`)
  if (values.refresh || !existsSync(file)) {
    const got = await fetch(run.raw_results_url, { headers: { 'Accept-Encoding': 'gzip' }, decompress: false } as RequestInit)
    if (!got.ok) throw new Error(`${run.raw_results_url}: HTTP ${got.status}`)
    const bytes = new Uint8Array(await got.arrayBuffer())
    writeFileSync(file, bytes[0] === 0x1f && bytes[1] === 0x8b ? bytes : Bun.gzipSync(bytes))
  }
  const gz = readFileSync(file)
  const report = JSON.parse(new TextDecoder().decode(Bun.gunzipSync(gz))) as Report
  const lines: string[] = []
  for (const r of report.results) {
    const tests: Sub[] = r.subtests.map((s) => ({ name: s.name, status: SUBTEST[s.status] ?? 1, message: s.message ?? null }))
    const kind = kinds.get(r.test) ?? (tests.length || r.status === 'OK' ? 'testharness' : 'reftest')
    const result: Result = { url: r.test, kind, status: r.status, message: r.message ?? null, ms: r.duration ?? 0, worker: 0 }
    if (kind === 'testharness') result.tests = tests
    lines.push(JSON.stringify(result))
  }
  const dir = join(STATE, 'results', label)
  mkdirSync(dir, { recursive: true })
  writeFileSync(join(dir, 'results.jsonl'), lines.join('\n') + '\n')
  const meta = {
    label,
    wpt: REVISION,
    runtime: `wpt.fyi run ${run.id}: ${run.browser_name} ${run.browser_version} on ${run.os_name} ${run.os_version}`,
    source: run.raw_results_url,
    sha256: createHash('sha256').update(gz).digest('hex'),
    started: run.time_start,
    workers: 0,
    dirs: null,
    platform: run.os_name,
  }
  writeFileSync(join(dir, 'meta.json'), JSON.stringify(meta, null, 2) + '\n')
  console.log(`${label}: ${lines.length} tests, ${meta.runtime}`)
}
