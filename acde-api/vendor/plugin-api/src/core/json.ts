export type Json = null | boolean | number | string | readonly Json[] | { readonly [key: string]: Json }

export function isJson(value: unknown): value is Json {
  if (value === null || typeof value === 'string' || typeof value === 'boolean') return true
  if (typeof value === 'number') return Number.isFinite(value)
  if (Array.isArray(value)) return value.every(isJson)
  if (typeof value !== 'object' || Object.getPrototypeOf(value) !== Object.prototype) return false
  return Object.values(value).every(isJson)
}

const byCodeUnits = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0)

export function canonicalJson(value: Json): string {
  if (value === null || typeof value !== 'object') {
    if (typeof value === 'number' && !Number.isFinite(value)) throw new Error('JSON 里的数字必须是有限数')
    return JSON.stringify(value)
  }
  if (Array.isArray(value)) return `[${(value as readonly Json[]).map(canonicalJson).join(',')}]`
  const fields = value as { readonly [key: string]: Json }
  const keys = Object.keys(fields).sort(byCodeUnits)
  return `{${keys.map((key) => `${JSON.stringify(key)}:${canonicalJson(fields[key] as Json)}`).join(',')}}`
}

export const utf8Bytes = (text: string): number =>
  [...text].reduce((sum, char) => {
    const point = char.codePointAt(0) ?? 0
    return sum + (point < 0x80 ? 1 : point < 0x800 ? 2 : point < 0x10000 ? 3 : 4)
  }, 0)
