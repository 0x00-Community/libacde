import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import type { Test } from './prepare'

const STATE = process.env.WPT_STATE ?? join(import.meta.dir, '..', '..', 'logs', 'wpt')
const label = process.argv[2]
if (!label) throw new Error('usage: scripts/wpt/wpt.sh report <label> [--depth N]')
const depth = Number(process.argv[process.argv.indexOf('--depth') + 1] || 2) || 2
const dir = join(STATE, 'results', label)
const meta = JSON.parse(readFileSync(join(dir, 'meta.json'), 'utf8'))
const tests = JSON.parse(readFileSync(join(STATE, 'tests.json'), 'utf8')) as Test[]
const only = meta.dirs ? (meta.dirs as string).split(',').map((d) => '/' + d.replace(/^\/+|\/+$/g, '') + '/') : null
const inScope = tests.filter((t) => !only || only.some((d) => t.url.startsWith(d)))
const byUrl = new Map(inScope.map((t) => [t.url, t]))

type Sub = { name: string; status: number; message: string | null }
type Result = { url: string; status: string; message?: string | null; tests?: Sub[]; ms: number }
const results = new Map<string, Result>()
for (const line of readFileSync(join(dir, 'results.jsonl'), 'utf8').split('\n')) if (line) results.set(JSON.parse(line).url, JSON.parse(line))

const group = (url: string) => url.split('?')[0]!.split('/').slice(1, -1).slice(0, depth).join('/') || '(root)'
type Row = { files: number; ok: number; error: number; timeout: number; crash: number; pass: number; subs: number; filesAllPass: number; ms: number }
const empty = (): Row => ({ files: 0, ok: 0, error: 0, timeout: 0, crash: 0, pass: 0, subs: 0, filesAllPass: 0, ms: 0 })
const rows = new Map<string, Row>()
const total = empty()
const statusCount: Record<string, number> = {}
const flagStats = new Map<string, { files: number; timeout: number; pass: number; subs: number }>()

for (const r of results.values()) {
  const t = byUrl.get(r.url)
  if (!t) continue
  const g = group(r.url)
  if (!rows.has(g)) rows.set(g, empty())
  const subs = r.tests ?? []
  const pass = subs.length ? subs.filter((s) => s.status === 0).length : r.status === 'PASS' ? 1 : 0
  for (const row of [rows.get(g)!, total]) {
    row.files++
    row.ms += r.ms
    if (r.status === 'OK' || r.status === 'PASS') row.ok++
    else if (r.status === 'ERROR') row.error++
    else if (r.status === 'TIMEOUT' || r.status === 'NOLOAD') row.timeout++
    else if (r.status === 'CRASH' || r.status === 'HANG') row.crash++
    row.pass += pass
    row.subs += Math.max(subs.length, 1)
    if ((r.status === 'OK' && subs.length > 0 && pass === subs.length) || r.status === 'PASS') row.filesAllPass++
  }
  statusCount[r.status] = (statusCount[r.status] ?? 0) + 1
  for (const f of [...t.flags, ...(r.url.includes('.tentative.') || r.url.includes('/tentative/') ? ['tentative'] : [])]) {
    if (!flagStats.has(f)) flagStats.set(f, { files: 0, timeout: 0, pass: 0, subs: 0 })
    const s = flagStats.get(f)!
    s.files++
    if (r.status === 'TIMEOUT') s.timeout++
    s.pass += pass
    s.subs += Math.max(subs.length, 1)
  }
}

const normalize = (m: string) =>
  m
    .replace(/\s+at .*$/s, '')
    .replace(/^(promise_test: Unhandled rejection with value: (?:object )?)"(.{0,90})[^"]*"$/s, (_, head: string, inner: string) => `${head}«${inner.replace(/\b\d+\b/g, 'N')}»`)
    .replace(/"(?:[^"\\]|\\.)*"/g, '"…"')
    .replace(/'(?:[^'\\]|\\.)*'/g, "'…'")
    .replace(/\b\d+(\.\d+)?\b/g, 'N')
    .replace(/Element node <[^>]*>(?:[^<]*<\/[^>]*>)?/g, 'Element node <…>')
    .slice(0, 160)
const clusters = new Map<string, { count: number; files: Set<string>; example: string }>()
const missing = new Map<string, { count: number; files: Set<string> }>()
const API = [
  /(?:ReferenceError: )?([A-Za-z_$][\w$]*) is not defined/,
  /([A-Za-z_$][\w$.]*) is not a constructor/,
  /([A-Za-z_$][\w$.]*) is not a function/,
  /Cannot read properties of (?:undefined|null) \(reading '([^']+)'\)/,
  /undefined is not an object \(evaluating '([^']+)'\)/,
  /assert_(?:true|own_property|idl_attribute|inherits)[^:]*: (?:property )?"?([A-Za-z_$][\w$]*)"? (?:missing|is not|not found|should)/,
]
function note(message: string | null | undefined, url: string) {
  if (!message) return
  const key = normalize(message)
  if (!clusters.has(key)) clusters.set(key, { count: 0, files: new Set(), example: url })
  const c = clusters.get(key)!
  c.count++
  c.files.add(url)
  for (const re of API) {
    const m = message.match(re)
    if (m) {
      const name = m[1]!
      if (!missing.has(name)) missing.set(name, { count: 0, files: new Set() })
      missing.get(name)!.count++
      missing.get(name)!.files.add(url)
      break
    }
  }
}
for (const r of results.values()) {
  if (!byUrl.has(r.url)) continue
  if (r.status !== 'OK' && r.status !== 'PASS') note(`[${r.status}] ${r.message ?? ''}`, r.url)
  for (const s of r.tests ?? []) if (s.status !== 0) note(s.message ?? `subtest status ${s.status}`, r.url)
}

const pct = (a: number, b: number) => (b ? `${((100 * a) / b).toFixed(1)}%` : '—')
const notRun = (reason: string) => inScope.filter((t) => t.unhosted === reason).length
const notYet = inScope.filter((t) => !t.unhosted && !results.has(t.url)).length
const lines: string[] = []
lines.push(`# WPT on libacde — ${label}`, '')
const took = meta.seconds ? `, ${Math.round(meta.seconds / 60)} min` : ''
const runtime = meta.runtime ?? meta.acdert
lines.push(`- WPT ${String(meta.wpt).slice(0, 10)}, runtime \`${runtime}\`, ${meta.platform}, ${meta.workers} workers, started ${meta.started}${took}`)
lines.push('- testharness, reftest and crashtest files; a reftest or crashtest counts as one subtest.')
lines.push(`- The runner cannot host yet: ${notRun('https')} need https/h2, ${notRun('websocket')} websockets (wptserve's ws port is not the page's), ${notRun('print')} print reftests${notYet ? `; ${notYet} not reached yet` : ''}. Not applicable: see scripts/wpt/not-applicable.`)
lines.push(`- Ran ${total.files} test files: ${Object.entries(statusCount).sort((a, b) => b[1] - a[1]).map(([k, v]) => `${k} ${v}`).join(', ')}.`)
lines.push(`- Subtests: **${total.pass} / ${total.subs} pass (${pct(total.pass, total.subs)})**; files with every subtest passing: ${total.filesAllPass} / ${total.files} (${pct(total.filesAllPass, total.files)}).`)
lines.push('- A TIMEOUT or ERROR file with no subtests counts as one failing subtest.', '')
lines.push('## By directory', '', '| dir | files | subtests pass | % | all-pass files | TIMEOUT or NOLOAD | ERROR | CRASH |', '| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |')
for (const [g, r] of [...rows.entries()].sort((a, b) => b[1].subs - a[1].subs))
  lines.push(`| ${g} | ${r.files} | ${r.pass} / ${r.subs} | ${pct(r.pass, r.subs)} | ${r.filesAllPass} | ${r.timeout} | ${r.error} | ${r.crash} |`)
lines.push('', '## Tests that use iframes, testdriver or server handlers', '', 'How tests that load iframes, drive input through testdriver or call wptserve handlers fare.', '', '| flag | files | TIMEOUT | subtests pass |', '| --- | ---: | ---: | ---: |')
for (const [f, s] of [...flagStats.entries()].sort((a, b) => b[1].files - a[1].files)) lines.push(`| ${f} | ${s.files} | ${s.timeout} | ${s.pass} / ${s.subs} (${pct(s.pass, s.subs)}) |`)
lines.push('', '## Missing or broken APIs named by failures', '', '| name | failing subtests | files | example |', '| --- | ---: | ---: | --- |')
for (const [name, m] of [...missing.entries()].sort((a, b) => b[1].files.size - a[1].files.size).slice(0, 60))
  lines.push(`| \`${name}\` | ${m.count} | ${m.files.size} | ${[...m.files][0]} |`)
lines.push('', '## Most common failure messages', '', '| failures | files | message | example |', '| ---: | ---: | --- | --- |')
for (const [msg, c] of [...clusters.entries()].sort((a, b) => b[1].files.size - a[1].files.size || b[1].count - a[1].count).slice(0, 60))
  lines.push(`| ${c.count} | ${c.files.size} | ${msg.replace(/\|/g, '\\|').replace(/\n/g, ' ')} | ${c.example} |`)

writeFileSync(join(dir, 'report.md'), lines.join('\n') + '\n')
writeFileSync(
  join(dir, 'summary.json'),
  JSON.stringify(
    {
      meta,
      total,
      statusCount,
      notRun: { worker: notRun('worker'), server: notRun('server'), https: notRun('https'), notReached: notYet },
      dirs: Object.fromEntries(rows),
      flags: Object.fromEntries(flagStats),
      missing: Object.fromEntries([...missing.entries()].sort((a, b) => b[1].files.size - a[1].files.size).map(([k, v]) => [k, { count: v.count, files: v.files.size }])),
    },
    null,
    2,
  ) + '\n',
)
console.log(lines.slice(0, 9).join('\n'))
if (existsSync(join(dir, 'report.md'))) console.log(`\n-> ${join(dir, 'report.md')}`)
