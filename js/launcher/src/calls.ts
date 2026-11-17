export type Answer = 'acknowledge' | 'close' | 'unsupported'

const ACKNOWLEDGED = new Set(['window.ready', 'window.setTitle', 'page.shown'])

export function answer(service: string, method: string, args: readonly unknown[], window: number): Answer {
  const call = `${service}.${method}`
  if (ACKNOWLEDGED.has(call)) return 'acknowledge'
  if (call === 'window.close' || (call === 'windows.close' && args[0] === window)) return 'close'
  return 'unsupported'
}

export function unsupported(service: string, method: string): Error {
  return Object.assign(new Error(`acdev: ${service}.${method} is not supported by the single-app launcher`), { status: 501, code: 'unsupported' })
}
