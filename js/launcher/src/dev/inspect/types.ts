import type { InspectNode, WindowState } from '@acde/api'
import type { Rpc } from '../../../../page/src/rpc'

export interface ShellWindow {
  readonly id: number
  readonly state?: 'normal' | 'minimized' | 'filled' | 'fullscreen'
  readonly window?: number
  readonly canvas: number
  readonly mode: string
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
  readonly z: number
  readonly focus: boolean
  readonly hidden?: boolean
  readonly pid?: number | null
  readonly parent?: number | null
  readonly modal?: boolean
  readonly minimize?: boolean
}

export interface InjectEvent {
  type: 'move' | 'motion' | 'down' | 'up' | 'wheel' | 'keydown' | 'keyup'
  x?: number
  y?: number
  button?: number
  dx?: number
  dy?: number
  source?: 'wheel' | 'finger' | 'continuous'
  code?: number
}

export interface InspectShell {
  focus(window: number): void
  setState?(window: number, state: 'normal' | 'minimized' | 'filled' | 'fullscreen'): void
  windows(): ShellWindow[]
  menus?(window: number): string | null
  inject(event: InjectEvent): void
}

export interface NativeAccessibility {
  tree(pid: number, title: string | null, limit: number): Promise<string | null>
  act(node: string, action: string, text?: string | null): Promise<boolean>
}

export interface InspectHost {
  readonly shell: InspectShell
  web(window: number): Rpc | null
  record(window: number): { readonly kind: 'native' | 'web'; readonly app: string | null; readonly title: string | null; readonly state?: WindowState } | null
  panel(window: number): boolean
  close(window: number): Promise<unknown>
  hide(window: number, hidden: boolean): void
  open(name: string): Promise<string>
  accessibility(): Promise<NativeAccessibility | null>
  records(): readonly number[]
  ref?(window: number): string | null
}

export interface NativeNode {
  readonly id: string
  readonly role: string
  readonly name?: string
  readonly value?: string
  readonly states?: readonly string[]
  readonly rect?: readonly [number, number, number, number] | null
  readonly actions?: readonly string[]
  readonly more?: number
  readonly children?: readonly NativeNode[]
}

export interface Aliases {
  readonly byKey: Map<string, number>
  readonly byNumber: Map<number, { key: string; rect: readonly [number, number, number, number] | null }>
  next: number
}

export interface Tree {
  readonly nodes: InspectNode[]
  readonly notes: string[]
}

export type Box = readonly [number, number, number, number]

export interface MenuGeometry {
  readonly menus: readonly { readonly id: string; readonly label: string; readonly rect: Box }[]
  readonly open: {
    readonly index: number
    readonly hover: number | null
    readonly items: readonly {
      readonly label: string
      readonly shortcut: string | null
      readonly checked: boolean
      readonly disabled: boolean
      readonly separator: boolean
      readonly rect: Box
    }[]
  } | null
}

export const LEFT = 0x110
export const RIGHT = 0x111
export const MIDDLE = 0x112
export const MODIFIERS = new Set(['shift', 'ctrl', 'control', 'alt', 'option', 'meta', 'super', 'cmd', 'command'])

export type InspectInput =
  | { readonly type: 'move'; readonly x: number; readonly y: number }
  | { readonly type: 'down' | 'up'; readonly x: number; readonly y: number; readonly button?: 'left' | 'right' | 'middle' }
  | { readonly type: 'wheel'; readonly x: number; readonly y: number; readonly dx?: number; readonly dy?: number }
  | { readonly type: 'keydown' | 'keyup'; readonly key: string }
  | { readonly type: 'text'; readonly text: string }
  | { readonly type: 'wait'; readonly ms: number }

export type Point = { readonly x: number; readonly y: number } | { readonly error: string }
