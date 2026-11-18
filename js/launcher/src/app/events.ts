export interface ShellEvent {
  readonly type: string
  readonly window?: number
  readonly [field: string]: unknown
}

const NUMBER = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value)

export function toDesktop(event: ShellEvent): ShellEvent {
  if (event.type !== 'windowgeometry' || (event['rect'] && typeof event['rect'] === 'object')) return event
  const { x, y, width, height } = event
  if (![x, y, width, height].every(NUMBER)) return event
  const { x: _x, y: _y, width: _w, height: _h, ...rest } = event
  return { ...rest, rect: { x, y, width, height } }
}
