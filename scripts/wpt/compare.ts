import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const STATE = process.env.WPT_STATE ?? join(import.meta.dir, '..', '..', 'logs', 'wpt')
const [before, after] = process.argv.slice(2)
if (!before || !after) throw new Error('usage: scripts/wpt/wpt.sh compare <old label> <new label>')

type Sub = { name: string; status: number; message: string | null }
type Result = { url: string; kind?: string; status: string; message?: string | null; tests?: Sub[] }

function load(label: string): Map<string, Result> {
  const map = new Map<string, Result>()
  for (const line of readFileSync(join(STATE, 'results', label, 'results.jsonl'), 'utf8').split('\n')) if (line) map.set(JSON.parse(line).url, JSON.parse(line))
  return map
}

const a = load(before)
const b = load(after)
const regressions: string[] = []
const progressions: string[] = []
let passA = 0
let passB = 0
let wholeA = 0
let wholeB = 0
let common = 0
for (const [url, ra] of a) {
  const rb = b.get(url)
  if (!rb) continue
  common++
  const subsA = new Map((ra.tests ?? []).map((t) => [t.name, t]))
  const subsB = new Map((rb.tests ?? []).map((t) => [t.name, t]))
  for (const t of subsA.values()) if (t.status === 0) passA++
  for (const t of subsB.values()) if (t.status === 0) passB++
  if (ra.kind === 'reftest' || ra.kind === 'crashtest') {
    if (ra.status === 'PASS') wholeA++
    if (rb.status === 'PASS') wholeB++
    const said = rb.message ? ` (${String(rb.message).slice(0, 160)})` : ''
    if (ra.status === 'PASS' && rb.status !== 'PASS') regressions.push(`${url}: ${ra.kind} PASS -> ${rb.status}${said}`)
    if (ra.status !== 'PASS' && rb.status === 'PASS') progressions.push(`${url}: ${ra.kind} ${ra.status} -> PASS`)
    continue
  }
  if (ra.status !== rb.status && ra.status === 'OK') regressions.push(`${url}: harness ${ra.status} -> ${rb.status}${rb.message ? ` (${String(rb.message).slice(0, 160)})` : ''}`)
  if (ra.status !== rb.status && rb.status === 'OK') progressions.push(`${url}: harness ${ra.status} -> OK`)
  for (const [name, t] of subsA) {
    const u = subsB.get(name)
    if (t.status === 0 && u && u.status !== 0) regressions.push(`${url} | ${name}: ${String(u.message ?? `status ${u.status}`).slice(0, 200)}`)
    if (t.status !== 0 && u?.status === 0) progressions.push(`${url} | ${name}`)
  }
}

function signed(n: number): string {
  return n >= 0 ? `+${n}` : String(n)
}

const lines = [
  `# ${before} -> ${after}`,
  '',
  `${common} files in both; passing subtests ${passA} -> ${passB} (${signed(passB - passA)}),`
    + ` passing reftests and crashtests ${wholeA} -> ${wholeB} (${signed(wholeB - wholeA)}).`,
  '',
  `## Regressions (${regressions.length})`,
  '',
  ...regressions.map((r) => `- ${r}`),
  '',
  `## Progressions (${progressions.length})`,
  '',
  ...progressions.map((r) => `- ${r}`),
]
const out = join(STATE, 'results', after, `compare-${before}.md`)
writeFileSync(out, lines.join('\n') + '\n')
console.log(lines.slice(0, 3).join('\n'))
console.log(`regressions ${regressions.length}, progressions ${progressions.length} -> ${out}`)
for (const r of regressions.slice(0, 30)) console.log(`  - ${r}`)
process.exit(regressions.length ? 1 : 0)
