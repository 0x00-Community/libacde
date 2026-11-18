import { PageSource } from '../../../page/src/page'
import { process } from '../../../page/src/sys/process'

export interface NativeDevToolsTarget {
  readonly id: string
  readonly kind: 'page' | 'service'
  readonly title: string
  readonly url: string
}

export interface NativeDevTools {
  open(): { port: number } | null
  targets(targets: NativeDevToolsTarget[]): void
  reloading?(web: string): void
}

export interface DevToolsShell {
  devEvent?(json: string): number
}

export interface DevToolsPage {
  readonly name: string
  readonly app: string | null
  readonly kind: 'app' | 'desktop'
  readonly window: number
  readonly title: string
  readonly url: string | null
}

export class DevToolsEndpoint {
  static readonly TARGETS_MS = 250
  readonly #native: NativeDevTools | null
  #published = ''

  constructor(native: NativeDevTools | undefined, readonly shell: DevToolsShell, readonly page: () => DevToolsPage | null, announce?: (port: number) => void) {
    const endpoint = native?.open() ?? null
    this.#native = endpoint ? native! : null
    if (!endpoint) return
    announce?.(endpoint.port)
    const timer = setInterval(() => this.publish(), DevToolsEndpoint.TARGETS_MS) as unknown as { unref?: () => void }
    timer.unref?.()
  }

  port(): { port: number } {
    const endpoint = this.#native?.open()
    if (!endpoint) throw new Error('devtools: this session serves no DevTools endpoint')
    this.publish()
    return { port: endpoint.port }
  }

  reloading(web: string): void {
    this.#native?.reloading?.(web)
  }

  publish(): void {
    if (!this.#native) return
    const page = this.page()
    const targets: NativeDevToolsTarget[] = page ? [DevToolsEndpoint.#target(page)] : []
    const key = JSON.stringify(targets)
    if (key === this.#published) return
    this.#published = key
    this.#native.targets(targets)
  }

  pressed(): void {
    const page = this.page()
    if (!page) {
      DevToolsEndpoint.#say('acdev: F12: no page is open')
      return
    }
    const event = { type: 'event', event: 'devtools', web: page.name, app: page.app, window: page.window }
    let reached = 0
    try {
      reached = this.shell.devEvent?.(JSON.stringify(event)) ?? 0
    } catch (error) {
      DevToolsEndpoint.#say(`acdev: F12: ${(error as Error).message}`)
      return
    }
    const outcome = reached ? 'asked acdev dev to open DevTools' : 'no acdev dev is connected to open DevTools'
    DevToolsEndpoint.#say(`acdev: F12 on ${page.name}: ${outcome}`)
  }

  static #target(page: DevToolsPage): NativeDevToolsTarget {
    const owner = page.kind === 'desktop' ? 'desktop' : (page.app ?? page.name)
    return { id: page.name, kind: 'page', title: `${page.title} (${owner})`, url: page.url ?? `${PageSource.ORIGIN}/` }
  }

  static #say(line: string): void {
    process.stderr.write(`${line}\n`)
  }
}
