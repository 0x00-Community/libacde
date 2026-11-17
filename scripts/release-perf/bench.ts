import assert from 'node:assert/strict'
import { mkdir, readFile, rename } from 'node:fs/promises'
import { join, resolve } from 'node:path'

const artifacts = process.argv[2]
assert.ok(artifacts, 'Run this through scripts/release-perf/bench.sh')
const run = resolve(artifacts)
const hosts = (process.env['BENCH_HOSTS'] ?? 'dist,dist-release').split(',').map((name) => name.trim())
const rounds = Number(process.env['BENCH_ROUNDS'] ?? 5)
const binary = process.platform === 'win32' ? 'acdert.exe' : 'acdert'
const runtime = process.env['ACDEV_RUNTIME'] ?? resolve('target', 'release', binary)
const pkgRoot = join(run, 'package')
await mkdir(pkgRoot, { recursive: true })
await Bun.write(join(pkgRoot, 'package.json'), JSON.stringify({ name: 'perf-probe', acde: {
  apps: [{ id: 'perf-probe', label: 'Perf Probe', icon: 'files', entry: '/perf/app.html', capabilities: {} }],
  assets: { '/perf/': ['.'] },
} }))
await Bun.write(join(pkgRoot, 'app.html'), '<!doctype html><meta charset="utf-8"><title>Perf Probe</title><body>perf</body>')

const workload = `(() => {
  const time = (fn) => { const t = performance.now(); fn(); return performance.now() - t }
  const dom = () => {
    const nodes = []
    for (let i = 0; i < 2000; i++) {
      const d = document.createElement('div')
      d.className = 'item ' + (i % 7)
      d.textContent = 'row ' + i
      document.body.appendChild(d)
      nodes.push(d)
    }
    void document.body.offsetHeight
    for (const n of nodes) n.remove()
  }
  const compute = () => {
    const map = new Map()
    let total = 0
    for (let i = 0; i < 200000; i++) {
      const key = 'k' + (i % 997)
      map.set(key, (map.get(key) || 0) + i)
      total += key.length
    }
    return total + map.size
  }
  const dom5 = [0, 1, 2, 3, 4].map(() => time(dom))
  const compute3 = [0, 1, 2].map(() => time(compute))
  return { dom: dom5, compute: compute3 }
})()`

const median = (values: number[]) => {
  const sorted = [...values].sort((a, b) => a - b)
  return sorted[Math.floor(sorted.length / 2)] ?? 0
}

async function once(host: string, index: number) {
  const dir = join(run, `${host}-${index}`)
  const debug = join(dir, 'debug')
  const home = join(dir, 'home')
  const state = join(dir, 'state')
  await Promise.all([debug, home, state].map((path) => mkdir(path, { recursive: true })))
  const env = { ...process.env }
  delete env['ACDE_STORAGE']
  delete env['ACDE_BACKEND']
  delete env['ACDE_DEV']
  const child = Bun.spawn([runtime, 'app', 'perf-probe', '--seconds', '60'], {
    env: { ...env, ACDE_APP: resolve('web', 'packages', 'acde-host', host), ACDE_PATH: pkgRoot, ACDE_DEBUG_DIR: debug, ACDE_HEADLESS: '1',
      HOME: home, USERPROFILE: home, XDG_CONFIG_HOME: home, XDG_DATA_HOME: home, XDG_STATE_HOME: state,
      ...Object.fromEntries(['DESKTOP', 'DOCUMENTS', 'DOWNLOAD', 'MUSIC', 'PICTURES', 'VIDEOS'].map((n) => [`XDG_${n}_DIR`, join(home, 'places', n)])), },
    stdout: Bun.file(join(dir, 'runtime.stdout.log')),
    stderr: Bun.file(join(dir, 'runtime.stderr.log')),
  })
  async function call<T>(name: string, source: string): Promise<T> {
    const base = join(debug, name)
    await Bun.write(`${base}.tmp`, `export default ${source}`)
    await rename(`${base}.tmp`, `${base}.mjs`)
    const deadline = Date.now() + 30_000
    while (Date.now() < deadline) {
      const data = await Bun.file(`${base}.out`).json().catch(() => undefined) as { error?: string; value: T } | undefined
      if (data?.error) throw new Error(data.error)
      if (data) return data.value
      if (child.exitCode !== null) throw new Error(`acdert exited with ${child.exitCode} before ${name}`)
      await Bun.sleep(50)
    }
    throw new Error(`Timed out waiting for ${name}`)
  }
  let result: { dom: number[]; compute: number[] } = { dom: [], compute: [] }
  try {
    const id = await call<number>('wait', `async (api) => {
      for (let tries = 0; tries < 300; tries++) {
        const app = api.webs().find((web) => web.app === 'perf-probe')
        if (app && await api.web(app.name, 'document.title').catch(() => '') === 'Perf Probe') return app.window
        await api.sleep(50)
      }
      throw new Error('Perf probe did not start')
    }`)
    const target = JSON.stringify(`perf-probe#${id}`)
    result = await call<{ dom: number[]; compute: number[] }>('bench', `async (api) => api.web(${target}, ${JSON.stringify(workload)})`)
    await call('close', `(api) => { setTimeout(() => api.broker.closeWeb(${id}), 100); return true }`)
    await Promise.race([child.exited, Bun.sleep(8000)])
  } finally {
    if (child.exitCode === null) child.kill()
    await child.exited
  }
  const log = await Bun.file(join(dir, 'runtime.stderr.log')).text()
  const marker = (label: string) => {
    const match = new RegExp(`startup: ${label} ms=(\\d+)`).exec(log)
    return match ? Number(match[1]) : null
  }
  return {
    host,
    round: index,
    loadedMs: marker('loaded'),
    shellRunningMs: marker('shell running'),
    domMs: median(result.dom),
    computeMs: median(result.compute),
  }
}

const rows: Awaited<ReturnType<typeof once>>[] = []
for (let round = 0; round < rounds; round++) {
  for (const host of hosts) {
    rows.push(await once(host, round))
    console.log(JSON.stringify(rows[rows.length - 1]))
  }
}

const summary = hosts.map((host) => {
  const mine = rows.filter((row) => row.host === host)
  const loaded = mine.map((row) => row.loadedMs).filter((value): value is number => value !== null)
  const running = mine.map((row) => row.shellRunningMs).filter((value): value is number => value !== null)
  const dom = mine.map((row) => row.domMs)
  const compute = mine.map((row) => row.computeMs)
  return {
    host,
    runs: mine.length,
    loadedMsMedian: median(loaded),
    shellRunningMsMedian: median(running),
    domMsMedian: median(dom),
    computeMsMedian: median(compute),
  }
})
console.log(JSON.stringify({ summary, rows }, null, 2))
await Bun.write(join(run, 'summary.json'), JSON.stringify({ summary, rows }, null, 2))
