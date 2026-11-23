export type Fields = { readonly [key: string]: unknown }

export const NAME = /^[a-z][a-z0-9._:-]{0,63}$/

export const isFields = (value: unknown): value is Fields =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

export function names(fields: Fields, key: string, errors: string[], pattern = NAME): string[] {
  const value = fields[key]
  if (value === undefined) return []
  if (!Array.isArray(value) || !value.every((item) => typeof item === 'string' && pattern.test(item))) {
    errors.push(`${key} 是名字的数组 (小写字母开头, 最多 64 个字符)`)
    return []
  }
  return [...new Set(value as string[])]
}

export function oneOf<T extends string>(fields: Fields, key: string, allowed: readonly T[], errors: string[]): T[] {
  const value = fields[key]
  if (value === undefined) return []
  if (!Array.isArray(value) || !value.every((item) => allowed.includes(item as T))) {
    errors.push(`${key} 是 ${allowed.join(' / ')} 组成的数组`)
    return []
  }
  return [...new Set(value as T[])]
}

export function relativePath(value: unknown, label: string, errors: string[]): string | null {
  if (value === undefined) return null
  const parts = typeof value === 'string' ? value.split('/') : ['']
  if (parts.some((part) => part === '..' || part === '')) {
    errors.push(`${label} 是插件目录里的相对路径`)
    return null
  }
  return parts.filter((part) => part !== '.').join('/')
}
