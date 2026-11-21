export type Route =
  | 'acknowledge'
  | 'close'
  | 'minimize'
  | 'reload'
  | 'devreload'
  | 'capabilities'
  | 'panel'
  | 'windows'
  | 'manager'
  | 'session'
  | 'lock'
  | 'title'
  | 'menus'
  | 'page'
  | 'unsupported'

const ACKNOWLEDGED = new Set(['window.ready', 'page.shown'])
const PAGE = new Set(['open', 'save', 'notify'])
const PICKERS = new Set(['open', 'save'])
const LOCAL: Readonly<Record<string, Route>> = {
  'window.close': 'close',
  'window.minimize': 'minimize',
  'window.requestState': 'minimize',
  'window.reload': 'reload',
  'page.devreload': 'devreload',
  'capabilities.get': 'capabilities',
  'capabilities.list': 'capabilities',
}

export interface Caller {
  readonly window: number
  readonly desktop: boolean
  readonly managed: boolean
}

export function route(service: string, method: string, args: readonly unknown[], caller: Caller): Route {
  const call = `${service}.${method}`
  const local = LOCAL[call]
  if (local) return local
  if (call === 'windows.close' && args[0] === caller.window && !caller.desktop) return 'close'
  if (caller.desktop && service === 'panel') return 'panel'
  if (caller.desktop && service === 'windows') return 'windows'
  if (service === 'page' && PICKERS.has(method)) return 'page'
  if (caller.managed) return 'manager'
  if (service === 'session' || service === 'desktops') return 'session'
  if (service === 'lock') return 'lock'
  if (call === 'window.setTitle') return 'title'
  if (call === 'window.setMenus') return 'menus'
  if (service === 'page' && PAGE.has(method)) return 'page'
  return ACKNOWLEDGED.has(call) ? 'acknowledge' : 'unsupported'
}

export function unsupported(service: string, method: string): Error {
  return Object.assign(new Error(`acdev: ${service}.${method} is not supported by the single-app launcher`), { status: 501, code: 'unsupported' })
}

export function refused(what: string): Error {
  return Object.assign(new Error(`acdev: ${what}`), { status: 400, code: 'bad_request' })
}

export function wallpaperOf(given: unknown): { color: string; image?: string | null } {
  const { color, image } = (given ?? {}) as { color?: unknown; image?: unknown }
  if (typeof color !== 'string' || !/^#(?:[0-9a-f]{6}|[0-9a-f]{8})$/i.test(color)) {
    throw Object.assign(new Error(`acdev: windows.setWallpaper: ${String(color)} is not #rrggbb or #rrggbbaa`), { status: 400, code: 'bad_color' })
  }
  if (image !== undefined && image !== null && (typeof image !== 'string' || !image.startsWith('/'))) {
    throw refused('windows.setWallpaper: image is an absolute path or null')
  }
  return { color, ...(image !== undefined && { image }) }
}
