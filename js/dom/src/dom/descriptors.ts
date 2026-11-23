export function topLevel(text: string, split: (c: string) => boolean): string[] {
  const parts: string[] = []
  let depth = 0, quote = '', start = 0
  for (let i = 0; i < text.length; i++) {
    const c = text[i]!
    if (c === '\\') {
      i++
      continue
    }
    if (quote) {
      if (c === quote) quote = ''
      continue
    }
    if (c === '"' || c === "'") quote = c
    else if (c === '(' || c === '[' || c === '{') depth++
    else if (c === ')' || c === ']' || c === '}') depth--
    else if (depth === 0 && split(c)) {
      parts.push(text.slice(start, i))
      start = i + 1
    }
  }
  parts.push(text.slice(start))
  return parts.map((part) => part.trim()).filter(Boolean)
}

export interface Descriptor {
  value: string
  important: boolean
}

export function parseDescriptors(body: string): Map<string, Descriptor> {
  const out = new Map<string, Descriptor>()
  for (const declaration of topLevel(body, (c) => c === ';')) {
    const colon = declaration.indexOf(':')
    if (colon <= 0) continue
    const raw = declaration.slice(0, colon).trim()
    const name = raw.startsWith('--') ? raw : raw.toLowerCase()
    let value = declaration.slice(colon + 1).trim()
    const important = /!\s*important$/i.test(value)
    if (important) value = value.replace(/!\s*important$/i, '').trim()
    if (!value) continue
    out.delete(name)
    out.set(name, { value, important })
  }
  return out
}

export function serializeDescriptors(descriptors: ReadonlyMap<string, Descriptor>): string {
  return [...descriptors].map(([name, { value, important }]) => `${name}: ${value}${important ? ' !important' : ''};`).join(' ')
}
