import type { AcdePage } from './page'

export type AcdeModules = {
  readonly [K in keyof AcdePage as string extends K ? never : number extends K ? never : K]-?: NonNullable<AcdePage[K]>
}
