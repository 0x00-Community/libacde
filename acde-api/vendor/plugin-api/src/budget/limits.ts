import { isFields } from '../core/fields'

export const BUDGET_ERRORS = ['budget_calls', 'budget_depth'] as const
export type BudgetError = (typeof BUDGET_ERRORS)[number]

export interface BudgetLimits {
  readonly maxCalls: number
  readonly maxDepth: number
}

export type ToolLimits = Partial<BudgetLimits>

export const MAX_CALLS = 64
export const MAX_DEPTH = 8
export const BUDGET_DEFAULTS: BudgetLimits = { maxCalls: 32, maxDepth: MAX_DEPTH }

export interface BudgetState {
  readonly calls: number
  readonly depth: number
}

export type BudgetCheck = { readonly ok: true } | { readonly ok: false; readonly error: BudgetError }

const within = (value: unknown, max: number): value is number =>
  typeof value === 'number' && Number.isSafeInteger(value) && value >= 1 && value <= max

function field(declared: unknown, max: number, enclosing: number): number | null {
  if (declared === undefined) return enclosing
  return within(declared, max) ? Math.min(declared, enclosing) : null
}

export function toolLimits(declared: unknown, enclosing: BudgetLimits = BUDGET_DEFAULTS): BudgetLimits | null {
  if (declared === undefined) return enclosing
  if (!isFields(declared)) return null
  const maxCalls = field(declared['maxCalls'], MAX_CALLS, enclosing.maxCalls)
  const maxDepth = field(declared['maxDepth'], MAX_DEPTH, enclosing.maxDepth)
  return maxCalls === null || maxDepth === null ? null : { maxCalls, maxDepth }
}

export function budgetAllows(state: BudgetState, limits: BudgetLimits): BudgetCheck {
  if (state.calls >= limits.maxCalls) return { ok: false, error: 'budget_calls' }
  return state.depth > limits.maxDepth ? { ok: false, error: 'budget_depth' } : { ok: true }
}
