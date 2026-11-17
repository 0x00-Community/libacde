import { sys, type FileStat } from './natives'

function enoent(path: string): Error {
  return Object.assign(new Error(`ENOENT: no such file or directory, '${path}'`), { code: 'ENOENT' })
}

export function readFileSync(path: string): Uint8Array
export function readFileSync(path: string, encoding: 'utf8' | 'utf-8' | { encoding: 'utf8' | 'utf-8' }): string
export function readFileSync(path: string, encoding?: unknown): string | Uint8Array {
  return encoding ? sys().readText(path) : sys().readFile(path)
}

export function readTextOr<T>(path: string, fallback: T): string | T {
  try {
    return sys().readText(path)
  } catch {
    return fallback
  }
}

export function readDirOr(path: string): string[] {
  try {
    return sys().readDir(path)
  } catch {
    return []
  }
}

export function writeFileSync(path: string, data: string | Uint8Array, options?: { mode?: number }): void {
  sys().writeFile(path, data)
  if (options?.mode !== undefined) sys().chmod(path, options.mode)
}

export function existsSync(path: string): boolean {
  return sys().exists(path)
}

export function stat(path: string): FileStat | null {
  return sys().stat(path)
}

export function readdirSync(path: string): string[] {
  return sys().readDir(path)
}

export function mkdirSync(path: string, options?: { recursive?: boolean; mode?: number }): void {
  sys().mkdir(path, options?.recursive ?? false)
  if (options?.mode !== undefined) sys().chmod(path, options.mode)
}

export function renameSync(from: string, to: string): void {
  sys().rename(from, to)
}

export function rmSync(path: string, options?: { recursive?: boolean; force?: boolean }): void {
  if (!options?.force && !sys().exists(path)) throw enoent(path)
  sys().remove(path, options?.recursive ?? false)
}
