import { Level, Log } from '../../../page/src/log'
import type { Env } from '../../../page/src/sys/process'

export interface Glass {
  readonly level: number
  readonly reducedTransparency: boolean
}

export interface AppearanceChange {
  readonly dark?: boolean
  readonly glass?: number
  readonly reducedTransparency?: boolean
}

export interface AppearancePortal {
  open(): Promise<{
    dark(): Promise<boolean>
    glass(): Promise<{ glass: number; reducedTransparency: boolean }>
    watch(callback: (change: AppearanceChange) => void): void
  }>
}

export function fixedScheme(env: Env): boolean | null {
  const set = env['ACDE_COLOR_SCHEME']
  if (set === 'dark') return true
  if (set === 'light') return false
  if (set) Log.write(Level.Warn, 'acde_launcher', `ACDE_COLOR_SCHEME=${set}: neither light nor dark, following the portal`)
  return null
}

export async function followAppearance(portal: AppearancePortal | undefined, fixedDark: boolean, apply: (change: AppearanceChange) => void): Promise<void> {
  if (!portal) return
  try {
    const opened = await portal.open()
    const followed = ({ dark, ...rest }: AppearanceChange): AppearanceChange => (fixedDark || dark === undefined ? rest : { dark, ...rest })
    opened.watch((change) => apply(followed(change)))
    apply(followed({ ...(await opened.glass()), dark: await opened.dark().catch(() => false) }))
  } catch (error) {
    Log.write(Level.Debug, 'acde_launcher', `appearance: light and the clearest glass, the portal has no setting (${reason(error)})`)
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
