import assert from 'node:assert/strict'
import { existsSync, readdirSync } from 'node:fs'
import { copyFile, mkdir, rename } from 'node:fs/promises'
import { basename, dirname, join, resolve } from 'node:path'

assert.equal(process.platform, 'win32', 'This check covers the Windows app backends')
const artifacts = process.argv[2]
assert.ok(artifacts, 'Run this check through scripts/check-backends.sh')
const run = resolve(artifacts)
const pkg = join(run, 'fixture')
const fixture = import.meta.dir
const runtime = process.env['ACDEV_RUNTIME'] ?? resolve('target', 'release', 'acdert.exe')
const host = process.env['ACDE_APP'] ?? resolve('web', 'packages', 'acde-host', 'dist')
const base = join(process.env['XDG_RUNTIME_DIR'] || process.env['TMP'] || process.env['TEMP'] || '', 'acde', 'b')
const report: Record<string, unknown> = { runtime, base }

interface Session {
  name: string
  acdert: ReturnType<typeof Bun.spawn>
  page: <T>(code: string, ms?: number) => Promise<T>
  call: <T>(source: string, ms?: number) => Promise<T>
  result: Record<string, unknown>
  pids: number[]
}

function shell(script: string): string {
  const result = Bun.spawnSync(['pwsh', '-NoProfile', '-NonInteractive', '-Command', script], { timeout: 30_000 })
  return result.stdout.toString().trim()
}

function alive(pid: number): boolean {
  try {
    return process.kill(pid, 0)
  } catch {
    return false
  }
}

async function waitFor<T>(read: () => T | undefined | Promise<T | undefined>, label: string, ms = 20_000): Promise<T> {
  const deadline = Date.now() + ms
  while (Date.now() < deadline) {
    const value = await read()
    if (value !== undefined) return value
    await Bun.sleep(50)
  }
  throw new Error(`Timed out waiting for ${label}`)
}

async function preload(): Promise<string> {
  const given = process.env['ACDE_LISTEN']
  if (given) return Bun.file(given).text()
  const source = join(process.env['ACDE_SRC'] ?? resolve('..', 'acde'), 'src', 'listen.ts')
  assert.ok(existsSync(source), `No ${source}: check out acde beside acdert, or set ACDE_SRC or ACDE_LISTEN`)
  const built = await Bun.build({ entrypoints: [source], target: 'node', format: 'esm', minify: { syntax: true } })
  assert.ok(built.success && built.outputs[0], `Bundling ${source} failed: ${built.logs.join('\n')}`)
  return built.outputs[0].text()
}

async function writeFixture(): Promise<void> {
  const listen = await preload()
  assert.match(listen, /acde-listen-channel\/1/, 'The preload does not support the Windows channel')
  const apps = [['node', 'node.js', listen], ['bun', 'bun.js', listen], ['old', 'node.js', 'export {}\n']] as const
  for (const [id, server, text] of apps) {
    const dir = join(pkg, id)
    await mkdir(join(dir, 'backend'), { recursive: true })
    await Bun.write(join(dir, 'index.html'), '<!doctype html><title>Backend check</title><script src="scenario.js"></script>')
    await copyFile(join(fixture, 'scenario.js'), join(dir, 'scenario.js'))
    await copyFile(join(fixture, server), join(dir, 'backend', 'server.js'))
    await Bun.write(join(dir, 'backend', 'package.json'), '{ "type": "module" }\n')
    await Bun.write(join(dir, 'backend', 'acde-listen.js'), text)
  }
  await Bun.write(join(pkg, 'package.json'), JSON.stringify({
    name: 'backend-check',
    acde: {
      apiLevel: 1,
      apps: apps.map(([id]) => ({
        id: `backend-${id}`, label: `Backend ${id}`, icon: 'app', entry: `/${id}/index.html`,
        backend: { entry: `/${id}/backend/server.js`, port: 3000 },
      })),
      assets: Object.fromEntries(apps.map(([id]) => [`/${id}/`, [id]])),
    },
  }))
}

function pipes(): Set<string> {
  return new Set(readdirSync('\\\\.\\pipe\\'))
}

function endpoints(pids: number[]): { tcp: number; udp: number } {
  const list = pids.join(',')
  const counts = shell(`$p = @(${list}); [pscustomobject]@{
    tcp = @(Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object OwningProcess -in $p).Count
    udp = @(Get-NetUDPEndpoint -ErrorAction SilentlyContinue | Where-Object OwningProcess -in $p).Count } | ConvertTo-Json -Compress`)
  return JSON.parse(counts)
}

function owned(names: Iterable<string>, pids: number[]): string[] {
  const marks = pids.flatMap((pid) => [String(pid), pid.toString(16)])
  return [...names].filter((name) => name.split(/[^0-9a-fA-F]+/).some((part) => marks.includes(part.toLowerCase())))
}

function sddl(path: string): string {
  return shell(`(Get-Acl -LiteralPath '${path}').Sddl`)
}

function aces(text: string): string[] {
  return (text.match(/\([^)]*\)/g) ?? []).sort()
}

const sid = shell('[Security.Principal.WindowsIdentity]::GetCurrent().User.Value')
assert.match(sid, /^S-1-5-/, 'Cannot read the current user SID')

async function session(name: string, prefer: string, body: (s: Session) => Promise<void>): Promise<void> {
  const app = `backend-${name}`
  const dir = join(run, name)
  const debug = join(dir, 'debug')
  const home = join(dir, 'home')
  await Promise.all([debug, home].map((path) => mkdir(path, { recursive: true })))
  const result: Record<string, unknown> = {}
  report[name] = result
  let calls = 0
  const call = async <T>(source: string, ms = 30_000): Promise<T> => {
    const file = join(debug, `c${++calls}`)
    await Bun.write(`${file}.tmp`, `export default ${source}`)
    await rename(`${file}.tmp`, `${file}.mjs`)
    const got = await waitFor(async () => {
      const out = Bun.file(`${file}.out`)
      if (!(await out.exists())) return undefined
      return out.json().catch(() => undefined) as Promise<{ value: T; error?: string } | undefined>
    }, `debug call ${calls}`, ms)
    if (got.error) throw new Error(got.error)
    return got.value
  }
  const web = `api.webs().find((w) => w.name.startsWith('${app}#'))`
  const page = <T>(code: string, ms?: number) => call<T>(`async (api) => api.web(${web}.name, ${JSON.stringify(code)})`, ms)
  const env: Record<string, string | undefined> = { ...process.env }
  delete env['ACDE_BACKEND']
  delete env['ACDE_DEV']
  const acdert = Bun.spawn([runtime, 'app', app], {
    env: {
      ...env, ACDE_RUNTIME: prefer, ACDE_APP: host, ACDE_PATH: pkg, ACDE_HEADLESS: '1', ACDE_SIZE: '800x600', ACDE_SCALE: '1', ACDE_STORAGE: '',
      ACDE_DEBUG_DIR: debug, HOME: home, XDG_CONFIG_HOME: home, XDG_DATA_HOME: home, XDG_STATE_HOME: home, XDG_CACHE_HOME: home,
      ...Object.fromEntries(['DESKTOP', 'DOCUMENTS', 'DOWNLOAD', 'MUSIC', 'PICTURES', 'VIDEOS'].map((n) => [`XDG_${n}_DIR`, join(home, 'places', n)])),
    },
    stdout: Bun.file(join(dir, 'acdert.stdout.log')),
    stderr: Bun.file(join(dir, 'acdert.stderr.log')),
  })
  const s: Session = { name, acdert, page, call, result, pids: [] }
  try {
    await call(`async (api) => { for (let i = 0; i < 200; i++) { if (${web}) return true; await api.sleep(100) } throw new Error('no web') }`)
    await waitFor(async () => ((await page('typeof window.runScenario')) === 'function' ? true : undefined), 'the page')
    await body(s)
  } finally {
    if (acdert.exitCode === null) acdert.kill()
    await acdert.exited
    for (const pid of s.pids) if (alive(pid)) process.kill(pid)
  }
}

const text = (path: string, method = 'GET') =>
  `fetch('${path}', { method: '${method}' }).then((r) => r.ok ? r.text() : Promise.reject(new Error('status ' + r.status)))`

function scenarioPassed(name: string, outcome: Record<string, { ok: boolean; error?: string }>): void {
  const failed = Object.entries(outcome).filter(([, step]) => !step.ok)
  assert.deepEqual(failed, [], `${name}: ${JSON.stringify(failed)}`)
}

async function closeWindow(s: Session, pids: number[]): Promise<void> {
  const begun = Date.now()
  await s.call(`async (api) => { const w = api.shell.windows()[0]; setTimeout(() => api.broker.closeWeb(w.window), 100); return true }`)
  const timeout = new Promise<string>((done) => setTimeout(done, 10_000, 'timeout').unref())
  assert.equal(await Promise.race([s.acdert.exited, timeout]), 0, `${s.name}: acdert did not exit with 0 after its last window closed`)
  s.result['closeToExitMs'] = Date.now() - begun - 100
  await waitFor(() => (pids.some(alive) ? undefined : true), `${s.name}: the backend processes to end`, 5000)
}

async function backend(name: string, prefer: string, full: boolean): Promise<void> {
  const stale = join(base, `${Bun.spawnSync(['cmd', '/c', 'exit']).pid}.${Math.floor(Math.random() * 0xffffff).toString(16).padStart(6, '0')}`)
  if (full) {
    await mkdir(stale, { recursive: true })
    await Bun.write(join(stale, '3000.sock'), '')
  }
  await session(name, prefer, async (s) => {
    const before = pipes()
    const pid = Number(await s.page(text('/api/pid')))
    const transport = JSON.parse(await s.page<string>(text('/api/env')))
    const child = Number(await s.page(text('/api/child', 'POST')))
    s.pids.push(pid, child)
    s.result['transport'] = transport
    scenarioPassed(name, await s.page('runScenario(' + full + ')', 120_000))
    const opened = endpoints([s.acdert.pid, pid, child])
    const newPipes = [...pipes()].filter((pipe) => !before.has(pipe))
    const ours = owned(newPipes, [s.acdert.pid, pid, child])
    s.result['endpoints'] = { ...opened, newPipes, ours }
    assert.deepEqual({ ...opened, ours }, { tcp: 0, udp: 0, ours: [] }, `${name}: open endpoints`)
    let generation: string | undefined
    if (full) {
      generation = transport.sockets as string
      assert.deepEqual([transport.channel, transport.control], [null, 'fd'])
      assert.equal(dirname(generation).toLowerCase(), base.toLowerCase(), `${name}: the socket directory is not under ${base}`)
      assert.ok(Buffer.byteLength(join(generation, '65535.sock')) < 108)
      assert.ok(!existsSync(stale), `${name}: ${stale} was not swept`)
      const dirAcl = sddl(generation)
      const sockAcl = sddl(join(generation, '3000.sock'))
      s.result['acl'] = { dir: dirAcl, socket: sockAcl }
      assert.ok(dirAcl.includes('D:P'), `${name}: the directory inherits: ${dirAcl}`)
      assert.deepEqual(aces(dirAcl), [`(A;OICI;FA;;;${sid})`, '(A;OICI;FA;;;SY)'].sort(), dirAcl)
      assert.deepEqual(aces(sockAcl), [`(A;ID;FA;;;${sid})`, '(A;ID;FA;;;SY)'].sort(), sockAcl)
    } else {
      assert.deepEqual(transport, { sockets: null, channel: 'fd', control: null })
    }
    process.kill(pid)
    const again = Number(await s.page(text('/api/pid'), 30_000))
    s.result['restart'] = { from: pid, to: again }
    assert.ok(again > 0 && again !== pid, `${name}: the backend was not started again`)
    await waitFor(() => (alive(child) ? undefined : true), `${name}: the killed backend's child to end`, 5000)
    if (generation) {
      assert.ok(!existsSync(generation), `${name}: the old generation directory is still there`)
      assert.notEqual(JSON.parse(await s.page<string>(text('/api/env'))).sockets, generation)
    }
    scenarioPassed(`${name} after restart`, await s.page('runScenario(' + full + ')', 120_000))
    const child2 = Number(await s.page(text('/api/child', 'POST')))
    const second = full ? JSON.parse(await s.page<string>(text('/api/env'))).sockets as string : undefined
    s.pids.push(again, child2)
    const stopMs = await s.call<number>(`async (api) => {
      const begun = Date.now()
      await api.broker.backends.stop('backend-${name}')
      const ms = Date.now() - begun
      api.broker.backends.start(api.broker.apps.list().find((app) => app.id === 'backend-${name}'))
      return ms
    }`)
    s.result['stopMs'] = stopMs
    assert.ok(stopMs < 3000, `${name}: the backend did not leave on SHUTDOWN (${stopMs} ms)`)
    await waitFor(() => ([again, child2].some(alive) ? undefined : true), `${name}: the stopped backend and its child to end`, 5000)
    if (second) assert.ok(!existsSync(second), `${name}: the stopped generation directory is still there`)
    const third = Number(await s.page(text('/api/pid'), 30_000))
    const child3 = Number(await s.page(text('/api/child', 'POST')))
    s.pids.push(third, child3)
    await closeWindow(s, [third, child3])
    if (full) assert.deepEqual(readdirSync(base).filter((entry) => entry.startsWith(`${s.acdert.pid}.`)), [], `${name}: generation directories left`)
    const log = await Bun.file(join(run, name, 'acdert.stderr.log')).text()
    assert.match(log, /starting it again/, `${name}: no restart in the log`)
    const closed = s.result['closeToExitMs']
    console.log(`${name}: scenario passed twice, no TCP / UDP / named pipe, restarted ${pid} -> ${again}, stopped in ${stopMs} ms, closed in ${closed} ms`)
  })
}

try {
  await mkdir(pkg, { recursive: true })
  await writeFixture()
  await backend('node', 'node', false)
  await backend('bun', 'bun', true)
  await session('old', 'node', async (s) => {
    const warned = await waitFor(async () => {
      const log = await Bun.file(join(run, 'old', 'acdert.stderr.log')).text()
      return log.includes('acde-listen.js does not support the Windows channel (built by an older acde: acde build it again)') ? true : undefined
    }, 'the old preload warning', 10_000)
    s.result['warned'] = warned
    await closeWindow(s, [])
    console.log('old: an old preload is refused with the warning')
  })
} finally {
  await Bun.write(join(run, 'report.json'), JSON.stringify(report, null, 2))
  console.log(`Report: ${join(basename(dirname(run)), basename(run), 'report.json')}`)
}
