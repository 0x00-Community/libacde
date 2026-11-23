import type { AppAction, JsonSchema, ToolRisk } from '../acde-api/src/index'
import { TOOL_NAME, TOOL_RISKS } from '../acde-api/vendor/plugin-api/src/tools'

const SHAPE = '{name, description, inputSchema, outputSchema?, annotations?, longRunning?, target?}'

const isObject = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value)

function schema(value: unknown, field: string): JsonSchema | string {
  if (!isObject(value) || value['type'] !== 'object') return `${field} is a JSON schema of an object ({type: "object", …})`
  return value as JsonSchema
}

function annotations(value: unknown): AppAction['annotations'] | string {
  if (value === undefined) return undefined
  if (!isObject(value)) return 'annotations is {readOnly?, destructive?, toolRisk?}'
  const { readOnly, destructive, toolRisk } = value
  if (readOnly !== undefined && typeof readOnly !== 'boolean') return 'annotations.readOnly is true or false'
  if (destructive !== undefined && typeof destructive !== 'boolean') return 'annotations.destructive is true or false'
  if (toolRisk !== undefined && !TOOL_RISKS.includes(toolRisk as ToolRisk)) return `annotations.toolRisk is one of ${TOOL_RISKS.join(', ')}`
  return {
    ...(readOnly !== undefined && { readOnly }),
    ...(destructive !== undefined && { destructive }),
    ...(toolRisk !== undefined && { toolRisk: toolRisk as ToolRisk }),
  }
}

function action(raw: unknown): AppAction | string {
  if (!isObject(raw)) return `an action is ${SHAPE}`
  const { name, description, inputSchema, outputSchema, longRunning, target } = raw
  if (typeof name !== 'string' || !TOOL_NAME.test(name)) {
    return `an action's name is lower case letters, digits, _ and - (at most 64), not ${JSON.stringify(name)}`
  }
  const named = (why: string) => `action ${name}: ${why}`
  if (typeof description !== 'string' || !description.trim()) return named('needs a description')
  const input = schema(inputSchema, 'inputSchema')
  if (typeof input === 'string') return named(input)
  const output = outputSchema === undefined ? undefined : schema(outputSchema, 'outputSchema')
  if (typeof output === 'string') return named(output)
  const hints = annotations(raw['annotations'])
  if (typeof hints === 'string') return named(hints)
  if (longRunning !== undefined && typeof longRunning !== 'boolean') return named('longRunning is true or false')
  if (target !== undefined && (!isObject(target) || typeof target['type'] !== 'string' || !target['type'])) return named('target is {type}')
  return {
    name,
    description,
    inputSchema: input,
    ...(output && { outputSchema: output }),
    ...(hints && { annotations: hints }),
    ...(longRunning === true && { longRunning: true }),
    ...(target !== undefined && { target: { type: (target as { type: string }).type } }),
  }
}

export function actions(raw: unknown): AppAction[] | string {
  if (raw === undefined) return []
  if (!Array.isArray(raw)) return `actions lists ${SHAPE}`
  const out: AppAction[] = []
  for (const entry of raw) {
    const parsed = action(entry)
    if (typeof parsed === 'string') return parsed
    if (out.some((a) => a.name === parsed.name)) return `action ${parsed.name} is declared twice`
    out.push(parsed)
  }
  return out
}
