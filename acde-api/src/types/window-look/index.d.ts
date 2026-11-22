export type CornerShape = 'round' | 'squircle' | `superellipse(${number})`

export interface BackdropEffect {
  readonly radius?: number
  readonly blur?: number
  readonly saturate?: number
  readonly brightness?: number
  readonly shape?: CornerShape
}

export interface WindowCorners {
  readonly radius: number
  readonly shape?: CornerShape
}

export interface ShadowLayer {
  readonly x: number
  readonly y: number
  readonly blur: number
  readonly spread: number
  readonly color: string
}

export interface WindowShadows {
  readonly focused: readonly ShadowLayer[]
  readonly unfocused: readonly ShadowLayer[]
}

export type MotionPreset = 'standard' | 'reduced'

export interface Motion {
  readonly preset: MotionPreset
  readonly duration: number
  readonly easing: string
}

export interface WindowLook {
  readonly corners?: WindowCorners
  readonly shadows?: WindowShadows
  readonly motion?: MotionPreset
}

declare module '../page' {
  interface AcdeDesktop {
    setWindowLook?(look: WindowLook | null): void
    motion?(preset?: MotionPreset): Motion
  }
}
