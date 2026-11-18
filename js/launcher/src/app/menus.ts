import type { WireMenu } from '../../../page/src/contract'
import { Level, Log } from '../../../page/src/log'

export interface MenuKey {
  readonly menu: string
  readonly item: string
}

export interface MenuShell {
  setMenus?(window: number, menus: string): void
  shortcut?(combo: string, scope: string, owner: number): void
  unshortcut?(combo: string, scope: string, owner: number): void
}

export class AppMenus {
  #keys = new Map<string, MenuKey>()

  constructor(
    readonly shell: MenuShell,
    readonly window: () => number,
    readonly page: ((chords: readonly string[]) => void) | null = null,
  ) {}

  remember(menus: readonly WireMenu[]): void {
    this.#keys = AppMenus.keys(menus)
    this.page?.([...this.#keys.keys()])
  }

  set(menus: readonly WireMenu[]): void {
    const window = this.window()
    this.shell.setMenus?.(window, JSON.stringify(menus))
    const next = AppMenus.keys(menus)
    for (const combo of this.#keys.keys()) if (!next.has(combo)) this.shell.unshortcut?.(combo, 'menu', window)
    for (const combo of next.keys()) {
      if (this.#keys.has(combo)) continue
      try {
        this.shell.shortcut?.(combo, 'menu', window)
      } catch (error) {
        next.delete(combo)
        Log.write(Level.Warn, 'acde_host', `window ${window}: menu shortcut ${combo}: ${(error as Error).message}`)
      }
    }
    this.#keys = next
  }

  pressed(combo: string): MenuKey | null {
    return this.#keys.get(combo) ?? null
  }

  static keys(menus: readonly WireMenu[]): Map<string, MenuKey> {
    const keys = new Map<string, MenuKey>()
    for (const menu of menus) {
      for (const item of menu.items) {
        if ('type' in item || !item.shortcut || !item.action || item.disabled || keys.has(item.shortcut)) continue
        keys.set(item.shortcut, { menu: menu.id, item: item.id })
      }
    }
    return keys
  }
}
