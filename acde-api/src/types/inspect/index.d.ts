import type { WindowState } from '../page'

export interface InspectNode {
  readonly ref: string
  readonly role: string
  readonly name?: string
  readonly value?: string
  readonly states?: readonly string[]
  readonly level?: number
  readonly rect?: readonly [number, number, number, number]
  readonly more?: number
  readonly children?: readonly InspectNode[]
}

export interface InspectWindow {
  readonly ref: string
  readonly id: number
  readonly kind: 'desktop' | 'web' | 'native'
  readonly app: string | null
  readonly title: string | null
  readonly mode: string
  readonly state?: WindowState
  readonly rect: readonly [number, number, number, number]
  readonly z: number
  readonly focus: boolean
  readonly hidden: boolean
  readonly pid: number | null
  readonly parent?: number | null
  readonly modal?: boolean
}

export interface InspectMatch extends InspectNode {
  readonly tag: string
  readonly visible: boolean
  readonly attributes?: Readonly<Record<string, string | null>>
}

export type InspectAction =
  | 'click'
  | 'double'
  | 'right'
  | 'hover'
  | 'drag'
  | 'type'
  | 'settext'
  | 'select'
  | 'key'
  | 'scroll'
  | 'focus'
  | 'close'
  | 'minimize'
  | 'restore'
  | 'fill'
  | 'fullscreen'
  | 'move'
  | 'resize'
  | 'raise'

export type InspectArg = string | number | readonly string[] | null

export interface AcdeInspect {
  snapshot(options?: {
    window?: string
    ref?: string
    all?: boolean
    diff?: boolean
    limit?: number
    find?: string
    tree?: boolean
    offscreen?: boolean
  }): Promise<{ text: string; windows: InspectWindow[]; trees: Record<string, InspectNode[]> }>
  act(ref: string, action: InspectAction, arg?: InspectArg): Promise<{ ok: boolean; text: string }>
  run(
    steps: readonly { ref: string; action: InspectAction; text?: InspectArg }[],
  ): Promise<{ ok: boolean; text: string }>
  open(name: string): Promise<{ ok: boolean; text: string }>
  query(options: {
    selector: string
    window?: string
    ref?: string
    limit?: number
    attributes?: readonly string[]
  }): Promise<{ window: string; count: number; nodes: InspectMatch[] }>
}
