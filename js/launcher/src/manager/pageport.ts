import type { BackendLink } from './manager'

export const LINE_LIMIT = 262144

export type PortPost = (message: Record<string, unknown>) => void

export class Lines {
  readonly #decoder = new TextDecoder()
  #text = ''
  #skipping = false

  constructor(readonly each: (line: string | null) => void) {}

  push(chunk: Uint8Array): void {
    this.#text += this.#decoder.decode(chunk, { stream: true })
    let at = this.#text.indexOf('\n')
    while (at >= 0) {
      const line = this.#text.slice(0, at)
      this.#text = this.#text.slice(at + 1)
      if (this.#skipping) this.#skipping = false
      else this.each(line)
      at = this.#text.indexOf('\n')
    }
    if (this.#text.length > LINE_LIMIT) {
      if (!this.#skipping) this.each(null)
      this.#skipping = true
      this.#text = ''
    }
  }
}

class Connection {
  #link: BackendLink | null = null
  #waiting: string[] | null = []
  #open = true

  constructor(connect: () => Promise<BackendLink>, post: PortPost, log: (line: string) => void) {
    const lines = new Lines((line) => this.#open && post({ type: 'portline', line }))
    connect().then(
      (link) => {
        if (!this.#open) return link.close()
        link.onData((chunk) => lines.push(chunk))
        link.onClose(() => {
          if (!this.#open) return
          this.#open = false
          post({ type: 'portclosed' })
        })
        for (const line of this.#waiting ?? []) link.write(line)
        this.#waiting = null
        this.#link = link
      },
      (error: Error & { status?: number; code?: string }) => {
        if (!this.#open) return
        this.#open = false
        log(`the page port has no connection: ${error.status ?? ''} ${error.code ?? ''} ${error.message}`.replace(/ +/g, ' '))
        post({ type: 'portclosed' })
      },
    )
  }

  write(line: string): void {
    if (!this.#open) return
    if (this.#link) this.#link.write(`${line}\n`)
    else this.#waiting?.push(`${line}\n`)
  }

  close(): void {
    if (!this.#open) return
    this.#open = false
    this.#link?.close()
  }
}

export class PagePort {
  #connection: Connection | null = null

  constructor(
    readonly data: unknown,
    readonly connect: (() => Promise<BackendLink>) | undefined,
    readonly log: (line: string) => void,
  ) {}

  static #unsupported(): Promise<BackendLink> {
    return Promise.reject(Object.assign(new Error('this acde cannot connect a page port'), { status: 501, code: 'unsupported' }))
  }

  init(): { port: { data: unknown } } {
    return { port: { data: this.data } }
  }

  open(post: PortPost): void {
    this.close()
    this.#connection = new Connection(this.connect ?? PagePort.#unsupported, post, this.log)
  }

  take(message: unknown): boolean {
    const m = message as { portline?: unknown; portclosed?: unknown } | null
    if (typeof m?.portline === 'string') this.#connection?.write(m.portline)
    else if (m?.portclosed === true) this.close()
    else return false
    return true
  }

  close(): void {
    this.#connection?.close()
    this.#connection = null
  }
}
