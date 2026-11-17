import { sys } from './natives'

const windows = (): boolean => sys().platform === 'win32'

function split(path: string): [root: string, rest: string] {
  const p = windows() ? path.replaceAll('\\', '/') : path
  if (windows()) {
    const drive = /^[A-Za-z]:(\/|$)/.exec(p)
    if (drive) return [`${p.slice(0, 2)}/`, p.slice(drive[0].length)]
    const unc = /^\/\/[^/]+\/[^/]+(\/|$)/.exec(p)
    if (unc) return [unc[0].endsWith('/') ? unc[0] : `${unc[0]}/`, p.slice(unc[0].length)]
  }
  return p.startsWith('/') ? ['/', p.slice(1)] : ['', p]
}

export function normalize(path: string): string {
  if (!path) return '.'
  const [root, rest] = split(path)
  const out: string[] = []
  for (const part of rest.split('/')) {
    if (!part || part === '.') continue
    if (part === '..') {
      if (out.length && out[out.length - 1] !== '..') out.pop()
      else if (!root) out.push('..')
    } else out.push(part)
  }
  const joined = out.join('/')
  const trailing = rest.endsWith('/') && joined ? '/' : ''
  return root ? `${root}${joined}${trailing}` : (joined || '.') + trailing
}

export function join(...parts: string[]): string {
  return normalize(parts.filter((p) => p !== '').join('/'))
}

export function isAbsolute(path: string): boolean {
  return split(path)[0] !== ''
}

export function resolve(...parts: string[]): string {
  let path = ''
  for (let i = parts.length - 1; i >= 0 && !isAbsolute(path); i--) path = parts[i] ? `${parts[i]}/${path}` : path
  if (!isAbsolute(path)) path = `${sys().cwd()}/${path}`
  const out = normalize(path)
  return out.length > split(out)[0].length && out.endsWith('/') ? out.slice(0, -1) : out
}

export function dirname(path: string): string {
  const [root, rest] = split(path)
  const trimmed = rest.replace(/\/+$/, '')
  const at = trimmed.lastIndexOf('/')
  if (at < 0) return root || '.'
  return root + trimmed.slice(0, at)
}

export function basename(path: string, ext?: string): string {
  const name = split(path)[1].replace(/\/+$/, '').split('/').pop() ?? ''
  return ext && name.endsWith(ext) && name !== ext ? name.slice(0, -ext.length) : name
}
