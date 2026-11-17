import assert from 'node:assert/strict'
import { mkdir, rename } from 'node:fs/promises'
import { join, resolve } from 'node:path'

const artifacts = process.argv[2]
assert.ok(artifacts, 'Run this audit through scripts/audit-thread-progress.sh')
const run = resolve(artifacts)
assert.equal(process.platform, 'win32', 'The thread audit needs a visible Windows session')
const fixture = join(run, 'fixture')
const debug = join(run, 'debug')
const home = join(run, 'home')
await Promise.all([fixture, debug, home].map((path) => mkdir(path, { recursive: true })))

await Promise.all([
  Bun.write(join(fixture, 'package.json'), JSON.stringify({ name: 'thread-audit', acde: {
    apiLevel: 1,
    desktops: [{ id: 'audit', name: 'Thread audit', entry: 'desktop.html' }],
    apps: [{ id: 'probe', label: 'Thread probe', icon: 'editor', entry: '/audit/app.html',
      size: [500, 550], multiple: true, frame: false }],
    assets: { '/audit/': ['.'] },
  } })),
  Bun.write(join(fixture, 'desktop.html'), '<!doctype html><title>Thread audit</title><body style="background: #123">'),
  Bun.write(join(fixture, 'app.html'), `<!doctype html><title>Probe</title>
<style>body {margin:0;background:#243;color:white} .tile {width:15%;height:12%;position:absolute;
  border-radius:28%;background:linear-gradient(45deg,#e81,#815);filter:blur(8px) drop-shadow(1px 4px 8px #036)}</style>
<div id="label">ready</div><script>
let ticks = 0, frames = 0, input = 0, heavy = false
const tiles = Array.from({length:24}, (_, i) => {
  const tile = document.createElement('div'); tile.className = 'tile'; tile.hidden = true
  tile.style.left = (i % 5) * 18 + '%'; tile.style.top = (i / 5 | 0) * 15 + '%'
  document.body.append(tile); return tile
})
setInterval(() => ticks++, 20)
addEventListener('keyup', () => input++)
window.audit = () => ({ ticks, frames, input, at: Date.now() })
window.load = (value) => { heavy = value; for (const tile of tiles) tile.hidden = !value }
function paint() {
  document.getElementById('label').textContent = String(++frames)
  if (heavy) for (let i = 0; i < tiles.length; i++) tiles[i].style.transform =
    'translateX(' + (12 * Math.sin(frames / 10 + i)) + 'px)'
  requestAnimationFrame(paint)
}
requestAnimationFrame(paint)
</script>`),
])

const env = { ...process.env }
delete env['ACDE_HEADLESS']
delete env['ACDE_DEV']
delete env['ACDE_BACKEND']
const runtime = process.env['ACDEV_RUNTIME'] ?? resolve('target', 'release', 'acdert.exe')
const host = process.env['ACDE_APP'] ?? resolve('web/packages/acde-host/dist')
const child = Bun.spawn([runtime, 'desktop', 'audit', '--seconds', '75'], {
  env: { ...env, ACDE_PATH: fixture, ACDE_APP: host,
    ACDE_DEBUG_DIR: debug, ACDE_SIZE: '1120x720', ACDE_STORAGE: '', HOME: home, USERPROFILE: home,
    XDG_CONFIG_HOME: home, XDG_STATE_HOME: home, XDG_DATA_HOME: home, XDG_RUNTIME_DIR: home,
    ...Object.fromEntries(['DESKTOP', 'DOCUMENTS', 'DOWNLOAD', 'MUSIC', 'PICTURES', 'VIDEOS'].map((n) => [`XDG_${n}_DIR`, join(home, 'places', n)])),
    RUST_LOG: 'info,perf::raster=debug,perf::task=debug' },
  stdout: Bun.file(join(run, 'runtime.stdout.log')),
  stderr: Bun.file(join(run, 'runtime.stderr.log')),
})
console.log(`Runtime PID ${child.pid}`)
const ps = ['pwsh', '-NoProfile', '-File', 'scripts/audit-thread-win32.ps1', '-ProcessId', String(child.pid)]
const trace = Bun.spawn([...ps, '-Mode', 'Trace'], {
  stdout: Bun.file(join(run, 'threads.jsonl')),
  stderr: Bun.file(join(run, 'threads.stderr.log')),
})

async function call<T>(name: string, source: string, ms = 20_000): Promise<T> {
  const base = join(debug, name)
  await Bun.write(`${base}.tmp`, `export default ${source}`)
  await rename(`${base}.tmp`, `${base}.mjs`)
  const deadline = Date.now() + ms
  while (Date.now() < deadline) {
    const result = await Bun.file(`${base}.out`).json().catch(() => undefined) as
      { value: T; error?: string } | undefined
    if (result?.error) throw new Error(result.error)
    if (result) return result.value
    if (child.exitCode !== null) throw new Error(`Runtime exited ${child.exitCode} while waiting for ${name}`)
    await Bun.sleep(75)
  }
  throw new Error(`Timed out waiting for ${name}`)
}

try {
  const ids = await call<{ a: number; b: number }>('setup', `async (api) => {
    const a = api.broker.launch('probe', null, 'free', [15, 35, 500, 550]).window
    const b = api.broker.launch('probe', null, 'free', [545, 35, 500, 550]).window
    for (let attempt = 0; attempt < 35; attempt++) {
      const snapshots = await Promise.all([a,b].map((id) => api.web('probe#' + id, 'window.audit()').catch(() => null)))
      if (snapshots.every((state) => state?.frames > 1) && api.shell.stats().frames > 0) return { a, b }
      await api.sleep(200)
    }
    throw new Error('Both windows must present frames before applying load')
  }`)
  const a = `probe#${ids.a}`, b = `probe#${ids.b}`
  const js = await call<Record<string, unknown>>('busy-js', `async (api) => {
    const probe = async () => ({ ...(await api.web('${b}', 'window.audit()')), composed: api.shell.stats().frames })
    const before = await probe()
    const busy = api.web('${a}', '(() => {' +
      'const from=Date.now(); const end=performance.now()+5500;' +
      'while(performance.now()<end) {} return {from,to:Date.now()}; })()')
    await api.sleep(350)
    api.shell.focus(${ids.b})
    api.shell.inject({type:'keydown',code:30}); api.shell.inject({type:'keyup',code:30})
    const samples = []
    for (let i=0; i<7; i++) { await api.sleep(500); samples.push(await probe()) }
    const span = await busy
    const within = samples.filter((sample) => sample.at > span.from && sample.at < span.to)
    const during = within.at(-1)
    if (!during || during.input <= before.input || during.ticks <= before.ticks ||
        during.frames <= before.frames || during.composed <= before.composed) {
      throw new Error('Independent input or frames stalled')
    }
    return { before, span, samples, during }
  }`)
  console.log(`Busy JS: ${JSON.stringify(js)}`)

  const render = await call<Record<string, unknown>>('render-load', `async (api) => {
    await api.web('${a}', 'window.load(true)')
    await api.sleep(500)
    const probe = async () => ({ ...(await api.web('${b}', 'window.audit()')), composed: api.shell.stats().frames })
    const before = await probe()
    const jobs = Array.from({length:6}, (_, i) => {
      const from = Date.now()
      return api.shell.screenshot(${JSON.stringify(join(run, 'screenshot-'))} + i + '.png')
        .then((size) => ({from,to:Date.now(),size}))
    })
    jobs.push(...Array.from({length:4}, () => {
      const from = Date.now()
      return api.shell.capture(${ids.a})
        .then((shot) => ({from,to:Date.now(),size:{width:shot.width,height:shot.height}}))
    }))
    const samples = []
    for (let i=0; i<10; i++) {
      const start=Date.now(); samples.push({ ...await probe(), rpcMs: Date.now()-start }); await api.sleep(100)
    }
    const jobsDone = await Promise.all(jobs)
    const end = Math.max(...jobsDone.map((job) => job.to))
    const during = samples.filter((sample) => sample.at > before.at && sample.at < end).at(-1)
    if (!during || during.ticks <= before.ticks) throw new Error('The independent isolate did not tick during readback')
    await api.web('${a}', 'window.load(false)')
    return { before, jobsDone, samples, during }
  }`)
  console.log(`Render load: ${JSON.stringify(render)}`)

  const nativeResize = Bun.spawn([...ps, '-Mode', 'Resize'], { stdout: 'pipe', stderr: 'inherit' })
  assert.equal(await nativeResize.exited, 0, 'The native window must be resizable')
  const resizeStarted = Number(await new Response(nativeResize.stdout).text())
  assert.ok(resizeStarted > 0, 'The Win32 resize probe must report a timestamp')
  const nativeSize = await call('resize', `async (api) => {
    for (let attempt = 0; attempt < 40; attempt++) {
      const desktop = api.shell.windows().find((w) => w.window === 0)
      if (desktop?.width < 1050 && desktop?.width > 500) return [desktop.width, desktop.height]
      await api.sleep(50)
    }
    throw new Error('The compositor did not receive the new native size')
  }`)
  console.log(`Native resize: ${JSON.stringify({ size: nativeSize, elapsedMs: Date.now() - resizeStarted })}`)

  const lifecycle = await call<Record<string, unknown>>('lifecycle', `async (api) => {
    const before = api.shell.windows().find((w) => w.window === ${ids.a})
    await api.web('${a}', 'import("acde:window").then((m)=>m.default.requestState("fullscreen"))')
    await api.sleep(400)
    const resized = api.shell.windows().find((w) => w.window === ${ids.a})
    if (!before || !resized || resized.width <= before.width) {
      throw new Error('Fullscreen resize did not reach the surface')
    }
    const capture = api.shell.screenshot(${JSON.stringify(join(run, 'lifecycle.png'))})
    const longTask = api.web('${b}', '(() => {' +
      'const end=performance.now()+6500; while(performance.now()<end) {} })()').catch(() => null)
    await api.sleep(350)
    const started = Date.now()
    const close = await api.broker.closeWeb(${ids.b})
    const closeMs = Date.now()-started
    await Promise.all([capture, longTask])
    if (!close || closeMs >= 6000 || api.broker.windows.some((w)=>w.id === ${ids.b})) {
      throw new Error('Closing the busy app did not complete')
    }
    const reloadStarted = Date.now()
    await api.broker.devReload('probe')
    const replacement = api.webs().find((w) => w.app === 'probe')
    if (!replacement) throw new Error('Reload did not create a replacement window')
    await api.sleep(350)
    const result = await api.web(replacement.name, 'window.audit()')
    return { before, resized, closeMs, reloadMs:Date.now()-reloadStarted, replacement, result }
  }`)
  console.log(`Lifecycle: ${JSON.stringify(lifecycle)}`)
  const close = Bun.spawn(['pwsh', '-NoProfile', '-Command',
    `if (-not (Get-Process -Id ${child.pid}).CloseMainWindow()) { exit 1 }`], { stdout: 'ignore', stderr: 'inherit' })
  assert.equal(await close.exited, 0, 'A real Windows host window must accept a close request')
  const status = await Promise.race([child.exited, Bun.sleep(8000).then(() => null)])
  assert.equal(status, 0, 'Host must exit cleanly after native close')
  assert.equal(await trace.exited, 0, 'Windows thread sampling must exit cleanly')
  const snapshots = await Bun.file(join(run, 'threads.jsonl')).text()
  assert.ok(snapshots.includes('cpuMs'), 'Windows thread sampling must record thread activity')
  console.log('Native host close exited with status 0')
} finally {
  if (child.exitCode === null) child.kill()
  await child.exited
  if (trace.exitCode === null) trace.kill()
  await trace.exited
}
