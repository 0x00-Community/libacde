import { readFileSync, stat } from './sys/fs'
import { dirname, join, normalize } from './sys/path'

export class PageSource {
  static readonly ORIGIN = 'http://acde.local'

  static element(document: Document, tag: string, className: string | null, style: string | null, text?: string): HTMLElement {
    const element = document.createElement(tag)
    if (className) element.className = className
    if (style) element.setAttribute('style', style)
    if (text !== undefined) element.textContent = text
    return element
  }

  static escape(text: string): string {
    return text.replace(/[&<>"]/g, (c) => `&#${c.charCodeAt(0)};`)
  }

  static window(app: { label: string; entry: string }, theme: string): string {
    return `<!doctype html><html lang="zh-Hans"><head><meta charset="utf-8"><title>${PageSource.escape(app.label)}</title><style>${theme}
html, body { margin: 0; width: 100%; height: 100%; overflow: hidden; }
body { font-family: var(--f-sans, sans-serif); color: var(--ink, #1d1d1f); background: var(--paper, #fff); -webkit-font-smoothing: antialiased; }
#app { display: block; width: 100%; height: 100%; overflow: auto; font-size: initial; line-height: normal; }
</style></head><body><div id="app" class="w-app"></div>
<script type="module">import self from 'acde:window'; const app = (await import(${JSON.stringify(app.entry)})).default; self.mount(app)</script></body></html>`
  }

  static isHtml(entry: string): boolean {
    return /\.html?$/i.test(entry.split(/[?#]/)[0]!) || PageSource.isServer(entry)
  }

  static isServer(entry: string): boolean {
    return entry.split(/[?#]/)[0]!.endsWith('/')
  }

  static async served(
    window: { fetch(url: string, init?: object): Promise<{ status: number; headers: { get(name: string): string | null }; text(): Promise<string> }> },
    url: string,
  ): Promise<{ text: string; type: string }> {
    try {
      const response = await window.fetch(url, { headers: { accept: 'text/html,application/xhtml+xml,*/*;q=0.8' } })
      return { text: await response.text(), type: response.headers.get('content-type') ?? 'text/html' }
    } catch (error) {
      const esc = PageSource.escape
      const head = `<!doctype html><html><head><meta charset="utf-8"><title>${esc(url)}</title></head>`
      const reason = esc(String((error as Error)?.message ?? error))
      const body = `<body style="font: 14px/1.5 system-ui, sans-serif; padding: 24px"><p>${esc(url)}: ${reason}</p></body></html>`
      return { text: head + body, type: 'text/html' }
    }
  }

  static html(entry: string, resources: readonly { prefix: string; dirs: readonly string[] }[]): string {
    const file = PageSource.file(entry, resources)
    if (!file) throw new Error(`${entry}: not a file of the installed packages' assets`)
    return readFileSync(file, 'utf8')
  }

  static rooted(entry: string, resources: readonly { prefix: string; dirs: string[] }[]): { prefix: string; dirs: string[] }[] {
    const path = entry.split(/[?#]/)[0]!
    const dir = PageSource.isServer(path) ? path : `${dirname(normalize(path))}/`.replace(/\/+$/, '/')
    const match = PageSource.#longest(dir, resources)
    if (!match || match.prefix === '/') return [...resources]
    const rest = dir.slice(match.prefix.length)
    const dirs = match.dirs.map((d) => (rest ? join(d, rest.replace(/\/+$/, '')) : d))
    const root = resources.find((r) => r.prefix === '/')
    return [{ prefix: '/', dirs: [...dirs, ...(root?.dirs ?? [])] }, ...resources.filter((r) => r.prefix !== '/')]
  }

  static htmlFor(entry: string, url: string | null | undefined, resources: readonly { prefix: string; dirs: readonly string[] }[]): string {
    if (!url) return entry
    let path: string
    try {
      path = new globalThis.URL(url).pathname
    } catch {
      return entry
    }
    return PageSource.isHtml(path) && PageSource.file(path, resources) ? path : entry
  }

  static file(path: string, resources: readonly { prefix: string; dirs: readonly string[] }[]): string | null {
    let clean: string
    try {
      clean = normalize(decodeURIComponent(path.split(/[?#]/)[0]!))
    } catch {
      return null
    }
    if (!clean.startsWith('/')) return null
    const match = PageSource.#longest(clean, resources)
    if (!match) return null
    for (const dir of match.dirs) {
      const file = join(dir, clean.slice(match.prefix.length))
      if (file.startsWith(dir.endsWith('/') ? dir : `${dir}/`) && stat(file)?.isFile) return file
    }
    return null
  }

  static #longest<T extends { prefix: string }>(path: string, resources: readonly T[]): T | undefined {
    let best: T | undefined
    for (const r of resources) if (path.startsWith(r.prefix) && (!best || r.prefix.length > best.prefix.length)) best = r
    return best
  }
}
