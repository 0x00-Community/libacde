import { readFileSync } from 'node:fs'
import { join } from 'node:path'

declare const ACDEV_API_LEVEL: number | undefined
declare const ACDEV_MIN_API_LEVEL: number | undefined

function level(file: string, defined: number | undefined): number {
  if (typeof defined === 'number') return defined
  const value = Number(readFileSync(join(import.meta.dirname, '..', 'acde-api', file), 'utf8').trim())
  if (!Number.isInteger(value) || value < 1) throw new Error(`acde-api/${file} holds no positive contract level`)
  return value
}

export const API_LEVEL = level('api-level', typeof ACDEV_API_LEVEL === 'number' ? ACDEV_API_LEVEL : undefined)
export const MIN_API_LEVEL = level('min-api-level', typeof ACDEV_MIN_API_LEVEL === 'number' ? ACDEV_MIN_API_LEVEL : undefined)
if (MIN_API_LEVEL > API_LEVEL) throw new Error(`acde-api/min-api-level (${MIN_API_LEVEL}) is above acde-api/api-level (${API_LEVEL})`)
