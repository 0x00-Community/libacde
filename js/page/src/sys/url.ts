import { sys } from './natives'

const windows = (): boolean => sys().platform === 'win32'

export function fileURLToPath(url: string | URL): string {
  const parsed = typeof url === 'string' ? new URL(url) : url
  if (parsed.protocol !== 'file:') throw new TypeError(`not a file URL: ${parsed.href}`)
  const path = decodeURIComponent(parsed.pathname)
  if (!windows()) return path
  if (parsed.hostname) return `//${parsed.hostname}${path}`
  return /^\/[A-Za-z]:(\/|$)/.test(path) ? path.slice(1) : path
}

export function pathToFileURL(path: string): URL {
  const p = windows() ? path.replaceAll('\\', '/') : path
  const encoded = (s: string) => s.split('/').map(encodeURIComponent).join('/')
  if (windows() && /^[A-Za-z]:(\/|$)/.test(p)) return new URL(`file:///${p.slice(0, 2)}${encoded(p.slice(2))}`)
  if (windows() && p.startsWith('//')) return new URL(`file:${encoded(p)}`)
  return new URL(`file://${encoded(p)}`)
}
