import { appendFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { parseArgs } from 'node:util'
import { Acde, Png, type Image } from 'acde/driver'
import { compare, fuzzyFor, within } from './reftest'
import { perform, type DriverRequest } from './drive'
import type { Test } from './prepare'

const ROOT = join(import.meta.dir, '..', '..')
const STATE = process.env.WPT_STATE ?? join(ROOT, 'logs', 'wpt')
const { values } = parseArgs({
  options: {
    label: { type: 'string', default: new Date().toISOString().slice(0, 16).replace(/[:T]/g, '-') },
    workers: { type: 'string', default: '3' },
    first: { type: 'string', default: '1' },
    dirs: { type: 'string', default: '' },
    kinds: { type: 'string', default: 'testharness,reftest,crashtest' },
    tests: { type: 'string', default: '' },
    limit: { type: 'string', default: '0' },
    chunk: { type: 'string', default: '40' },
    acdert: { type: 'string', default: process.env.ACDEV_RUNTIME ?? '' },
    app: { type: 'string', default: process.env.ACDE_APP ?? join(ROOT, 'web', 'packages', 'acde-host', 'dist') },
    'timeout-scale': { type: 'string', default: '1' },
    retry: { type: 'boolean', default: false },
    flag: { type: 'string', default: '' },
    rerun: { type: 'boolean', default: false },
    'skip-slow': { type: 'string', default: '' },
    shard: { type: 'string', default: '1/1' },
  },
})
const [SHARD, SHARDS] = values.shard!.split('/').map(Number) as [number, number]
if (!(SHARDS >= 1 && SHARD >= 1 && SHARD <= SHARDS)) throw new Error(`--shard ${values.shard}: want i/n with 1 <= i <= n`)
if (!values.acdert) throw new Error('--acdert <path to acdebin-dbg> (or ACDEV_RUNTIME) is required')
if (!existsSync(join(values.app!, 'broker.js'))) throw new Error(`${values.app}: no broker.js; build acdert's web (cd web && bun run build) or pass --app <acde-host dist>`)
const WORKERS = Number(values.workers)
const FIRST = Number(values.first)
const SCALE = Number(values['timeout-scale'])
const CHUNK = Math.max(1, Number(values.chunk))
const OUT = join(STATE, 'results', values.label!)
const DATA = join(STATE, 'work', 'data', values.label!)
mkdirSync(OUT, { recursive: true })
const RESULTS = join(OUT, 'results.jsonl')
const VIEWPORT: [number, number] = [800, 600]
const LOAD_MS = 6000

export type Sub = { name: string; status: number; message: string | null }
export type Result = { url: string; kind: Test['kind']; status: string; message?: string | null; tests?: Sub[]; ms: number; worker: number; attempt?: number }

const HARNESS = ['OK', 'ERROR', 'TIMEOUT', 'PRECONDITION_FAILED']

function slowOf(dir: string): Set<string> {
  const urls = new Set<string>()
  if (!existsSync(dir)) return urls
  for (const name of readdirSync(dir))
    if (name.endsWith('.json'))
      for (const [url, e] of Object.entries(JSON.parse(readFileSync(join(dir, name), 'utf8')) as Record<string, string | { status: string; flaky?: unknown }>)) {
        const status = typeof e === 'string' ? e : e.status
        if (['TIMEOUT', 'NOLOAD', 'HANG'].includes(status) && !(typeof e === 'object' && e.flaky === true)) urls.add(url)
      }
  return urls
}
const only = values.dirs ? values.dirs.split(',').map((d) => '/' + d.replace(/^\/+|\/+$/g, '') + '/') : null
const kinds = new Set(values.kinds!.split(','))
const chosen = values.tests ? new Set(JSON.parse(readFileSync(values.tests, 'utf8')) as string[]) : null
const all = JSON.parse(readFileSync(join(STATE, 'tests.json'), 'utf8')) as Test[]
const byUrl = new Map(all.map((t) => [t.url, t]))
const slow = values['skip-slow'] ? slowOf(values['skip-slow']) : null
let list = all.filter((t) => !t.unhosted && kinds.has(t.kind) && (!only || only.some((d) => t.url.startsWith(d))) && (!chosen || chosen.has(t.url)) && (!values.flag || t.flags.includes(values.flag)) && !slow?.has(t.url))
if (slow) console.log(`skipping ${slow.size} tests the expectations in ${values['skip-slow']} record as TIMEOUT, NOLOAD or HANG`)

for (let n = FIRST; n < FIRST + WORKERS; n++)
  if (!existsSync(join(STATE, 'work', `app${n}`, 'acde.config.jsonc'))) throw new Error(`${join(STATE, 'work', `app${n}`)} is missing: run scripts/wpt/wpt.sh prepare --first ${FIRST} --workers ${WORKERS}`)

const done = new Map<string, Result>()
if (existsSync(RESULTS))
  for (const line of readFileSync(RESULTS, 'utf8').split('\n'))
    if (line) {
      const r = JSON.parse(line) as Result
      done.set(r.url, r)
    }
list = list.filter((t) => {
  const r = done.get(t.url)
  return !r || values.rerun || (values.retry && (r.status === 'CRASH' || r.status === 'HANG'))
})
if (Number(values.limit) > 0) list = list.slice(0, Number(values.limit))

const groups = new Map<string, Test[]>()
for (const t of list) {
  const key = `${t.kind}:${t.url.split('?')[0]!.split('/').slice(1, -1).slice(0, 2).join('/')}`
  if (!groups.has(key)) groups.set(key, [])
  groups.get(key)!.push(t)
}
const queue: [string, Test[]][] = []
const shardOf = (key: string) => [...key].reduce((h, c) => (Math.imul(h, 31) + c.charCodeAt(0)) >>> 0, 7) % SHARDS
for (const [key, tests] of groups)
  for (let i = 0; i < tests.length; i += CHUNK) {
    const chunk = `${key}#${i / CHUNK}`
    if (shardOf(chunk) === SHARD - 1) queue.push([chunk, tests.slice(i, i + CHUNK)])
  }
queue.sort((a, b) => b[1].length - a[1].length)
const total = queue.reduce((n, [, tests]) => n + tests.length, 0)
let finished = 0
const started = Date.now()
const previous = existsSync(join(OUT, 'meta.json')) ? JSON.parse(readFileSync(join(OUT, 'meta.json'), 'utf8')) : null
const meta = {
  ...(previous ?? {}),
  passes: [...(previous?.passes ?? []), { started: new Date().toISOString(), flag: values.flag || null, rerun: values.rerun, dirs: values.dirs || null, kinds: values.kinds, tests: values.tests || null }],
  label: values.label,
  wpt: readFileSync(join(STATE, 'work', 'wpt-sha'), 'utf8').trim(),
  acdert: values.acdert,
  started: new Date().toISOString(),
  workers: WORKERS,
  dirs: values.dirs || null,
  platform: process.platform,
}
writeFileSync(join(OUT, 'meta.json'), JSON.stringify(meta, null, 2) + '\n')
console.log(`${total} tests in ${queue.length} chunks${SHARDS > 1 ? ` (shard ${SHARD} of ${SHARDS})` : ''}, ${done.size} already done, ${WORKERS} workers -> ${OUT}`)

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))
const gone = (e: unknown) => /closed the connection|no longer connected|not connected|ECONNREFUSED|EPIPE|no web|no page|exited/i.test(String((e as Error)?.message ?? e))
const text = (e: unknown) => String((e as Error)?.message ?? e).slice(0, 500)

class Crash extends Error {}
class Hang extends Error {}

class Worker {
  acde: Acde | null = null
  web = ''
  launches = 0
  viewport: [number, number] | null = null
  shots = new Map<string, Image>()
  constructor(readonly n: number) {}

  async launch(chunk: string) {
    await this.close()
    const data = join(DATA, `w${this.n}`, `${this.launches++}`)
    mkdirSync(data, { recursive: true })
    this.acde = await Acde.launch({
      dirs: [join(STATE, 'work', `app${this.n}`)],
      headless: true,
      force: true,
      timeout: 20000,
      env: {
        ACDEV_RUNTIME: values.acdert!, ACDE_APP: values.app!,
        XDG_DATA_HOME: data, XDG_CONFIG_HOME: join(data, 'config'), XDG_CACHE_HOME: join(data, 'cache'),
      },
    })
    const webs = await this.acde.webs()
    const web = webs.find((w) => w.app === `wpt${this.n}`)
    if (!web) throw new Error(`worker ${this.n}: no page after launch for ${chunk}`)
    this.web = web.name
    this.viewport = null
    this.shots.clear()
  }

  async close() {
    const acde = this.acde
    this.acde = null
    if (!acde) return
    await Promise.race([acde.close().catch(() => 0), sleep(15000)])
  }

  async eval<T>(code: string): Promise<T> {
    return (await this.acde!.evaluate<T>(this.web, code)) as T
  }

  async navigate(url: string) {
    try {
      await this.eval(`window.__wpt = undefined; window.__old = 1; if (location.pathname + location.search === ${JSON.stringify(url)}) location.reload(); else location.href = ${JSON.stringify(url)}; 1`)
    } catch (e) {
      throw gone(e) ? new Crash(`navigate: ${text(e)}`) : new Hang(`navigate: ${text(e)}`)
    }
  }

  async poll<T>(code: string, until: (value: T) => boolean, ms: number): Promise<T | null> {
    const end = Date.now() + ms
    let last: T | null = null
    while (Date.now() < end) {
      try {
        last = await this.eval<T>(code)
        if (until(last)) return last
      } catch (e) {
        if (gone(e)) throw new Crash(text(e))
      }
      await sleep(25)
    }
    return null
  }

  async settle(waitClass: string, budget: number): Promise<'ready' | 'noload' | 'timeout'> {
    const loaded = await this.poll<number>(`window.__old === 1 || document.readyState !== 'complete' ? 0 : 1`, (v) => v === 1, Math.min(LOAD_MS * SCALE, budget))
    if (loaded === null) return 'noload'
    await this.eval(
      `window.__settled = 0; (async () => { try { await document.fonts?.ready } catch {} ; const root = document.documentElement; while (root && root.classList.contains(${JSON.stringify(waitClass)})) await new Promise((r) => setTimeout(r, 20)); await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))); window.__settled = 1 })(); 1`,
    ).catch((e) => {
      if (gone(e)) throw new Crash(text(e))
    })
    const settled = await this.poll<number>(`window.__settled === 1 ? 1 : 0`, (v) => v === 1, budget)
    return settled === null ? 'timeout' : 'ready'
  }

  async size(viewport: [number, number]) {
    if (this.viewport && this.viewport[0] === viewport[0] && this.viewport[1] === viewport[1]) return
    await this.acde!.resize(this.web, viewport[0], viewport[1])
    this.viewport = viewport
    this.shots.clear()
  }

  async shot(url: string, budget: number): Promise<Image | 'noload' | 'timeout'> {
    await this.navigate(url)
    const state = await this.settle('reftest-wait', budget)
    if (state !== 'ready') return state
    return Png.decode((await this.acde!.screenshot(this.web)).png)
  }

  async reference(url: string, budget: number): Promise<Image | 'noload' | 'timeout'> {
    const cached = this.shots.get(url)
    if (cached) return cached
    const image = await this.shot(url, budget)
    if (typeof image !== 'string') this.shots.set(url, image)
    return image
  }

  async reftest(t: Test, budget: number, res: (status: string, extra?: Partial<Result>) => Result): Promise<Result> {
    await this.size(t.viewport ?? VIEWPORT)
    const test = await this.shot(t.url, budget)
    if (test === 'noload') return res('NOLOAD', { message: 'the test page never loaded' })
    if (test === 'timeout') return res('TIMEOUT', { message: 'reftest-wait was never removed' })
    const failures: string[] = []
    for (const ref of t.references ?? []) {
      const chain = await this.matches(t, test, ref.url, ref.relation, budget, new Set([t.url]))
      if (chain === null) return res('PASS')
      failures.push(chain)
    }
    return res('FAIL', { message: failures.join('; ').slice(0, 2000) })
  }

  async matches(t: Test, image: Image, ref: string, relation: '==' | '!=', budget: number, seen: Set<string>): Promise<string | null> {
    const other = await this.reference(ref, budget)
    if (other === 'noload') return `${ref}: the reference never loaded`
    if (other === 'timeout') return `${ref}: reftest-wait was never removed`
    const diff = compare(image, other)
    const same = !diff.size && within(diff, fuzzyFor(t, ref))
    if (relation === '==' && !same) return `${ref}: ${diff.size ?? `${diff.totalPixels} pixels differ, by up to ${diff.maxDifference}`}`
    if (relation === '!=' && same) return `${ref}: matches, but it must not`
    const next = byUrl.get(ref)
    if (next?.kind === 'reftest' && next.references?.length && !seen.has(ref)) {
      seen.add(ref)
      for (const deeper of next.references) {
        const why = await this.matches(next, other, deeper.url, deeper.relation, budget, seen)
        if (why !== null) return `${ref} -> ${why}`
      }
    }
    return null
  }

  async crashtest(t: Test, budget: number, res: (status: string, extra?: Partial<Result>) => Result): Promise<Result> {
    await this.navigate(t.url)
    const state = await this.settle('test-wait', budget)
    if (state === 'noload') return res('NOLOAD', { message: 'the test page never loaded' })
    if (state === 'timeout') return res('TIMEOUT', { message: 'test-wait was never removed' })
    try {
      await this.eval('1')
    } catch (e) {
      return res(gone(e) ? 'CRASH' : 'ERROR', { message: text(e) })
    }
    return res('PASS')
  }

  async testharness(t: Test, budget: number, t0: number, res: (status: string, extra?: Partial<Result>) => Result): Promise<Result> {
    await this.navigate(t.url)
    let loaded = false
    let timedOut = false
    while (true) {
      await sleep(loaded ? 40 : 25)
      let got: unknown
      try {
        got = await this.eval(`window.__old === 1 ? 0 : typeof window.__wpt === 'string' ? window.__wpt : window.__wptdrv && window.__wptdrv.queue.length ? { drive: window.__wptdrv.queue.splice(0) } : 1`)
      } catch (e) {
        if (gone(e)) return res('CRASH', { message: text(e) })
        if (Date.now() - t0 > budget + 8000) return res('HANG', { message: text(e) })
        continue
      }
      if (got === 1) loaded = true
      if (!loaded && got === 0 && Date.now() - t0 > Math.min(LOAD_MS * SCALE, budget)) return res('NOLOAD', { message: 'the test page never loaded' })
      if (got && typeof got === 'object' && 'drive' in got) {
        loaded = true
        for (const req of (got as { drive: DriverRequest[] }).drive) {
          const error = await perform(this.acde!, this.web, req)
          try {
            await this.eval(`window.__wptdrv && window.__wptdrv.done(${req.id}, ${JSON.stringify(error)}); 1`)
          } catch (e) {
            if (gone(e)) return res('CRASH', { message: text(e) })
          }
        }
        continue
      }
      if (typeof got === 'string') {
        const r = JSON.parse(got) as { status: number; message: string | null; stack: string | null; tests: Sub[] }
        return res(timedOut ? 'TIMEOUT' : (HARNESS[r.status] ?? `STATUS${r.status}`), { message: r.message, tests: r.tests })
      }
      if (!timedOut && Date.now() - t0 > budget) {
        timedOut = true
        try {
          await this.eval(`typeof timeout === 'function' ? (timeout(), 1) : 0`)
        } catch (e) {
          if (gone(e)) return res('CRASH', { message: text(e) })
          return res('HANG', { message: `timeout(): ${text(e)}` })
        }
      }
      if (timedOut && Date.now() - t0 > budget + 3000) return res('TIMEOUT', { message: 'no completion after timeout()' })
    }
  }

  async runOne(t: Test): Promise<Result> {
    const t0 = Date.now()
    const budget = (t.long ? 60000 : 10000) * SCALE
    const res = (status: string, extra: Partial<Result> = {}): Result => ({ url: t.url, kind: t.kind, status, ms: Date.now() - t0, worker: this.n, ...extra })
    try {
      if (t.kind === 'reftest') return await this.reftest(t, budget, res)
      if (t.kind === 'crashtest') return await this.crashtest(t, budget, res)
      return await this.testharness(t, budget, t0, res)
    } catch (e) {
      if (e instanceof Crash) return res('CRASH', { message: e.message })
      if (e instanceof Hang) return res('HANG', { message: e.message })
      return res(gone(e) ? 'CRASH' : 'ERROR', { message: text(e) })
    }
  }

  async work() {
    let failures = 0
    while (queue.length) {
      const [key, tests] = queue.shift()!
      try {
        await this.launch(key)
        failures = 0
      } catch (e) {
        const message = String((e as Error)?.message ?? e)
        console.error(`worker ${this.n}: launch failed for ${key}: ${message.split('\n').slice(0, 12).join('\n')}`)
        queue.push([key, tests])
        if (++failures >= 3) {
          console.error(`worker ${this.n}: stops after ${failures} launch failures in a row`)
          return
        }
        continue
      }
      for (const t of tests) {
        let r = await this.runOne(t)
        if (r.status === 'CRASH' || r.status === 'HANG') {
          try {
            await this.launch(key)
            const again = await this.runOne(t)
            r = { ...again, attempt: 2, message: again.status === r.status ? `${r.message} | retry: ${again.message}` : again.message }
            if (again.status === 'CRASH' || again.status === 'HANG') await this.launch(key)
          } catch (e) {
            console.error(`worker ${this.n}: relaunch failed in ${key}: ${String((e as Error)?.message ?? e).split('\n')[0]}`)
          }
        }
        record(r)
      }
      await this.close()
      rmSync(join(DATA, `w${this.n}`), { recursive: true, force: true })
    }
  }
}

function record(r: Result) {
  appendFileSync(RESULTS, JSON.stringify(r) + '\n')
  finished++
  if (finished % 100 === 0 || finished === total) {
    const rate = finished / ((Date.now() - started) / 1000)
    console.log(`${finished}/${total} (${rate.toFixed(1)}/s, eta ${Math.round((total - finished) / rate / 60)} min)`)
  }
}

const SERVE = JSON.parse(readFileSync(join(STATE, 'work', 'serve.json'), 'utf8')) as { wpt: string; http: number }
const BIND = (http: number) => ({ 80: http, 8001: http + 1, 8443: http + 2, 8444: http + 3, 8002: http + 4, 8445: http + 5 })

async function wptserve(): Promise<() => void> {
  const http = SERVE.http
  const config = join(STATE, 'work', 'wptserve.json')
  writeFileSync(config, JSON.stringify({ browser_host: 'acde.local', server_host: '127.0.0.1', alternate_hosts: {}, check_subdomains: false, ports: { http: [80, 8001], https: [8443, 8444], ws: [8002], wss: [8445], h2: [9000] } }) + '\n')
  const log = Bun.file(join(OUT, 'wptserve.log'))
  const child = Bun.spawn([join(SERVE.wpt, '_venv3', 'bin', 'python'), join(import.meta.dir, 'serve.py'), SERVE.wpt, config, JSON.stringify(BIND(http))], { stdout: log, stderr: log, detached: true })
  for (let i = 0; i < 600; i++) {
    if (child.exitCode !== null) throw new Error(`wptserve exited (${child.exitCode}); see ${join(OUT, 'wptserve.log')}`)
    const up = await fetch(`http://127.0.0.1:${http}/resources/testharness.js`, { method: 'HEAD' }).then((r) => r.ok, () => false)
    if (up) {
      console.log(`wptserve on 127.0.0.1:${http} (advertised as acde.local:80), log ${join(OUT, 'wptserve.log')}`)
      return () => {
        try {
          process.kill(-child.pid, 'SIGINT')
        } catch {}
      }
    }
    await sleep(100)
  }
  child.kill()
  throw new Error(`wptserve did not answer on ${http} within 60 s; see ${join(OUT, 'wptserve.log')}`)
}

const stopServe = await wptserve()
const workers = Array.from({ length: WORKERS }, (_, i) => new Worker(FIRST + i))
const stop = async () => {
  await Promise.all(workers.map((w) => w.close()))
  stopServe()
  process.exit(130)
}
process.on('SIGINT', stop)
process.on('SIGTERM', stop)
await Promise.all(workers.map((w) => w.work()))
stopServe()
writeFileSync(join(OUT, 'meta.json'), JSON.stringify({ ...meta, finished: new Date().toISOString(), seconds: Math.round((Date.now() - started) / 1000) }, null, 2) + '\n')
console.log(`done in ${Math.round((Date.now() - started) / 1000)} s`)
