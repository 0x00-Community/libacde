import { readDirOr, readTextOr, mkdirSync, renameSync, stat, writeFileSync } from '../../../page/src/sys/fs'
import { xdgHome } from '../../../page/src/sys/os'
import { dirname, join } from '../../../page/src/sys/path'
import { onceSignal, process } from '../../../page/src/sys/process'
import type { LaunchArgs } from '../args'
import type { NativeDevTools } from '../dev/devtools'
import type { LauncherShell } from '../shell'
import { LocalOpener } from './open'
import { LocalRegistry } from './registry'
import { LocalSession, type LocalHost } from './session'

export interface LocalDesktopParts extends Pick<LocalHost, 'launcher' | 'service'> {
  shell(title: string): LauncherShell
  readonly devtools?: NativeDevTools
  readonly signals: readonly ('SIGTERM' | 'SIGINT' | 'SIGHUP')[]
}

export async function localDesktop(args: LaunchArgs, parts: LocalDesktopParts): Promise<number> {
  const files = { readText: (file: string) => readTextOr(file, null), readDir: readDirOr, isFile: (file: string) => stat(file)?.isFile === true }
  const roots = {
    dev: (process.env['ACDE_PATH'] ?? '').split(':').filter(Boolean),
    system: (process.env['XDG_DATA_DIRS'] || '/usr/local/share:/usr/share').split(':').filter(Boolean),
    user: xdgHome('data'),
  }
  const scan = () => LocalRegistry.scan(files, roots, args.app || null)
  const registry = scan()
  const shell = parts.shell(registry.desktop.desktop.name)
  const data = [xdgHome('data'), ...(process.env['XDG_DATA_DIRS'] || '/usr/local/share:/usr/share').split(':').filter(Boolean)]
  if (process.platform === 'darwin') data.push('/opt/homebrew/share', '/opt/local/share')
  const opener = new LocalOpener({
    readText: (file) => readTextOr(file, null),
    readDir: readDirOr,
    isDirectory: (path) => stat(path)?.isDirectory === true,
    exists: (path) => stat(path) !== null,
    writeText: (file, text) => {
      mkdirSync(dirname(file), { recursive: true })
      writeFileSync(`${file}.new`, text)
      renameSync(`${file}.new`, file)
    },
  }, {
    data,
    config: xdgHome('config'),
    configDirs: (process.env['XDG_CONFIG_DIRS'] || '/etc/xdg').split(':').filter(Boolean),
    applications: data.map((dir) => join(dir, 'applications')),
  })
  const session = new LocalSession({
    shell,
    opener,
    ...(shell.openDefault && { openDefault: (path: string) => shell.openDefault!(path) }),
    ...(parts.devtools && { devtools: parts.devtools }),
    registry,
    rescan: scan,
    launcher: parts.launcher,
    service: parts.service,
  })
  for (const signal of parts.signals) onceSignal(signal, () => session.end(0))
  if (args.seconds !== null) setTimeout(() => session.end(0), args.seconds * 1000)
  if (args.shot) {
    const file = args.shot
    setTimeout(() => {
      shell.screenshot(file).then(
        ({ width, height }) => process.stderr.write(`acdev: screenshot ${width}x${height} -> ${file}\n`),
        (error: Error) => process.stderr.write(`acdev: screenshot: ${error.message}\n`),
      )
    }, args.shotAfterMs)
  }
  return session.run()
}

