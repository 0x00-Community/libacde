import type { InspectNode } from '@dopame/acde-api'

export type Line = readonly [string, string]

export const Outline = {
  lines(nodes: readonly InspectNode[], depth: number, out: Line[]): void {
    const pad = '  '.repeat(depth)
    for (const n of nodes) {
      const parts = [`${pad}- ${n.role}`]
      if (n.name) parts.push(JSON.stringify(n.name))
      let line = parts.join(' ')
      if (n.value !== undefined) line += `: ${JSON.stringify(n.value)}`
      if (n.level) line += ` [level=${n.level}]`
      for (const s of n.states ?? []) line += ` [${s}]`
      line += ` [${n.ref}]`
      out.push([`${n.ref}|${n.role}`, line])
      if (n.children) Outline.lines(n.children, depth + 1, out)
      if (n.more) out.push([`${n.ref}|more`, `${pad}  - … ${n.more} more not listed`])
    }
  },

  find(nodes: readonly InspectNode[], needle: string): InspectNode[] {
    const hit = (n: InspectNode) => [n.name, n.value].some((s) => s?.toLowerCase().includes(needle))
    const walk = (n: InspectNode): InspectNode | null => {
      if (hit(n)) return n
      const children = (n.children ?? []).map(walk).filter((c): c is InspectNode => !!c)
      if (!children.length) return null
      const { more: _more, ...rest } = n
      return { ...rest, children }
    }
    return nodes.map(walk).filter((n): n is InspectNode => !!n)
  },

  diff(before: readonly Line[], after: readonly Line[]): string[] {
    const keyed = (lines: readonly Line[]) => {
      const seen = new Map<string, number>()
      return lines.map(([key, line]) => {
        const n = seen.get(key) ?? 0
        seen.set(key, n + 1)
        return [`${key}#${n}`, line] as const
      })
    }
    const old = new Map(keyed(before))
    const now = keyed(after)
    const kept = new Set<string>()
    const out: string[] = []
    for (const [key, line] of now) {
      const was = old.get(key)
      kept.add(key)
      if (was === undefined) out.push(`+ ${line.trimStart()}`)
      else if (was !== line) out.push(`~ ${line.trimStart()}`)
    }
    for (const [key, line] of old) if (!kept.has(key)) out.push(`- ${line.trimStart()}`)
    if (!out.length) return ['(no changes)']
    if (out.length > after.length * 0.6 && after.length > 10) return ['(many changes, listing everything)', ...after.map(([, line]) => line)]
    return out
  }
}
