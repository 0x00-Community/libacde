import type { Image } from 'acdev/driver'
import type { Fuzzy, Test } from './prepare'

export function compare(a: Image, b: Image): { maxDifference: number; totalPixels: number; size: string | null } {
  const width = Math.min(a.width, b.width)
  const height = Math.min(a.height, b.height)
  let maxDifference = 0
  let totalPixels = 0
  for (let y = 0; y < height; y++)
    for (let x = 0; x < width; x++) {
      const i = (y * a.width + x) * 4
      const j = (y * b.width + x) * 4
      let d = 0
      for (let c = 0; c < 4; c++) d = Math.max(d, Math.abs(a.data[i + c]! - b.data[j + c]!))
      if (d > 0) {
        totalPixels++
        if (d > maxDifference) maxDifference = d
      }
    }
  const size = a.width !== b.width || a.height !== b.height ? `${a.width}×${a.height} against ${b.width}×${b.height}` : null
  return { maxDifference, totalPixels, size }
}

export function fuzzyFor(test: Test, ref: string): Fuzzy | null {
  return test.fuzzy?.find((f) => f.ref === ref) ?? test.fuzzy?.find((f) => f.ref === null) ?? null
}

export function within(diff: { maxDifference: number; totalPixels: number }, fuzzy: Fuzzy | null): boolean {
  if (diff.totalPixels === 0) return !fuzzy || (fuzzy.maxDifference[0] === 0 && fuzzy.totalPixels[0] === 0)
  if (!fuzzy) return false
  return diff.maxDifference >= fuzzy.maxDifference[0] && diff.maxDifference <= fuzzy.maxDifference[1] && diff.totalPixels >= fuzzy.totalPixels[0] && diff.totalPixels <= fuzzy.totalPixels[1]
}
