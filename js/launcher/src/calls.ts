export type Route =
  | 'acknowledge'
  | 'close'
  | 'minimize'
  | 'reload'
  | 'capabilities'
  | 'panel'
  | 'windows'
  | 'manager'
  | 'unsupported'

const ACKNOWLEDGED = new Set(['window.ready', 'window.setTitle', 'page.shown'])
const LOCAL: Readonly<Record<string, Route>> = {
  'window.close': 'close',
  'window.minimize': 'minimize',
  'window.requestState': 'minimize',
  'window.reload': 'reload',
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
  if (caller.managed) return 'manager'
  return ACKNOWLEDGED.has(call) ? 'acknowledge' : 'unsupported'
}

export function unsupported(service: string, method: string): Error {
  return Object.assign(new Error(`acdev: ${service}.${method} is not supported by the single-app launcher`), { status: 501, code: 'unsupported' })
}

export function refused(what: string): Error {
  return Object.assign(new Error(`acdev: ${what}`), { status: 400, code: 'bad_request' })
}
