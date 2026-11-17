import type { NativeAccessibility } from './contract'
import type { DragIcon, DragInput } from './dnd'
import type { ImeEvent, NativeIme } from './ime'

export interface Modifiers {
  readonly shift?: boolean
  readonly ctrl?: boolean
  readonly alt?: boolean
  readonly meta?: boolean
}

export const WINDOW_EVENTS = [
  'windowopen', 'windowclose', 'windowtitle', 'windowstate',
  'windowactivate', 'windowminsize', 'windowparent', 'windowmenus', 'windowcrash', 'windowhang', 'windowresponsive',
  'windowattention', 'windowready', 'windowshow', 'windowanimated',
] as const
export const WINDOWS_EVENT_NAMES: readonly string[] = WINDOW_EVENTS.map((type) => type.slice('window'.length))
type WindowEventType = (typeof WINDOW_EVENTS)[number]
export interface WindowEvent {
  readonly type: WindowEventType
  readonly window: number
  readonly [field: string]: unknown
}

export type ShellEvent =
  | { type: 'frame'; time: number }
  | { type: 'panelmove'; id: number; x: number; y: number; width: number; height: number }
  | { type: 'pointermove' | 'pointerdown' | 'pointerup'; target: number | null; x: number; y: number; button?: number; modifiers?: Modifiers }
  | { type: 'pointerleave'; modifiers?: Modifiers }
  | { type: 'wheel'; target: number | null; x: number; y: number; deltaX: number; deltaY: number; modifiers?: Modifiers }
  | { type: 'keydown' | 'keyup'; key: string; code: string; keycode?: number; repeat?: boolean; modifiers?: Modifiers; fromIme?: boolean }
  | { type: 'resize'; width: number; height: number; scale: number }
  | DragInput
  | { type: 'dragend'; dropped: boolean }
  | { type: 'close' }
  | { type: 'exit' }

export interface RawShellEvent {
  readonly type: string
  readonly time?: number
  readonly window?: number
  readonly x?: number
  readonly y?: number
  readonly button?: number
  readonly deltaX?: number
  readonly deltaY?: number
  readonly code?: number
  readonly key?: string
  readonly title?: string
  readonly appId?: string
  readonly state?: string
  readonly previous?: string
  readonly width?: number
  readonly height?: number
  readonly scale?: number
  readonly parent?: number | null
  readonly mime?: readonly string[]
  readonly data?: Readonly<Record<string, string>>
  readonly files?: readonly string[]
}

export interface NativeSurface {
  readonly window: number
  readonly hosted?: boolean
  readonly hostControls?: boolean
  readonly minimizable?: boolean
  readonly contentOffset: readonly [number, number]
  setTitle(title: string): void
  setImeArea?(x: number, y: number, width: number, height: number): void
  pump(): RawShellEvent[]
  wait(): Promise<void>
  waitThen?(callback: () => void): void
  requestFrame(): void
  setClipboard(text: string): void
  readClipboard(): Promise<string>
  startDrag(data: Record<string, string>, icon?: DragIcon): void
  requestMode(mode: 'free' | 'fixed' | 'pinned'): void
  requestState(state: 'normal' | 'minimized' | 'filled' | 'fullscreen'): void
  setGrabHint(x: number, y: number, width: number, height: number, edges: number): void
  setBackdrop(x: number, y: number, width: number, height: number, radius: number, blur: number, saturate: number, brightness: number): void
  requestActivate(): void
  requestGrab(kind: 'pointer' | 'keyboard'): void
  releaseGrab(kind: 'pointer' | 'keyboard'): void
  openPanel(id: number, token: bigint, x: number, y: number, width: number, height: number): void
  placePanel(id: number, x: number, y: number, width: number, height: number, ix: number, iy: number, iw: number, ih: number): void
  closePanel(id: number): void
  reservePanel?(id: number, edge: number, thickness: number): void
  setDoubleClick?(action: 'fill' | 'minimize' | 'none'): void
  setFrameTheme?(...args: number[]): void
  setPageDark?(dark: boolean): void
}

export type Shell = Pick<NativeSurface, 'setClipboard' | 'readClipboard' | 'setImeArea'>

export interface NativeDevToolsTarget {
  readonly id: string
  readonly kind: 'page' | 'service'
  readonly title: string
  readonly url: string
}

export interface NativeDevTools {
  open(): { readonly port: number } | null
  targets(targets: NativeDevToolsTarget[]): void
  reloading?(web: string): void
}

export interface Native {
  Shell: { broker(options?: { title?: string }): unknown; readonly hosted?: boolean }
  DevTools?: NativeDevTools
  Finder?: new () => object
  Ime?: new (callback: (event: ImeEvent) => void) => NativeIme
  Appearance?: { open(): Promise<NativeAppearance> }
  FileChooser?: { open(): Promise<NativeFileChooser> }
  Greetd?: NativeGreetd
  Accessibility?: { open(): Promise<NativeAccessibility> }
  DesktopBus?: {
    open(handler: (request: BusRequest) => unknown): NativeDesktopBus
    send(uris: string[], app?: string | null): Promise<void>
  }
}

interface NativeAppearance {
  dark(): Promise<boolean>
  watch(callback: (dark: boolean) => void): void
}

type FileFilter = { name: string; globs: string[]; mimes: string[] }

export interface NativeFileChooser {
  openFile(options: { title: string; multiple: boolean; directory: boolean; filters: FileFilter[] }): Promise<string[] | null>
  saveFile(options: { title: string; name: string; filters: FileFilter[] }): Promise<string | null>
}

export interface GreetUser {
  readonly name: string
  readonly fullName: string
  readonly uid: number
}

interface NativeGreetd {
  available(): boolean
  users(): GreetUser[]
  login(user: string, password: string, cmd: string[], env: string[]): Promise<void>
}

interface BusRequest {
  readonly kind: 'folders' | 'items' | 'properties' | 'open'
  readonly uris: string[]
  readonly app: string
}

export interface NativeDesktopBus {
  readonly names: string[]
  close(): void
}

