import { Level, Log } from '../../../page/src/log'
import type { Env } from '../../../page/src/sys/process'
import type { WallTone } from '@acde/api'

export interface Glass {
  readonly level: number
  readonly reducedTransparency: boolean
}

export interface AppearanceChange {
  readonly dark?: boolean
  readonly accent?: string | null
  readonly glass?: number
  readonly reducedTransparency?: boolean
  readonly reducedMotion?: boolean
  readonly wallTones?: Readonly<Record<string, WallTone>>
  readonly primaryOutput?: string | null
}

export interface AppearancePortal {
  open(): Promise<{
    dark(): Promise<boolean>
    accent(): Promise<string | null>
    walls(): Promise<{ wallTones: Record<string, WallTone>; primaryOutput: string | null }>
    glass(): Promise<{ glass: number; reducedTransparency: boolean; reducedMotion: boolean }>
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
    const accent = await opened.accent().catch(() => null)
    const walls = await opened.walls().catch(() => ({}))
    apply(followed({ ...(await opened.glass()), ...walls, accent, dark: await opened.dark().catch(() => false) }))
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

type Walls = Pick<AppearanceChange, 'wallTones' | 'primaryOutput'>

export class WallToneOf {
  #tones: Readonly<Record<string, WallTone>> = {}
  #primary: string | null = null
  #tone: WallTone = null

  constructor(readonly output: () => string | null, readonly changed: (tone: WallTone) => void) {}

  get tone(): WallTone {
    return this.#tone
  }

  take({ wallTones, primaryOutput }: Walls): void {
    if (wallTones === undefined && primaryOutput === undefined) return
    if (wallTones !== undefined) this.#tones = wallTones
    if (primaryOutput !== undefined) this.#primary = primaryOutput
    this.check()
  }

  check(): void {
    const tone = WallToneOf.pick(this.#tones, this.output(), this.#primary)
    if (tone === this.#tone) return
    this.#tone = tone
    this.changed(tone)
  }

  static pick(tones: Readonly<Record<string, WallTone>>, output: string | null, primary: string | null): WallTone {
    const of = (name: string | null) => (name !== null && Object.hasOwn(tones, name) ? (tones[name] ?? null) : undefined)
    const own = of(output)
    return own !== undefined ? own : (of(primary) ?? null)
  }
}
