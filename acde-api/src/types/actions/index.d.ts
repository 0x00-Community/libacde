export type JsonSchema = { readonly type: 'object'; readonly [key: string]: unknown }

export type ToolRisk = 'readOnly' | 'risky' | 'destructive'

export interface Caller {
  readonly id: string
  readonly session: string
  readonly run: string
}

export interface ActionAnnotations {
  readonly readOnly?: boolean
  readonly destructive?: boolean
  readonly toolRisk?: ToolRisk
}

export interface AppAction {
  readonly name: string
  readonly description: string
  readonly inputSchema: JsonSchema
  readonly outputSchema?: JsonSchema
  readonly annotations?: ActionAnnotations
  readonly longRunning?: boolean
  readonly target?: { readonly type: string }
}

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
