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
  readonly stage?: boolean
  readonly topmost?: boolean
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
  | 'topmost'

export type InspectArg = string | number | readonly string[] | null

export type InspectPixelInput =
  | { readonly type: 'move'; readonly x: number; readonly y: number }
  | { readonly type: 'motion'; readonly dx: number; readonly dy: number }
  | { readonly type: 'down' | 'up'; readonly x?: number; readonly y?: number; readonly button?: 'left' | 'right' | 'middle' }
  | { readonly type: 'wheel'; readonly x?: number; readonly y?: number; readonly dx?: number; readonly dy?: number }
  | { readonly type: 'keydown' | 'keyup'; readonly key: string }
  | { readonly type: 'text'; readonly text: string }
  | { readonly type: 'wait'; readonly ms: number }

export type InspectShot =
  | {
    readonly ok: true
    readonly png: string
    readonly width: number
    readonly height: number
    readonly window: { readonly x: number; readonly y: number; readonly width: number; readonly height: number }
    readonly stage?: boolean
  }
  | { readonly ok: false; readonly text: string }

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
  open(name: string): Promise<{ ok: boolean; text: string; stage?: boolean }>
  shot(window: string | number, options?: { max?: number }): Promise<InspectShot>
  input(
    window: string | number,
    events: readonly InspectPixelInput[],
  ): Promise<{ ok: true; done: boolean; textSkipped: number; stage?: boolean } | { ok: false; text: string; code?: string; app?: string }>
  query(options: {
    selector: string
    window?: string
    ref?: string
    limit?: number
    attributes?: readonly string[]
  }): Promise<{ window: string; count: number; nodes: InspectMatch[] }>
}
