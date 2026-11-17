type Parser = new () => { parseFromString(html: string, type: string): Document }

interface DevHotIo {
  read(src: string): string | null
  liveEdit(url: string, source: string): string | undefined
}

export class DevHot {
  static readonly BUST = 'acde-dev'
  static readonly STYLE = /<style\b[^>]*>[\s\S]*?<\/style\s*>/gi
  static readonly LINK = /<link\b[^>]*>/gi
  static readonly SCRIPT_SRC = /(<script\b[^>]*?\ssrc\s*=\s*)(["']?)[^"'\s>]*\2/gi
  #generation = 0
  readonly #urls: string[]
  readonly #code: (string | null)[]

  constructor(
    readonly document: Document,
    readonly parser: Parser,
    readonly base: string,
    readonly io: DevHotIo,
    public source: string,
  ) {
    const scripts = this.#scripts(source)
    this.#urls = scripts.map((s) => new URL(s.getAttribute('src')!, base).href)
    this.#code = scripts.map((s) => io.read(s.getAttribute('src')!))
  }

  static shape(html: string): string {
    return html
      .replace(DevHot.STYLE, '')
      .replace(DevHot.LINK, (tag) => (/\brel\s*=\s*["']?[^"'>]*\bstylesheet\b/i.test(tag) ? '' : tag))
      .replace(DevHot.SCRIPT_SRC, '$1$2$2')
  }

  static sheets(root: Document): Element[] {
    return [...root.querySelectorAll('style, link')].filter((e) => e.localName === 'style' || /(^|\s)stylesheet(\s|$)/i.test(e.getAttribute('rel') ?? ''))
  }

  static href(element: Element): string {
    const href = element.getAttribute('href') ?? ''
    const at = href.search(/[?&]acde-dev=/)
    return at < 0 ? href : href.slice(0, at)
  }

  static same(a: Element, b: Element): boolean {
    if (a.localName !== b.localName) return false
    return a.localName === 'style' ? a.textContent === b.textContent : DevHot.href(a) === DevHot.href(b)
  }

  #parse(html: string): Document {
    return new this.parser().parseFromString(html, 'text/html')
  }

  #scripts(html: string): Element[] {
    return [...this.#parse(html).querySelectorAll('script[src]')]
  }

  apply(next: string): string | null {
    if (DevHot.shape(this.source) !== DevHot.shape(next)) return 'the page itself changed'
    const fresh = this.#scripts(next)
    if (fresh.length !== this.#urls.length) return 'scripts were added or removed'
    const edits: [number, string][] = []
    for (const [i, script] of fresh.entries()) {
      const src = script.getAttribute('src')!
      const code = this.io.read(src)
      if (code === null) return `${src} is gone`
      if (code === this.#code[i]) continue
      if (script.getAttribute('type') !== 'module') return `${src} is a classic script`
      edits.push([i, code])
    }
    const sheets = this.#sheets(next)
    if (typeof sheets === 'string') return sheets
    for (const [i, code] of edits) {
      const error = this.io.liveEdit(this.#urls[i]!, code)
      if (error !== undefined) return `${this.#urls[i]}: ${error}`
      this.#code[i] = code
    }
    sheets()
    this.source = next
    return null
  }

  #sheets(next: string): string | (() => void) {
    const old = DevHot.sheets(this.#parse(this.source))
    const fresh = DevHot.sheets(this.#parse(next))
    if (old.length !== fresh.length) return 'stylesheets were added or removed'
    const live = DevHot.sheets(this.document)
    const matched: Element[] = []
    let at = 0
    for (const [i, sheet] of old.entries()) {
      while (at < live.length && !DevHot.same(live[at]!, sheet)) at++
      if (at === live.length || sheet.localName !== fresh[i]!.localName) return 'stylesheets do not line up'
      matched.push(live[at++]!)
    }
    return () => {
      const generation = ++this.#generation
      for (const [i, element] of matched.entries()) {
        const sheet = fresh[i]!
        if (element.localName === 'style') {
          if (element.textContent !== sheet.textContent) element.textContent = sheet.textContent
          continue
        }
        const href = DevHot.href(sheet)
        element.setAttribute('href', `${href}${href.includes('?') ? '&' : '?'}${DevHot.BUST}=${generation}`)
      }
    }
  }
}
