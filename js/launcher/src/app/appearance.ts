import { Level, Log } from '../../../page/src/log'
import type { Env } from '../../../page/src/sys/process'

export interface AppearancePortal {
  open(): Promise<{ dark(): Promise<boolean>; watch(callback: (dark: boolean) => void): void }>
}

export function fixedScheme(env: Env): boolean | null {
  const set = env['ACDE_COLOR_SCHEME']
  if (set === 'dark') return true
  if (set === 'light') return false
  if (set) Log.write(Level.Warn, 'acde_launcher', `ACDE_COLOR_SCHEME=${set}: neither light nor dark, following the portal`)
  return null
}

export async function followScheme(portal: AppearancePortal | undefined, apply: (dark: boolean) => void): Promise<void> {
  if (!portal) return
  try {
    const opened = await portal.open()
    opened.watch((dark) => apply(dark === true))
    apply(await opened.dark())
  } catch (error) {
    Log.write(Level.Debug, 'acde_launcher', `color scheme: light, the portal has no setting (${reason(error)})`)
  }
}

function reason(error: unknown): string {
  const message = String((error as Error)?.message ?? error)
  try {
    return (JSON.parse(message) as { message?: string }).message || message
  } catch {
    return message
  }
}
