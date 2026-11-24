import type { ManagerEvent, ManagerLink, Welcome } from '../manager/manager'

export type PeerKind = 'desktop' | 'app' | 'service'

export interface Peer {
  readonly id: number
  readonly kind: PeerKind
  readonly app: string | null
  readonly service: string | null
  readonly instance: number
}

export interface PeerSide {
  call(peer: Peer, service: string, method: string, args: readonly unknown[]): Promise<unknown>
  frame(peer: Peer, frame: Record<string, unknown>): void
}

export class LocalLink implements ManagerLink {
  readonly #queue: ManagerEvent[] = []
  #wake: (() => void) | null = null
  #closed = false

  constructor(
    readonly welcome: Welcome,
    readonly peer: Peer,
    readonly side: PeerSide,
  ) {}

  call(service: string, method: string, args: readonly unknown[] = []): Promise<unknown> {
    if (this.#closed) return Promise.reject(Object.assign(new Error('the local session ended'), { status: 503, code: 'unavailable' }))
    return this.side.call(this.peer, service, method, args)
  }

  onEvents(callback: () => void): void {
    this.#wake = callback
  }

  pump(): ManagerEvent[] {
    return this.#queue.splice(0)
  }

  send(frame: object): void {
    if (!this.#closed) this.side.frame(this.peer, frame as Record<string, unknown>)
  }

  push(event: ManagerEvent): void {
    if (this.#closed) return
    this.#queue.push(event)
    this.#wake?.()
  }

  close(): void {
    if (this.#closed) return
    this.#queue.push({ type: 'managerclosed' })
    this.#wake?.()
    this.#closed = true
  }
}
