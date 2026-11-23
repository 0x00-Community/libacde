const NOT_A_NAME = new Set(['initial', 'inherit', 'unset', 'revert', 'revert-layer', 'revert-rule', 'default', 'none'])
const RESERVED = new Set(['decimal', 'disc', 'square', 'circle', 'disclosure-open', 'disclosure-closed'])
const NEEDS_TWO = new Set(['alphabetic', 'numeric'])

export type CounterStyleDescriptor = 'system' | 'symbols' | 'additive-symbols' | 'negative' | 'prefix' | 'suffix' | 'range' | 'pad' | 'speak-as' | 'fallback'

const isWhitespace = (c: string | undefined) => c === ' ' || c === '\t' || c === '\n' || c === '\r' || c === '\f'
const isHex = (c: string | undefined) => c !== undefined && /^[0-9a-fA-F]$/.test(c)
const isNameStart = (c: string | undefined) => c !== undefined && (/^[a-zA-Z_]$/.test(c) || c.charCodeAt(0) >= 0x80)
const isName = (c: string | undefined) => isNameStart(c) || (c !== undefined && /^[0-9-]$/.test(c))
const isEscape = (text: string, at: number) => text[at] === '\\' && at + 1 < text.length && text[at + 1] !== '\n' && text[at + 1] !== '\r' && text[at + 1] !== '\f'

function escapeAt(text: string, at: number): { char: string; next: number } {
  let i = at + 1
  if (!isHex(text[i])) return { char: text[i] ?? '�', next: i + 1 }
  let hex = ''
  while (hex.length < 6 && isHex(text[i])) hex += text[i++]
  if (text[i] === '\r' && text[i + 1] === '\n') i += 2
  else if (isWhitespace(text[i])) i++
  const code = parseInt(hex, 16)
  const char = code === 0 || code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff) ? '�' : String.fromCodePoint(code)
  return { char, next: i }
}

export function identOf(written: string): string | null {
  const text = written.replace(/^[ \t\n\r\f]+|[ \t\n\r\f]+$/g, '')
  const starts =
    text[0] === '-' ? isNameStart(text[1]) || text[1] === '-' || isEscape(text, 1) : isNameStart(text[0]) || isEscape(text, 0)
  if (!starts) return null
  let out = ''
  let i = 0
  while (i < text.length) {
    if (isEscape(text, i)) {
      const { char, next } = escapeAt(text, i)
      out += char
      i = next
    } else if (isName(text[i])) {
      out += text[i++]
    } else {
      return null
    }
  }
  return out
}

export function counterStyleName(written: string): string | null {
  const name = identOf(written)
  if (name === null || NOT_A_NAME.has(name.toLowerCase()) || RESERVED.has(name.toLowerCase())) return null
  return name
}

function topLevel(text: string, split: (c: string) => boolean): string[] {
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

export function counterStyleParts(text: string): { prelude: string; descriptors: Map<string, string> } {
  const open = text.indexOf('{'), close = text.lastIndexOf('}')
  const descriptors = new Map<string, string>()
  if (open < 0) return { prelude: text.trim(), descriptors }
  for (const declaration of topLevel(text.slice(open + 1, close > open ? close : text.length), (c) => c === ';')) {
    const colon = declaration.indexOf(':')
    if (colon > 0) descriptors.set(declaration.slice(0, colon).trim().toLowerCase(), declaration.slice(colon + 1).trim())
  }
  return { prelude: text.slice(0, open).trim(), descriptors }
}

export function counterStyleText(escapedName: string, descriptors: ReadonlyMap<string, string>): string {
  const body = [...descriptors].map(([name, value]) => ` ${name}: ${value};`).join('')
  return `@counter-style ${escapedName} {${body} }`
}

function algorithmOf(system: string | undefined): string {
  return (system ?? 'symbolic').trim().split(/[ \t\n\r\f]+/)[0]!.toLowerCase()
}

export function counterStyleSettable(descriptors: ReadonlyMap<string, string>, name: CounterStyleDescriptor, value: string): boolean {
  const algorithm = algorithmOf(descriptors.get('system'))
  if (name === 'system') return algorithmOf(value) === algorithm
  if (name === 'symbols' && NEEDS_TWO.has(algorithm)) return topLevel(value, isWhitespace).length >= 2
  return true
}
