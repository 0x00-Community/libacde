import { mkdirSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'

const ENTRIES = [['page/src/worker.ts', 'page.js'], ['launcher/src/main.ts', 'launcher.js']] as const

const [out = join(import.meta.dir, 'dist'), version = '0.0.0-dev'] = process.argv.slice(2)
const level = readFileSync(join(import.meta.dir, '../../npm/acde-api/api-level'), 'utf8').trim()
mkdirSync(resolve(out), { recursive: true })
for (const [entry, name] of ENTRIES) {
  const built = Bun.spawnSync([
    process.execPath, 'build', entry, '--format', 'esm', '--target', 'browser', '--minify-whitespace', '--minify-syntax', '--keep-names',
    '--conditions', 'source', '--define', `ACDEV_API_LEVEL=${level}`, '--define', `ACDEV_RUNTIME_VERSION="${version}"`,
    '--outfile', join(resolve(out), name),
  ], { cwd: import.meta.dir, stdout: 'inherit', stderr: 'inherit' })
  if (built.exitCode !== 0) process.exit(built.exitCode ?? 1)
}
