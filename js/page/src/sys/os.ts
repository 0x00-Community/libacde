import { readFileSync } from './fs'
import { sys } from './natives'
import { isAbsolute, join } from './path'
import { process } from './process'

export function homedir(): string {
  return sys().homedir()
}

const XDG_DEFAULTS = { config: '.config', data: '.local/share', state: '.local/state', cache: '.cache' } as const

export function xdgHome(kind: keyof typeof XDG_DEFAULTS): string {
  const set = process.env[`XDG_${kind.toUpperCase()}_HOME`]
  return set && isAbsolute(set) ? set : join(process.env['HOME'] || homedir(), XDG_DEFAULTS[kind])
}

const XDG_SYSTEM = { config: '/etc/xdg', data: '/usr/local/share:/usr/share' } as const

export function xdgDirs(kind: keyof typeof XDG_SYSTEM, sub: string, home = true): string[] {
  const system = (process.env[`XDG_${kind.toUpperCase()}_DIRS`] || XDG_SYSTEM[kind]).split(':').filter(isAbsolute)
  return [...new Set([...(home ? [xdgHome(kind)] : []), ...system].map((dir) => join(dir, sub)))]
}

export function runtimeDir(): string {
  const set = process.env['XDG_RUNTIME_DIR']
  if (set) return set
  if (process.platform === 'linux') return `/run/user/${process.getuid()}`
  const base = process.env['TMPDIR'] || process.env['TEMP'] || process.env['TMP'] || '/tmp'
  if (process.platform === 'win32') return join(base, `acde-${process.env['USERNAME'] || 'user'}`)
  return join(base, `acde-${process.getuid()}`)
}

export function ownRuntimeDir(): string {
  const shared = !!process.env['XDG_RUNTIME_DIR'] || process.platform === 'linux'
  return shared ? join(runtimeDir(), 'acde') : runtimeDir()
}

export function userDir(name: 'PICTURES' | 'DOWNLOAD', fallback: string): string {
  const home = homedir()
  try {
    const dirs = readFileSync(join(xdgHome('config'), 'user-dirs.dirs'), 'utf8')
    const line = new RegExp(`^XDG_${name}_DIR="([^"]+)"`, 'm').exec(dirs)
    if (line) return line[1]!.replace(/^\$HOME/, home)
  } catch {
  }
  return join(home, fallback)
}
