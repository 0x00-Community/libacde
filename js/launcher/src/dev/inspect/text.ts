import type { InspectNode, InspectWindow } from '@acde/api'
import type { Box, MenuGeometry } from './types'

export function menubar(window: InspectWindow, geometry: MenuGeometry): InspectNode {
  const [ox, oy] = window.rect
  const at = ([x, y, w, h]: Box): Box => [x + ox, y + oy, w, h]
  const menus = geometry.menus.map((menu, i): InspectNode => {
    const open = geometry.open?.index === i ? geometry.open : null
    const items = (open?.items ?? []).flatMap((item, k): InspectNode[] => {
      if (item.separator) return []
      const states = [...(item.disabled ? ['disabled'] : []), ...(item.checked ? ['checked'] : []), ...(open?.hover === k ? ['focused'] : [])]
      return [{
        ref: `${window.ref}.m${i + 1}.${k + 1}`,
        role: 'menuitem',
        name: item.label,
        ...(item.shortcut ? { value: item.shortcut } : {}),
        ...(states.length ? { states } : {}),
        rect: at(item.rect),
      }]
    })
    return {
      ref: `${window.ref}.m${i + 1}`,
      role: 'menuitem',
      name: menu.label,
      states: [open ? 'expanded' : 'collapsed'],
      rect: at(menu.rect),
      ...(items.length ? { children: items } : {}),
    }
  })
  return { ref: `${window.ref}.m`, role: 'menubar', children: menus }
}

export function header(w: InspectWindow): string {
  const flags = [w.app ? `app=${w.app}` : '', w.kind === 'native' ? 'native' : '', w.focus ? 'focused' : '', w.hidden ? 'minimized' : '']
  if (w.mode !== 'free') flags.push(w.mode)
  if ((w.state === 'filled' || w.state === 'fullscreen') && !flags.includes(w.state)) flags.push(w.state)
  const title = w.title ? ` ${JSON.stringify(w.title)}` : ''
  return `- ${w.kind === 'desktop' ? 'desktop' : 'window'}${title}${flags.map((f) => (f ? ` [${f}]` : '')).join('')} [${w.ref}]`
}

export function outline(windows: InspectWindow[], headers: string[], bodies: Map<string, string[]>, same: boolean): string[] {
  const lines: string[] = same ? ['(windows unchanged)'] : []
  for (const [i, window] of windows.entries()) {
    const body = bodies.get(window.ref)
    if (same && !body) continue
    lines.push(headers[i]!, ...(body ?? []).map((line) => `  ${line}`))
  }
  return lines
}
