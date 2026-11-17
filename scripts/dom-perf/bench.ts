import assert from 'node:assert/strict'
import { mkdir, rename } from 'node:fs/promises'
import { join, resolve } from 'node:path'

const artifacts = process.argv[2]
assert.ok(artifacts, 'Run this through scripts/dom-perf/bench.sh')
const run = resolve(artifacts)
const binary = process.platform === 'win32' ? 'acdert.exe' : 'acdert'
const defaultRuntime = resolve('target', 'release', binary)
const defaultApp = resolve('web', 'packages', 'acde-host', 'dist')
const sides = [
  { name: 'candidate', runtime: process.env['ACDEV_RUNTIME'] ?? defaultRuntime, app: process.env['ACDE_APP'] ?? defaultApp },
  ...(process.env['BASE_ACDERT'] || process.env['BASE_APP']
    ? [{ name: 'base', runtime: process.env['BASE_ACDERT'] ?? defaultRuntime, app: process.env['BASE_APP'] ?? defaultApp }]
    : []),
]
const rounds = Number(process.env['DOM_PERF_ROUNDS'] ?? 3)
const limit = Number(process.env['DOM_PERF_LIMIT'] ?? 1.1)

const pkgRoot = join(run, 'package')
await mkdir(pkgRoot, { recursive: true })
await Bun.write(
  join(pkgRoot, 'package.json'),
  JSON.stringify({
    name: 'dom-perf',
    acde: {
      apiLevel: 1,
      apps: [{ id: 'dom-perf', label: 'DOM Perf', icon: 'files', entry: '/dom-perf/index.html', capabilities: {} }],
      assets: { '/dom-perf/': ['.'] },
    },
  }),
)
const page = '<!doctype html><html><head><meta charset="utf-8"><title>DOM Perf</title></head><body><h1>bench</h1></body></html>'
await Bun.write(join(pkgRoot, 'index.html'), page)

const workload = `(async () => {
  const runs = 12, warm = 2, res = { create: [], restyle: [] }
  for (let r = 0; r < runs; r++) {
    const box = document.createElement('div')
    document.body.appendChild(box)
    let t = performance.now()
    for (let i = 0; i < 10000; i++) { const d = document.createElement('div'); d.textContent = 'item ' + i; box.appendChild(d) }
    const create = performance.now() - t
    void box.offsetHeight
    const kids = Array.from(box.childNodes)
    t = performance.now()
    for (let i = 0; i < kids.length; i++) kids[i].style.color = (r + i) % 2 ? 'red' : 'blue'
    void box.offsetHeight
    const restyle = performance.now() - t
    box.remove()
    if (r >= warm) { res.create.push(create); res.restyle.push(restyle) }
    await new Promise((ok) => setTimeout(ok, 30))
  }
  const med = (a) => { const s = [...a].sort((x, y) => x - y); return (s[(s.length - 1) >> 1] + s[s.length >> 1]) / 2 }
  return { create: med(res.create), restyle: med(res.restyle) }
})()`

const median = (values: number[]) => {
  const sorted = [...values].sort((a, b) => a - b)
  return sorted.length ? ((sorted[(sorted.length - 1) >> 1] ?? 0) + (sorted[sorted.length >> 1] ?? 0)) / 2 : 0
}

type Result = { create: number; restyle: number }

async function session(side: (typeof sides)[number], index: number): Promise<Result> {
  const dir = join(run, `${side.name}-${index}`)
  const debug = join(dir, 'debug')
  const home = join(dir, 'home')
  const state = join(dir, 'state')
  await Promise.all([debug, home, state].map((path) => mkdir(path, { recursive: true })))
  const env = { ...process.env }
  for (const name of ['ACDE_STORAGE', 'ACDE_BACKEND', 'ACDE_DEV', 'ACDEV_RUNTIME']) delete env[name]
  const child = Bun.spawn([side.runtime, 'app', 'dom-perf', '--seconds', '180'], {
    env: {
      ...env,
      ACDE_APP: side.app,
      ACDE_PATH: pkgRoot,
      ACDE_DEBUG_DIR: debug,
      ACDE_HEADLESS: '1',
      HOME: home,
      USERPROFILE: home,
      XDG_CONFIG_HOME: home,
      XDG_DATA_HOME: home,
      XDG_STATE_HOME: state,
      ...Object.fromEntries(['DESKTOP', 'DOCUMENTS', 'DOWNLOAD', 'MUSIC', 'PICTURES', 'VIDEOS'].map((n) => [`XDG_${n}_DIR`, join(home, 'places', n)])),
    },
    stdout: Bun.file(join(dir, 'runtime.stdout.log')),
    stderr: Bun.file(join(dir, 'runtime.stderr.log')),
  })
  async function call<T>(name: string, source: string): Promise<T> {
    const base = join(debug, name)
    await Bun.write(`${base}.tmp`, `export default ${source}`)
    await rename(`${base}.tmp`, `${base}.mjs`)
    const deadline = Date.now() + 150_000
    while (Date.now() < deadline) {
      const data = (await Bun.file(`${base}.out`)
        .json()
        .catch(() => undefined)) as { error?: string; value: T } | undefined
      if (data?.error) throw new Error(data.error)
      if (data) return data.value
      if (child.exitCode !== null) throw new Error(`acdert exited with ${child.exitCode} before ${name}`)
      await Bun.sleep(50)
    }
    throw new Error(`Timed out waiting for ${name}`)
  }
  try {
    const id = await call<number>(
      'wait',
      `async (api) => {
      for (let tries = 0; tries < 300; tries++) {
        const app = api.webs().find((web) => web.app === 'dom-perf')
        if (app && await api.web(app.name, 'document.title').catch(() => '') === 'DOM Perf') return app.window
        await api.sleep(50)
      }
      throw new Error('the bench page did not start')
    }`,
    )
    const target = JSON.stringify(`dom-perf#${id}`)
    const result = await call<Result>('bench', `async (api) => api.web(${target}, ${JSON.stringify(workload)})`)
    await call('close', `(api) => { setTimeout(() => api.broker.closeWeb(${id}), 100); return true }`)
    await Promise.race([child.exited, Bun.sleep(8000)])
    return result
  } finally {
    if (child.exitCode === null) child.kill()
    await child.exited
  }
}

const rows: { side: string; round: number; create: number; restyle: number }[] = []
for (let round = 0; round < rounds; round++) {
  for (const side of sides) {
    const result = await session(side, round)
    rows.push({ side: side.name, round, ...result })
    console.log(JSON.stringify(rows[rows.length - 1]))
  }
}

const summary = Object.fromEntries(
  sides.map((side) => {
    const mine = rows.filter((row) => row.side === side.name)
    return [side.name, { create: median(mine.map((row) => row.create)), restyle: median(mine.map((row) => row.restyle)) }]
  }),
)
const base = summary['base']
const candidate = summary['candidate']
const ratios = base && candidate ? { create: candidate.create / base.create, restyle: candidate.restyle / base.restyle } : null
const slower = ratios ? Object.entries(ratios).filter(([, ratio]) => ratio > limit) : []
const report = { summary, ratios, limit, rows }
console.log(JSON.stringify(report, null, 2))
await Bun.write(join(run, 'summary.json'), JSON.stringify(report, null, 2))
if (slower.length) {
  const list = slower.map(([name, ratio]) => `${name} ${ratio.toFixed(3)}x`).join(', ')
  console.error(`DOM perf: candidate is slower than base beyond ${limit}x: ${list}`)
  process.exit(1)
}
