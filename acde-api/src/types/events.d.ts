import type { DesktopApp, WindowState } from './page'

interface WindowsEventOf<T extends string> {
  readonly type: T
  readonly window: number
}

export interface WindowOpenEvent extends WindowsEventOf<'windowopen'> {
  readonly appId?: string | null
  readonly title?: string | null
  readonly web?: true
  readonly icon?: string
}

export interface WindowTitleEvent extends WindowsEventOf<'windowtitle'> {
  readonly appId?: string | null
  readonly title?: string | null
}

export interface WindowsStateEvent extends WindowsEventOf<'windowstate'> {
  readonly state: WindowState
  readonly previous: WindowState
}

export interface WindowMinSizeEvent extends WindowsEventOf<'windowminsize'> {
  readonly width: number
  readonly height: number
}

export interface WindowParentEvent extends WindowsEventOf<'windowparent'> {
  readonly parent?: number
}

export interface WindowMenuItem {
  readonly id: string
  readonly label: string
  readonly shortcut?: string
  readonly checked?: boolean
  readonly disabled?: boolean
  readonly action: boolean
}

export interface WindowMenu {
  readonly id: string
  readonly label: string
  readonly items: readonly ({ readonly type: 'separator' } | WindowMenuItem)[]
}

export interface WindowMenusEvent extends WindowsEventOf<'windowmenus'> {
  readonly menus: readonly WindowMenu[]
}

export interface WindowCrashEvent extends WindowsEventOf<'windowcrash'> {
  readonly reason: string
}

export interface WindowsEventMap {
  readonly open: WindowOpenEvent
  readonly close: WindowsEventOf<'windowclose'>
  readonly title: WindowTitleEvent
  readonly state: WindowsStateEvent
  readonly activate: WindowsEventOf<'windowactivate'>
  readonly minsize: WindowMinSizeEvent
  readonly parent: WindowParentEvent
  readonly menus: WindowMenusEvent
  readonly crash: WindowCrashEvent
  readonly hang: WindowsEventOf<'windowhang'>
  readonly responsive: WindowsEventOf<'windowresponsive'>
  readonly attention: WindowsEventOf<'windowattention'>
  readonly ready: WindowsEventOf<'windowready'>
  readonly show: WindowsEventOf<'windowshow'>
  readonly animated: WindowsEventOf<'windowanimated'>
}

export type WindowsEventType = keyof WindowsEventMap

export type WindowsEvent = WindowsEventMap[WindowsEventType]

export interface WindowsEvents {
  on<K extends WindowsEventType>(type: K, handler: (event: WindowsEventMap[K]) => void): () => boolean
}

export interface AppsChangedEvent {
  readonly type: 'appschanged'
  readonly apps: readonly DesktopApp[]
}

export interface NotificationEvent {
  readonly type: 'notification'
  readonly window: number
  readonly app: string | null
  readonly label: string | null
  readonly title: string
  readonly body: string
  readonly icon: string
  readonly tag: string
}

export interface DownloadEvent {
  readonly type: 'download'
  readonly window: number
  readonly app: string | null
  readonly path: string
  readonly name: string
  readonly size: number
}

export type ScreenshotEvent =
  | { readonly type: 'screenshot'; readonly path: string; readonly width: number; readonly height: number }
  | { readonly type: 'screenshot'; readonly path: string; readonly error: string }

export type ShortcutScope = 'session' | 'desktop' | 'app'

export interface ShortcutEvent {
  readonly type: 'shortcut'
  readonly window: number
  readonly key: string
  readonly title: ShortcutScope
}

export type ScreenEdge = 'top' | 'right' | 'bottom' | 'left'

export interface ScreenEdgeEvent {
  readonly type: 'screenedge'
  readonly edge: ScreenEdge
  readonly entered: boolean
  readonly output: string
}

export interface DesktopEventMap {
  readonly download: DownloadEvent
  readonly edge: ScreenEdgeEvent
  readonly notification: NotificationEvent
  readonly screenshot: ScreenshotEvent
  readonly shortcut: ShortcutEvent
}

export type TerminalEvent =
  | { readonly type: 'output'; readonly data: string }
  | { readonly type: 'cwd'; readonly path: string }
  | { readonly type: 'exit'; readonly code: number | null; readonly signal: number | null }

export interface WindowGrabEvent {
  readonly kind: 'pointer' | 'keyboard'
  readonly granted: boolean
}

export interface WindowGrabLostEvent {
  readonly kind: 'lost'
  readonly reason: 'esc' | 'other_window' | 'focus_lost' | 'denied'
}
