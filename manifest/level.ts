declare const ACDEV_API_LEVEL: number | undefined
declare const ACDEV_MIN_API_LEVEL: number | undefined

function read(file: string): number {
  const { readFileSync } = process.getBuiltinModule('node:fs')
  const value = Number(readFileSync(`${import.meta.dirname}/../acde-api/${file}`, 'utf8').trim())
  if (!Number.isInteger(value) || value < 1) throw new Error(`acde-api/${file} holds no positive contract level`)
  return value
}

export const API_LEVEL: number = typeof ACDEV_API_LEVEL === 'number' ? ACDEV_API_LEVEL : read('api-level')
export const MIN_API_LEVEL: number = typeof ACDEV_MIN_API_LEVEL === 'number' ? ACDEV_MIN_API_LEVEL : read('min-api-level')
if (MIN_API_LEVEL > API_LEVEL) throw new Error(`acde-api/min-api-level (${MIN_API_LEVEL}) is above acde-api/api-level (${API_LEVEL})`)
