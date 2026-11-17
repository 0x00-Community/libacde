import assert from 'node:assert/strict'
import { mkdir, rename } from 'node:fs/promises'
import { join, resolve } from 'node:path'

const artifacts = process.argv[2]
assert.ok(artifacts, 'Run this check through scripts/check-standalone.sh')
const run = resolve(artifacts)
const pkg = join(run, 'fixture')
const leveled = join(run, 'fixture-level2')
const unversioned = join(run, 'fixture-unversioned')
const debug = join(run, 'debug')
const home = join(run, 'home')
const binary = process.platform === 'win32' ? 'acdert.exe' : 'acdert'
const runtime = process.env['ACDEV_RUNTIME'] ?? resolve('target', 'release', binary)
const host = process.env['ACDE_APP'] ?? resolve('web', 'packages', 'acde-host', 'dist')

interface FixtureState {
  webs: string[]
  title: string
  windows: { window: number; mode: string; width: number; height: number }[]
}

await Promise.all([pkg, leveled, unversioned, debug, home].map((path) => mkdir(path, { recursive: true })))
await Promise.all([
  Bun.write(join(pkg, 'package.json'), JSON.stringify({
    name: 'standalone-smoke',
    acde: {
      apiLevel: 1,
      desktops: [{ id: 'smoke', name: 'Smoke Desktop', entry: 'desktop.html' }],
      apps: [
        {
          id: 'smoke-app', label: 'Smoke App', icon: 'editor',
          entry: '/smoke/app.html', capabilities: {}, services: ['smoke'],
        },
        {
          id: 'terminal-app', label: 'Terminal App', icon: 'term',
          entry: '/smoke/app.html', capabilities: { terminal: true },
        },
      ],
      assets: { '/smoke/': ['.'] },
      services: [{ name: 'smoke', module: 'service.js' }],
    },
  })),
  Bun.write(join(leveled, 'package.json'), JSON.stringify({
    name: 'standalone-level2',
    acde: {
      apiLevel: 2,
      apps: [{ id: 'level2-app', label: 'Level 2 App', icon: 'app', entry: '/level2/app.html' }],
      assets: { '/level2/': ['.'] },
    },
  })),
  Bun.write(join(leveled, 'app.html'), '<!doctype html><title>Level 2</title>'),
  Bun.write(join(unversioned, 'package.json'), JSON.stringify({
    name: 'standalone-unversioned',
    acde: { apps: [{ id: 'old-app', label: 'Old App', icon: 'app', entry: '/old/app.html' }], assets: { '/old/': ['.'] } },
  })),
  Bun.write(join(unversioned, 'app.html'), '<!doctype html><title>Old</title>'),
  Bun.write(join(pkg, 'desktop.html'), '<!doctype html><title>Smoke Desktop</title>'),
  Bun.write(join(pkg, 'app.html'), `<!doctype html><title>Starting</title>
<script type="module">
import smoke from 'acde:smoke'
document.title = await smoke.title()
</script>`),
  Bun.write(join(pkg, 'service.js'), `export default ({ spawn, log }) => {
  const child = spawn([${JSON.stringify(process.execPath)}, '-e', 'await Bun.stdin.text()'])
  log('smoke child ' + child.pid)
  return {
    api: { smoke: { title: () => 'Smoke App' } },
    close: () => {
      child.end()
      log('smoke service closed')
    },
  }
}`),
])

function start(id: string) {
  const env = { ...process.env }
  delete env['ACDE_BACKEND']
  delete env['ACDE_DEV']
  return Bun.spawn([runtime, 'app', id], {
    env: {
      ...env,
      ACDE_APP: host,
      ACDE_PATH: [pkg, leveled, unversioned].join(process.platform === 'win32' ? ';' : ':'),
      ACDE_DESKTOP: 'smoke',
      ACDE_HEADLESS: '1',
      ACDE_SIZE: '800x600',
      ACDE_SCALE: '1',
      ACDE_STORAGE: '',
      ACDE_DEBUG_DIR: debug,
      HOME: home,
      USERPROFILE: home,
      XDG_CONFIG_HOME: home,
      XDG_DATA_HOME: home,
      XDG_STATE_HOME: home,
      XDG_CACHE_HOME: home,
      XDG_RUNTIME_DIR: home,
      ...Object.fromEntries(['DESKTOP', 'DOCUMENTS', 'DOWNLOAD', 'MUSIC', 'PICTURES', 'VIDEOS'].map((n) => [`XDG_${n}_DIR`, join(home, 'places', n)])),
    },
    stdout: Bun.file(join(run, `${id}.stdout.log`)),
    stderr: Bun.file(join(run, `${id}.stderr.log`)),
  })
}

const child = start('smoke-app')

async function waitFor<T>(read: () => Promise<T | undefined>, label: string): Promise<T> {
  const deadline = Date.now() + 12_000
  while (Date.now() < deadline) {
    const result = await read()
    if (result !== undefined) return result
    if (child.exitCode !== null) throw new Error(`acdert exited with ${child.exitCode} before ${label}`)
    await Bun.sleep(50)
  }
  throw new Error(`Timed out waiting for ${label}`)
}

async function call<T>(name: string, source: string): Promise<T> {
  const base = join(debug, name)
  await Bun.write(`${base}.tmp`, `export default ${source}`)
  await rename(`${base}.tmp`, `${base}.mjs`)
  const result = await waitFor(async () => {
    const file = Bun.file(`${base}.out`)
    if (!await file.exists()) return undefined
    return file.json().catch(() => undefined) as Promise<{ value: T; error?: string } | undefined>
  }, name)
  if (result.error) throw new Error(result.error)
  return result.value
}

async function assertExited(app: string, processHandle: ReturnType<typeof start>, code: number): Promise<void> {
  try {
    const timeout = new Promise<null>((done) => setTimeout(done, 8000, null).unref())
    assert.equal(await Promise.race([processHandle.exited, timeout]), code, `${app} did not exit with status ${code}`)
    const log = await Bun.file(join(run, `${app}.stderr.log`)).text()
    assert.match(log, /smoke service closed/)
    const pid = Number(log.match(/smoke child (\d+)/)?.[1])
    assert.ok(Number.isInteger(pid) && pid > 0, 'Service reported an invalid child PID')
    assert.throws(() => process.kill(pid, 0), { code: 'ESRCH' })
    console.log(`${app} exited with ${code}; its desktop service closed and child ${pid} exited`)
  } finally {
    if (processHandle.exitCode === null) processHandle.kill()
    await processHandle.exited
  }
}

try {
  const state = await call<FixtureState>('standalone', `async (api) => {
    for (let attempt = 0; attempt < 40; attempt++) {
      const webs = api.webs().map(({ name }) => name)
      const app = webs.find((name) => name.startsWith('smoke-app#'))
      if (app && await api.web(app, 'document.title').catch(() => '') === 'Smoke App') {
        return { webs, title: 'Smoke App', windows: api.shell.windows() }
      }
      await api.sleep(100)
    }
    throw new Error('App did not read the selected desktop service')
  }`)
  console.log(JSON.stringify(state))
  assert.deepEqual(state.webs.map((name) => name.split('#')[0]), ['smoke-app'])
  assert.equal(state.windows.length, 1)
  const window = state.windows[0]!
  assert.deepEqual([window.mode, window.width, window.height], ['fullscreen', 800, 600])
  const serviceLog = await Bun.file(join(run, 'smoke-app.stderr.log')).text()
  const pid = Number(serviceLog.match(/smoke child (\d+)/)?.[1])
  assert.ok(Number.isInteger(pid) && pid > 0, 'Service reported an invalid child PID')
  assert.ok(process.kill(pid, 0), 'The service child must be running while the app is open')
  const id = window.window
  await Bun.write(join(pkg, 'app.html'), '<!doctype html><title>Reloaded App</title>')
  const reloaded = await call('reload-app', `async (api) => {
    await api.broker.devReload('smoke-app')
    for (let attempt = 0; attempt < 40; attempt++) {
      if (await api.web('smoke-app#${id}', 'document.title').catch(() => '') === 'Reloaded App') {
        return api.shell.windows().map(({ window, mode }) => [window, mode])
      }
      await api.sleep(100)
    }
    throw new Error('App did not reload in its fullscreen window')
  }`)
  assert.deepEqual(reloaded, [[id, 'fullscreen']])
  console.log(`Reloaded the app in fullscreen window ${id}`)
  await call('close-app', `async (api) => { setTimeout(() => api.broker.closeWeb(${id}), 100); return true }`)
  await assertExited('smoke-app', child, 0)
} finally {
  if (child.exitCode === null) child.kill()
  await child.exited
}

await assertExited('missing-app', start('missing-app'), 1)
assert.match(await Bun.file(join(run, 'missing-app.stderr.log')).text(), /no app missing-app/)
await assertExited('old-app', start('old-app'), 1)
assert.match(await Bun.file(join(run, 'old-app.stderr.log')).text(), /old-app: no acde\.apiLevel: .*rebuild it with the current acde \(level 1\)/)
await assertExited('level2-app', start('level2-app'), 1)
assert.match(await Bun.file(join(run, 'level2-app.stderr.log')).text(), /level2-app: needs level 2, runtime is level 1/)
if (process.platform === 'win32') {
  await assertExited('terminal-app', start('terminal-app'), 1)
  const failure = await Bun.file(join(run, 'terminal-app.stderr.log')).text()
  assert.match(failure, /Terminal App needs "terminal" \(unsupported on this platform\)/)
}
