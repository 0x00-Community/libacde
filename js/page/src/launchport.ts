import type { LinePort, PortLine } from '@acde/dom'

export interface LaunchWindow {
  readonly MessageEvent: new (type: string, init: { data: unknown; ports: unknown[] }) => unknown
  dispatchEvent(event: unknown): boolean
  setTimeout(run: () => void, ms: number): unknown
}

export class LaunchPort {
  readonly #line: LinePort

  constructor(
    readonly data: unknown,
    make: (line: PortLine) => LinePort,
    post: (message: Record<string, unknown>) => void,
  ) {
    this.#line = make({ send: (line) => post({ portline: line }), closed: () => post({ portclosed: true }) })
  }

  take(message: { type?: string; line?: unknown }): boolean {
    if (message.type === 'portline') this.#line.receive(typeof message.line === 'string' ? message.line : null)
    else if (message.type === 'portclosed') this.#line.close()
    else return false
    return true
  }

  hand(window: LaunchWindow): void {
    window.setTimeout(() => void window.dispatchEvent(new window.MessageEvent('message', { data: this.data, ports: [this.#line.port] })), 0)
  }
}
