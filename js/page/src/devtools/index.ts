export type DevToolsParams = Readonly<Record<string, unknown>>

export interface DevToolsSession {
  readonly id: number
  readonly enabled: Set<string>
}

export type DevToolsMethod = (params: DevToolsParams, session: DevToolsSession) => unknown

export interface DevToolsDomain {
  readonly name: string
  readonly methods: Readonly<Record<string, DevToolsMethod>>
  readonly closed?: (session: number) => void
}

export interface DevToolsHost {
  devtoolsHandler?(handler: (session: number, text: string | null) => void): string | undefined
  devtoolsSend?(session: number, text: string): void
}

export class DevToolsError extends Error {
  constructor(
    readonly code: number,
    message: string,
  ) {
    super(message)
  }
}

interface Incoming {
  readonly id?: unknown
  readonly method?: unknown
  readonly params?: unknown
  readonly sessionId?: unknown
}

export class DevToolsPage {
  readonly #sessions = new Map<number, DevToolsSession>()
  readonly #domains = new Map<string, DevToolsDomain>()

  constructor(
    readonly frameId: string,
    readonly send: (session: number, text: string) => void,
  ) {}

  static install(host: DevToolsHost, domains: (page: DevToolsPage) => readonly DevToolsDomain[]): DevToolsPage | null {
    const send = host.devtoolsSend
    if (!host.devtoolsHandler || !send) return null
    let page: DevToolsPage | null = null
    const frameId = host.devtoolsHandler((session, text) => page?.receive(session, text))
    if (typeof frameId !== 'string') return null
    page = new DevToolsPage(frameId, (session, text) => send.call(host, session, text))
    page.use(domains(page))
    return page
  }

  use(domains: readonly DevToolsDomain[]): void {
    for (const domain of domains) this.#domains.set(domain.name, domain)
  }

  receive(session: number, text: string | null): void {
    if (text === null) {
      this.#sessions.delete(session)
      for (const domain of this.#domains.values()) domain.closed?.(session)
      return
    }
    let message: Incoming
    try {
      message = JSON.parse(text) as Incoming
    } catch {
      return
    }
    let state = this.#sessions.get(session)
    if (!state) {
      state = { id: session, enabled: new Set() }
      this.#sessions.set(session, state)
    }
    this.#answer(state, message)
  }

  listening(domain: string): boolean {
    for (const session of this.#sessions.values()) if (session.enabled.has(domain)) return true
    return false
  }

  emitTo(session: number, domain: string, method: string, params: DevToolsParams): void {
    if (this.#sessions.has(session)) this.send(session, JSON.stringify({ method: `${domain}.${method}`, params }))
  }

  emit(domain: string, method: string, params: DevToolsParams): void {
    const text = JSON.stringify({ method: `${domain}.${method}`, params })
    for (const session of this.#sessions.values()) if (session.enabled.has(domain)) this.send(session.id, text)
  }

  #answer(session: DevToolsSession, message: Incoming): void {
    const reply: Record<string, unknown> = { id: message.id }
    if (message.sessionId !== undefined) reply['sessionId'] = message.sessionId
    const params = (message.params && typeof message.params === 'object' ? message.params : {}) as DevToolsParams
    const done = (result: unknown) => {
      reply['result'] = result ?? {}
      this.send(session.id, JSON.stringify(reply))
    }
    const failed = (error: unknown) => {
      const message = String((error as Error | null)?.message ?? error)
      reply['error'] = error instanceof DevToolsError ? { code: error.code, message } : { code: -32603, message }
      this.send(session.id, JSON.stringify(reply))
    }
    let result: unknown
    try {
      result = this.#call(String(message.method), params, session)
    } catch (error) {
      return failed(error)
    }
    if (result instanceof Promise) result.then(done, failed)
    else done(result)
  }

  #call(method: string, params: DevToolsParams, session: DevToolsSession): unknown {
    const dot = method.indexOf('.')
    const domain = this.#domains.get(method.slice(0, dot))
    const name = method.slice(dot + 1)
    const own = dot > 0 ? domain?.methods[name] : undefined
    if (own) return own(params, session)
    if (domain && name === 'enable') return void session.enabled.add(domain.name)
    if (domain && name === 'disable') return void session.enabled.delete(domain.name)
    throw new DevToolsError(-32601, `'${method}' wasn't found`)
  }
}
