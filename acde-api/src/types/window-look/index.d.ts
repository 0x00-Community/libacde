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
  readonly titlebarHeight?: number
  readonly controls?: WindowControls
  readonly title?: WindowTitle
}

export interface WindowControls {
  readonly inset?: number
  readonly size?: number
  readonly gap?: number
  readonly caption?: number
}

export interface WindowTitle {
  readonly size?: number
  readonly weight?: number
}

export type MaterialName = 'glass' | 'blur' | 'none' | (string & {})

export interface MaterialOptions {
  readonly role?: 'regular' | 'clear'
  readonly group?: string
  readonly spacing?: number
  readonly overrides?: Readonly<Record<string, string>>
  readonly region?: 'window' | 'titlebar'
}

export interface WindowMaterial extends MaterialOptions {
  readonly name: MaterialName
}

export interface CurrentMaterial {
  readonly material: string
  readonly options: MaterialOptions
}

export type WallTone = 'light' | 'dark' | null

declare module '../page' {
  interface AcdeWindow {
    setMaterial(material: MaterialName, options?: MaterialOptions): void
    material(): CurrentMaterial
    accent(): string | null
    wallTone(): WallTone
    on(name: 'accent', handler: (color: string | null) => void): () => boolean
    on(name: 'wallTone', handler: (tone: WallTone) => void): () => boolean
  }

  interface AcdeDesktop {
    setWindowLook?(look: WindowLook | null): void
    motion?(preset?: MotionPreset): Motion
  }
}
