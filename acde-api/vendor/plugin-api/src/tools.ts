import type { ToolLimits } from './budget/limits'
import { utf8Bytes } from './core/json'

export type JsonSchema = { readonly type: 'object'; readonly [key: string]: unknown }

export interface Caller {
  readonly id: string
  readonly session: string
  readonly run: string
}

export const MAX_CALLER_FIELD = 256

const CALLER_KEYS = ['id', 'run', 'session']

const field = (value: unknown): value is string =>
  typeof value === 'string' && value.length > 0 && utf8Bytes(value) <= MAX_CALLER_FIELD

export function callerOf(value: unknown): Caller | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  if (Object.keys(value).sort().join() !== CALLER_KEYS.join()) return null
  const { id, session, run } = value as { id?: unknown; session?: unknown; run?: unknown }
  return field(id) && field(session) && field(run) ? Object.freeze({ id, session, run }) : null
}

export interface ToolCall {
  readonly agent: Caller
}

export type ToolResult = string | { readonly text: string; readonly isError?: boolean }

export interface ToolHints {
  readonly readOnly?: boolean
  readonly destructive?: boolean
}

export interface Tool<A = { readonly [key: string]: unknown }> extends ToolHints {
  readonly name: string
  readonly description: string
  readonly parameters: JsonSchema
  readonly limits?: ToolLimits
  execute(args: A, call: ToolCall): ToolResult | Promise<ToolResult>
}

export const TOOL_NAME = /^[a-z][a-z0-9_-]{0,63}$/

export const TOOL_RISKS = ['readOnly', 'risky', 'destructive'] as const
export type ToolRisk = (typeof TOOL_RISKS)[number]

export interface ActionAnnotations extends ToolHints {
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

export function toolRisk(hints: Readonly<Partial<Record<keyof ToolHints, unknown>>>, trusted: boolean): ToolRisk {
  if (hints.destructive === true) return 'destructive'
  return trusted && hints.readOnly === true ? 'readOnly' : 'risky'
}
