import type { Caller } from '../../../vendor/plugin-api/src/tools'

export type { ActionAnnotations, AppAction, Caller, JsonSchema, ToolRisk } from '../../../vendor/plugin-api/src/tools'

export interface ActionTarget {
  readonly type: string
  readonly id: string
}

export interface ActionCall {
  readonly caller: Caller
  readonly target?: ActionTarget
  readonly signal: AbortSignal
  progress(done: number, text?: string): void
}

export type ActionError = { readonly error: { readonly code: string; readonly message: string } }

export type ActionResult = { readonly ok: true; readonly output?: unknown } | ActionError

export type ActionHandler<A = { readonly [key: string]: unknown }> = (args: A, call: ActionCall) => ActionResult | Promise<ActionResult>

export interface AcdeActions {
  handle<A = { readonly [key: string]: unknown }>(name: string, handler: ActionHandler<A>): () => boolean
}

declare module '../page' {
  interface AcdePage {
    readonly actions?: AcdeActions
  }
}
