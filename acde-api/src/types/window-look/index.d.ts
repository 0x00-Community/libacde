export type CornerShape = 'round' | 'squircle' | `superellipse(${number})`

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

export interface WindowLook {
  readonly corners: WindowCorners
  readonly shadows: WindowShadows
}

declare module '../page' {
  interface AcdeDesktop {
    setWindowLook?(look: WindowLook | null): void
  }
}
