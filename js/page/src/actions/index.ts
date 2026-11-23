import type { AcdeActions, ActionHandler, ActionResult, ActionTarget, AppAction, Caller } from '@acde/api'

export interface Operate {
  readonly id: string
  readonly name: string
  readonly args: unknown
  readonly caller: Caller
  readonly target?: ActionTarget
}

export type ActionEvent =
  | { readonly type: 'pending'; readonly id: string }
  | { readonly type: 'progress'; readonly id: string; readonly done: number; readonly text?: string }
  | { readonly type: 'result'; readonly id: string; readonly result: ActionResult }

const failed = (code: string, message: string): ActionResult => ({ error: { code, message } })

function checked(result: unknown): ActionResult {
  if (typeof result !== 'object' || result === null) return failed('bad_result', 'the handler answered neither {ok, output} nor {error: {code, message}}')
  const given = result as { ok?: unknown; output?: unknown; error?: { code?: unknown; message?: unknown } }
  if (given.ok === true) return given.output === undefined ? { ok: true } : { ok: true, output: given.output }
  const error = given.error
  if (error && typeof error.code === 'string' && typeof error.message === 'string') return failed(error.code, error.message)
  return failed('bad_result', 'the handler answered neither {ok, output} nor {error: {code, message}}')
}

export class Actions {
  readonly #declared: ReadonlyMap<string, AppAction>
  readonly #handlers = new Map<string, ActionHandler>()
  readonly #running = new Map<string, AbortController>()
  readonly #send: (event: ActionEvent) => void

  constructor(declared: readonly AppAction[], send: (event: ActionEvent) => void) {
    this.#declared = new Map(declared.map((action) => [action.name, action]))
    this.#send = send
  }

  readonly api: AcdeActions = {
    handle: (name, handler) => {
      if (!this.#declared.has(name)) throw new TypeError(`actions.handle: ${JSON.stringify(name)} is not an action this app declares (acde.apps[].actions)`)
      if (typeof handler !== 'function') throw new TypeError('actions.handle: the handler is a function (args, call) => result')
      const own = handler as ActionHandler
      this.#handlers.set(name, own)
      return () => this.#handlers.get(name) === own && this.#handlers.delete(name)
    },
  }

  async operate(request: Operate): Promise<void> {
    const { id, name } = request
    const declared = this.#declared.get(name)
    const handler = this.#handlers.get(name)
    if (!declared || !handler) {
      const why = declared ? failed('no_handler', `no handler for ${name} yet`) : failed('unknown_action', `${name} is not declared`)
      return this.#send({ type: 'result', id, result: why })
    }
    const controller = new AbortController()
    this.#running.set(id, controller)
    if (declared.longRunning) this.#send({ type: 'pending', id })
    const progress = (done: number, text?: string) => {
      if (!controller.signal.aborted) this.#send({ type: 'progress', id, done, ...(text !== undefined && { text: String(text) }) })
    }
    const call = { caller: request.caller, ...(request.target && { target: request.target }), signal: controller.signal, progress }
    let result: ActionResult
    try {
      result = checked(await handler(request.args as never, call))
    } catch (error) {
      result = failed('failed', error instanceof Error ? error.message : String(error))
    }
    this.#running.delete(id)
    if (controller.signal.aborted) result = failed('cancelled', `${name} was cancelled`)
    this.#send({ type: 'result', id, result })
  }

  cancel(id: string): boolean {
    const controller = this.#running.get(id)
    if (!controller) return false
    controller.abort()
    return true
  }
}
