import type { AnchorWire } from '../../page/src/pinanchor'
import type { ShellEvent } from './app/events'
import type { DevShell } from './dev/dev'
import type { InspectShell } from './dev/inspect'

export interface Grant {
  readonly window: number
  readonly token: bigint
}

export interface GrantOptions {
  readonly desktop?: boolean
  readonly window?: number
  readonly mode?: 'fullscreen' | 'free' | 'pinned'
  readonly size?: readonly number[]
  readonly outerSize?: readonly number[]
  readonly rect?: readonly number[]
  readonly host?: boolean
  readonly anchor?: AnchorWire
}

export interface LauncherShell extends DevShell, Omit<InspectShell, 'focus' | 'setState' | 'windows' | 'inject' | 'place' | 'play'> {
  devEvent?(json: string): number
  setMenus?(window: number, menus: string): void
  notify?(title: string, body: string, subtitle: string): void
  readonly socketName?: string
  pump(): ShellEvent[]
  onEvents(callback: () => void): void
  grant(options: GrantOptions): Grant
  place(window: number, x: number, y: number, width: number, height: number): void
  hide(window: number, hidden: boolean): void
  release?(window: number): void
  readonly anchors?: boolean
  reanchor?(window: number, anchor: AnchorWire): void
  focus(window: number): void
  close(window: number): void
  kill?(window: number): void
  setState?(window: number, state: 'normal' | 'minimized' | 'filled' | 'fullscreen'): void
  setAside?(window: number, on: boolean): boolean
  titlebarHeight?(): number
  windows(): ReturnType<InspectShell['windows']>
  inject?(event: Parameters<InspectShell['inject']>[0]): void
  injectSequence?(events: Parameters<InspectShell['play']>[0]): Promise<unknown>
  screenshot(path: string): Promise<{ width: number; height: number }>
  animate?(window: number, options: object): void
  shortcut?(combo: string, scope: string, owner: number): void
  unshortcut?(combo: string, scope: string, owner: number): void
  stats?(reset?: boolean): Record<string, unknown>
  addDesk?(desk: number): unknown
  removeDesk?(desk: number, into: number): unknown
  showDesk?(desk: number): unknown
  moveToDesk?(window: number, desk: number): unknown
  setWallpaper?(color: string, image?: string | null): unknown
}
