import assert from 'node:assert/strict'
import { mkdir, readdir, rename } from 'node:fs/promises'
import { join, resolve } from 'node:path'

const artifacts = process.argv[2]
assert.ok(artifacts, 'Run this check through scripts/check-file-chooser.sh')
const run = resolve(artifacts)
const pkg = join(run, 'package')
const debug = join(run, 'debug')
const home = join(run, 'home')
const state = join(run, 'state')
const fixtures = join(run, 'fixtures')
const out = join(run, 'out')
await Promise.all([pkg, debug, home, state, fixtures, out].map((path) => mkdir(path, { recursive: true })))
const first = join(fixtures, 'a.txt')
const second = join(fixtures, 'b.txt')
const saved = join(out, 'saved.txt')
await Promise.all([
  Bun.write(first, 'alpha'),
  Bun.write(second, 'beta'),
  Bun.write(join(pkg, 'package.json'), JSON.stringify({ name: 'picker-probe', acde: {
    apiLevel: 1,
    apps: [
      { id: 'picker-probe', label: 'Picker Probe', icon: 'files',
        entry: '/picker/app.html', capabilities: { fs: { roots: [] }, openFiles: true } },
    ],
    assets: { '/picker/': ['.'] },
  } })),
  Bun.write(join(pkg, 'app.html'), '<!doctype html><title>Picker Probe</title>'),
])

const env = { ...process.env }
delete env['ACDE_STORAGE']
delete env['ACDE_BACKEND']
delete env['ACDE_DEV']
const binary = process.platform === 'win32' ? 'acdert.exe' : 'acdert'
const runtime = env['ACDEV_RUNTIME'] ?? resolve('target', 'release', binary)
const host = env['ACDE_APP'] ?? resolve('web/packages/acde-host/dist')
const child = Bun.spawn([runtime, 'app', 'picker-probe', '--seconds', '60'], {
  env: { ...env, ACDE_APP: host, ACDE_PATH: pkg, ACDE_DEBUG_DIR: debug, ACDE_HEADLESS: '1',
    HOME: home, USERPROFILE: home, XDG_CONFIG_HOME: home, XDG_DATA_HOME: home, XDG_STATE_HOME: state,
    ...Object.fromEntries(['DESKTOP', 'DOCUMENTS', 'DOWNLOAD', 'MUSIC', 'PICTURES', 'VIDEOS'].map((n) => [`XDG_${n}_DIR`, join(home, 'places', n)])), },
  stdout: Bun.file(join(run, 'runtime.stdout.log')),
  stderr: Bun.file(join(run, 'runtime.stderr.log')),
})

async function call<T>(name: string, source: string): Promise<T> {
  const base = join(debug, name)
  await Bun.write(`${base}.tmp`, `export default ${source}`)
  await rename(`${base}.tmp`, `${base}.mjs`)
  const deadline = Date.now() + 12_000
  while (Date.now() < deadline) {
    const data = await Bun.file(`${base}.out`).json().catch(() => undefined) as
      { error?: string; value: T } | undefined
    if (data?.error) throw new Error(data.error)
    if (data) return data.value
    if (child.exitCode !== null) throw new Error(`acdert exited with ${child.exitCode} before ${name}`)
    await Bun.sleep(50)
  }
  throw new Error(`Timed out waiting for ${name}`)
}

const pageOpen = `(async () => {
  const handles = await showOpenFilePicker({ multiple: true })
  const texts = []
  for (const handle of handles) texts.push({ name: handle.name, text: await (await handle.getFile()).text() })
  return texts
})()`

try {
  const id = await call<number>('wait-for-app', `async (api) => {
    for (let tries = 0; tries < 40; tries++) {
      const app = api.webs().find((web) => web.app === 'picker-probe')
      if (app && await api.web(app.name, 'document.title').catch(() => '') === 'Picker Probe') return app.window
      await api.sleep(100)
    }
    throw new Error('Picker app did not start')
  }`)
  const app = `picker-probe#${id}`
  const ask = <T>(name: string, answer: string, source: string): Promise<T> => call(name, `async (api) => {
    ${answer}
    return api.web(${JSON.stringify(app)}, ${JSON.stringify(source)})
  }`)

  const answer = `api.shell.answerOpen([${JSON.stringify(first)}, ${JSON.stringify(second)}])`
  const opened = await ask<{ name: string; text: string }[]>('open-many', answer, pageOpen)
  assert.deepEqual(opened, [{ name: 'a.txt', text: 'alpha' }, { name: 'b.txt', text: 'beta' }])
  console.log('A preset list of two files becomes two readable handles, in order')

  const cancelledOpen = await ask<string>('open-cancel', 'api.shell.answerOpen(null)',
    `showOpenFilePicker().then(() => 'opened', (error) => error.name)`)
  assert.equal(cancelledOpen, 'AbortError')
  console.log('A cancelled open rejects with AbortError')

  const unanswered = await ask<string>('open-unanswered', '', `showOpenFilePicker().then(() => 'opened', (error) => error.name)`)
  assert.notEqual(unanswered, 'opened')
  console.log(`Without a preset the open chooser fails in headless runs (${unanswered})`)

  const savedName = await ask<string>('save-answer', `api.shell.answerSave(${JSON.stringify(saved)})`, `(async () => {
    const handle = await showSaveFilePicker({ suggestedName: 'saved.txt', types: [{ description: 'Text', accept: { 'text/plain': ['.txt'] } }] })
    const writer = await handle.createWritable()
    await writer.write('saved by the chooser')
    await writer.close()
    return handle.name
  })()`)
  assert.equal(savedName, 'saved.txt')
  assert.equal(await Bun.file(saved).text(), 'saved by the chooser')
  console.log('A preset save path receives the written file through the returned handle')

  const cancelledSave = await ask<string>('save-cancel', 'api.shell.answerSave(null)',
    `showSaveFilePicker({ suggestedName: 'x.txt' }).then(() => 'saved', (error) => error.name)`)
  assert.equal(cancelledSave, 'AbortError')
  assert.deepEqual(await readdir(out), ['saved.txt'])
  console.log('A cancelled save rejects with AbortError and creates nothing')

  await call('close', `(api) => { setTimeout(() => api.broker.closeWeb(${id}), 100); return true }`)
  const timeout = new Promise<null>((done) => setTimeout(done, 8000, null).unref())
  assert.equal(await Promise.race([child.exited, timeout]), 0)
  console.log('The isolated runtime exited with status 0.')
} finally {
  if (child.exitCode === null) child.kill()
  await child.exited
}
