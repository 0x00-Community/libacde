import { readFileSync } from 'node:fs'
import { join } from 'node:path'

declare const ACDEV_API_LEVEL: number | undefined

function level(): number {
  if (typeof ACDEV_API_LEVEL === 'number') return ACDEV_API_LEVEL
  const value = Number(readFileSync(join(import.meta.dirname, '..', 'acde-api', 'api-level'), 'utf8').trim())
  if (!Number.isInteger(value) || value < 1) throw new Error('acde-api/api-level holds no positive contract level')
  return value
}

export const API_LEVEL = level()
