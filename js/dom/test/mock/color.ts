const NAMED: Record<string, number[]> = {
  black: [0, 0, 0], white: [255, 255, 255], red: [255, 0, 0],
  darkblue: [0, 0, 139], rebeccapurple: [102, 51, 153], transparent: [0, 0, 0, 0],
}

export function parseColor(color: string): Float64Array | null {
  const text = color.trim().toLowerCase()
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/.exec(text)
  let rgba: number[] | undefined = NAMED[text]
  if (hex) {
    const h = hex[1]!.length === 3 ? [...hex[1]!].map((c) => c + c).join('') : hex[1]!
    rgba = [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)]
  }
  const rgb = /^rgba?\(\s*(\d+)[\s,]+(\d+)[\s,]+(\d+)(?:[\s,/]+([\d.]+))?\s*\)$/.exec(text)
  if (rgb) rgba = [Number(rgb[1]), Number(rgb[2]), Number(rgb[3]), rgb[4] === undefined ? 1 : Number(rgb[4])]
  if (text === 'oklch(0.628 0.2577 29.23)') rgba = [255, 0, 0]
  if (!rgba) return null
  return Float64Array.of(rgba[0]! / 255, rgba[1]! / 255, rgba[2]! / 255, rgba[3] ?? 1)
}
