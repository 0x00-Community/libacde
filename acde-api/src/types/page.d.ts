import type { AppsChangedEvent, DesktopEventMap, ShortcutEvent, WindowGrabEvent, WindowGrabLostEvent, WindowsEvents } from './events'
import type { AcdeIcons } from './names'
import type { AcdeOffice } from './office'
import type { AcdePermissions } from './permissions'
import type { SharedPlace } from './places'
import type { AcdeSoftware } from './software'
import type { AcdeTerminals } from './terminal/index'
import type { AcdeLock } from './lock/index'
import type { AcdeSession } from './session/index'
import type { AcdeInspect } from './inspect/index'
import type { AcdeAudio, AcdeBluetooth, AcdeDevices, AcdeDisplay, AcdeInput, AcdePower } from './hardware/index'

export type * from './events'
export type * from './office'
export type * from './permissions'
export type * from './software'
export type * from './terminal/index'
export type * from './lock/index'
export type * from './session/index'
export type * from './inspect/index'
export type * from './hardware/index'
export type { AcdeIcons, IconsChangedEvent } from './names'

export interface FinderRoot {
  id: string
  path?: string
  label: string
  source: 'desktop' | 'downloads' | 'default' | 'granted' | 'private' | 'place' | 'lent' | 'removable'
  place?: SharedPlace
  drive?: string
  mode: RootMode
  reason: string | null
  exists: boolean
}

export type RootMode = 'writable' | 'read_only' | 'unavailable'

export type EntryKind = 'directory' | 'file' | 'symlink' | 'other'

export interface FinderEntry {
  name: string
  kind: EntryKind
  size: number
  modifiedMs: number
  target?: string
  version?: string
  inPlace?: true
}

export interface FinderPreview {
  path: string
  name: string
  size: number
  modifiedMs: number
  kind: 'text' | 'image' | 'binary' | 'too_large'
  content?: string
  truncated?: boolean
}

export interface FileBytes {
  readonly name: string
  readonly contentType: string
  readonly data: Uint8Array
}

export type AcdeFsErrorCode =
  | 'EINVAL'
  | 'EACCES'
  | 'ENOENT'
  | 'EEXIST'
  | 'ECONFLICT'
  | 'ENOTEMPTY'
  | 'EFBIG'
  | 'EILSEQ'
  | 'ENOSPC'
  | 'EIO'

export interface AcdeFsError extends Error {
  readonly status: number
  readonly code: AcdeFsErrorCode
}

export interface FinderStat extends FinderEntry {
  path: string
}

export interface FileText {
  readonly text: string
  readonly modifiedMs: number
  readonly size: number
  readonly version: string
}

export interface FileData {
  readonly data: Uint8Array
  readonly modifiedMs: number
  readonly size: number
  readonly version: string
}

export interface FileRange {
  readonly data: Uint8Array
  readonly offset: number
  readonly size: number
  readonly modifiedMs: number
  readonly version: string
}

export interface ReadRangeOptions {
  ifVersion?: string
}

export interface OpenWriteOptions {
  keepExistingData?: boolean
  ifVersion?: string
  create?: 'new' | 'always' | 'never'
  mkdirs?: boolean
}

export interface WriteFileOptions {
  ifModifiedMs?: number
  ifVersion?: string
  create?: 'new' | 'always' | 'never'
  mkdirs?: boolean
}

export interface MkdirOptions {
  recursive?: boolean
}

export interface AcdeFs {
  roots(): Promise<FinderRoot[]>
  list(
    root: string,
    path?: string,
    hidden?: boolean,
  ): Promise<{ root: FinderRoot; path: string; entries: FinderEntry[]; truncated: boolean; skipped?: number }>
  preview(root: string, path: string): Promise<FinderPreview>
  download(root: string, path: string): Promise<FileBytes>
  mkdir(root: string, path: string, options?: MkdirOptions): Promise<FinderEntry>
  stat(root: string, path: string): Promise<FinderStat>
  readText(root: string, path: string): Promise<FileText>
  readBytes(root: string, path: string): Promise<FileData>
  writeFile(root: string, path: string, data: string | Uint8Array, options?: WriteFileOptions): Promise<FinderEntry>
  readRange(root: string, path: string, offset: number, length: number, options?: ReadRangeOptions): Promise<FileRange>
  openWrite(root: string, path: string, options?: OpenWriteOptions): Promise<{ id: string; size: number }>
  writeChunk(id: string, offset: number, data: string | Uint8Array): Promise<void>
  truncateWrite(id: string, size: number): Promise<void>
  commitWrite(id: string): Promise<FinderEntry>
  abortWrite(id: string): Promise<void>
  copy(fromRoot: string, fromPath: string, toRoot: string, toDir: string): Promise<FinderEntry & { path: string }>
  rename(root: string, from: string, to: string): Promise<FinderEntry>
  delete(root: string, path: string): Promise<{ status: 'deleted'; path: string }>
  locate(path: string): Promise<{ root: string; path: string }>
  openPath(root: string, path: string): Promise<{ root: string; path: string; file: string }>
  revoke(rootPath: string): Promise<void>
  on?(type: 'roots', handler: (event: FsRootsEvent) => void): () => boolean
}

export interface FsRootsEvent {
  readonly added?: readonly string[]
  readonly removed: readonly string[]
}

export interface NativeWindow {
  readonly id: number
  readonly appId: string | null
  readonly title: string | null
  readonly web?: boolean
  readonly state: WindowState
  readonly rect?: WindowRect
  readonly instance?: number
  readonly desk?: number
  readonly key?: string
  readonly stage?: boolean
}

export interface AcdeWindows extends WindowsEvents {
  list(): NativeWindow[]
  launch(
    argv: string[],
    options?: { cwd?: string; env?: Record<string, string>; slot?: boolean },
  ): number | Promise<number>
  close(id: number): void
  requestState?(id: number, state: WindowState): Promise<void>
  kill?(id: number): Promise<boolean>
  focus?(id: number): Promise<void>
  hide?(id: number): Promise<void>
  show?(id: number): Promise<void>
  invokeMenu?(id: number, menu: string, item: string): Promise<void>
  stats?(): Promise<Readonly<Record<string, number>>>
  animate?(
    id: number,
    options: {
      opacity?: number
      scale?: number
      dx?: number
      dy?: number
      duration?: number
      delay?: number
      easing?: 'linear' | 'ease' | 'ease-in' | 'ease-out' | 'ease-in-out'
    },
  ): Promise<void>
  shortcut?(combo: string, scope?: 'session' | 'desktop'): Promise<void>
  unshortcut?(combo: string, scope?: 'session' | 'desktop'): Promise<void>
  place?(id: number, rect: readonly [x: number, y: number, width: number, height: number]): Promise<void>
  addDesk?(desk: number): Promise<void>
  removeDesk?(desk: number, into: number): Promise<void>
  showDesk?(desk: number): Promise<void>
  moveToDesk?(window: number, desk: number): Promise<void>
  setWallpaper?(wallpaper: DisplayWallpaper): Promise<void>
  screenshot?(options?: ScreenshotOptions): Promise<Screenshot>
}

export interface ScreenshotOptions {
  readonly screen?: number
  readonly window?: number
  readonly cursor?: boolean
  readonly path?: string
}

export interface Screenshot {
  readonly path: string
  readonly width: number
  readonly height: number
}

export interface DisplayWallpaper {
  readonly color: string
  readonly image?: string | null
}

export interface DesktopApp {
  readonly id: string
  readonly label: string
  readonly icon: string
  readonly script: string
}

export interface OpenHandler {
  readonly kind: 'acde' | 'native'
  readonly id: string
  readonly label: string
  readonly icon: string
  readonly default: boolean
  readonly system?: boolean
}

export interface AcdeOpenFiles {
  openPath(
    path: string,
    options?: { app?: string; select?: boolean },
  ): Promise<{ kind: 'acde' | 'native'; id: string; window?: number; instance?: number }>
  handlers(path: string): Promise<{ mime: string; association: string | null; handlers: OpenHandler[] }>
  setDefault(mime: string, handler: string): Promise<string>
}

export interface NativeApp {
  readonly id: string
  readonly name: string
  readonly comment: string | null
  readonly icon: string
  readonly keywords: readonly string[]
  readonly wmClass: string | null
}

export interface LentRoot {
  readonly name: string
  readonly dir: string
  readonly label?: string
}

export interface OpenWindowOptions {
  readonly parent?: number
  readonly modal?: boolean
  readonly root?: LentRoot
  readonly newWindow?: boolean
  readonly size?: readonly [width: number, height: number]
}

export interface OpenFileRequest {
  readonly kind: 'open-file'
  readonly handle?: FileSystemFileHandle | FileSystemDirectoryHandle
  readonly url?: string
  readonly root?: string
  readonly path?: string
  readonly file?: string | null
}

export interface OpenLinkRequest {
  readonly kind: 'open-link'
  readonly url: string
}

export type SystemRequest = OpenFileRequest | OpenLinkRequest

export type AppRequest<K extends string = string, Fields extends object = Record<never, never>> = { readonly kind: K } & {
  readonly [P in keyof Fields]: Fields[P]
}

export type WindowRequest = SystemRequest | AppRequest

export interface Opened {
  readonly kind: 'acde' | 'native'
  readonly id: string
  readonly window?: number
  readonly instance?: number
}

export interface AcdeOpen {
  (request: SystemRequest): Promise<Opened>
  (app: string, request: SystemRequest): Promise<Opened>
  <R extends AppRequest>(app: string, request: R, options?: OpenWindowOptions): Promise<{ window: number; instance?: number; running?: boolean }>
  (app: string, request?: null, options?: OpenWindowOptions): Promise<{ window: number; instance?: number; running?: boolean }>
}

export interface AcdeAppList {
  on?(type: 'changed', handler: (event: AppsChangedEvent) => void): () => boolean
  list(): Promise<DesktopApp[]>
  open: AcdeOpen
}

export interface ServicesChangedEvent {
  readonly names: readonly string[]
  readonly added: readonly string[]
  readonly removed: readonly string[]
}

export interface AcdeServices {
  names(): string[]
  on(type: 'changed', handler: (event: ServicesChangedEvent) => void): () => boolean
}

export interface AcdeApps extends AcdeAppList, AcdeOpenFiles {
  refresh(): Promise<DesktopApp[]>
  install(dir: string): Promise<{ id: string; dir: string; apps: DesktopApp[] }>
  natives(): Promise<NativeApp[]>
  runNative(id: string): Promise<number>
  unlend?(dir: string): Promise<{ revoked: number }>
}

export interface AcdePackages {
  on(type: 'changed', handler: (event: AppsChangedEvent) => void): () => boolean
  refresh(): Promise<DesktopApp[]>
  install(dir: string): Promise<{ id: string; dir: string; apps: DesktopApp[] }>
  natives(): Promise<NativeApp[]>
  runNative(id: string): Promise<number>
}

export interface LaunchOptions {
  readonly cwd?: string
  readonly env?: Readonly<Record<string, string>>
}

export interface AcdeLaunch extends WindowsEvents {
  start(argv: readonly string[], options?: LaunchOptions): Promise<number>
  embed(argv: readonly string[], options?: LaunchOptions): Promise<number>
  close(window: number): Promise<boolean>
  list(): NativeWindow[]
}

export interface DesktopInfo {
  readonly kind: 'acde' | 'session'
  readonly id: string
  readonly name: string
  readonly description: string | null
  readonly package: string | null
}

export interface AcdeDesktops {
  list(): Promise<{ desktops: DesktopInfo[]; current: string }>
  choose(id: string, remember?: boolean): Promise<void>
}

export interface AppMenuItem {
  readonly id: string
  readonly label: string
  readonly shortcut?: string
  readonly checked?: boolean
  readonly disabled?: boolean
  onSelect?(): void | Promise<void>
}

export interface AppMenu {
  readonly id: string
  readonly label: string
  readonly items: readonly ({ readonly type: 'separator' } | AppMenuItem)[]
}

export interface WindowHost {
  setTitle(title: string): void
  setMenus(menus: readonly AppMenu[]): void
}

export interface WindowApp {
  mount(element: Element, host: WindowHost): () => void
}

export type WindowEventName =
  | 'activate'
  | 'deactivate'
  | 'beforeclose'
  | 'request'
  | 'suspend'
  | 'resume'
  | 'grab'
  | 'grablost'
  | 'state'
  | 'output'
  | 'hostcontrols'
  | 'minimizable'

export type WindowMode = 'free' | 'fixed' | 'pinned'
export type WindowState = 'normal' | 'minimized' | 'filled' | 'fullscreen'
export interface WindowStateEvent {
  readonly state: WindowState
  readonly previous: WindowState
}
export type WindowEdge = 'top' | 'bottom' | 'left' | 'right'
export interface WindowRect {
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
}

export interface WindowOutput {
  readonly width: number
  readonly height: number
  readonly scale: number
  readonly workArea: WindowRect
}

export interface AcdeWindowError extends Error {
  readonly code: 'unavailable'
  readonly status: 409
}

export interface AcdeWindow {
  readonly id: number
  readonly state: WindowState
  readonly output: WindowOutput
  readonly hostControls: boolean
  readonly minimizable: boolean
  readonly host: WindowHost
  mount(app: WindowApp): void
  setTitle(title: string): void
  setMenus(menus: readonly AppMenu[]): void
  close(): Promise<boolean>
  minimize(): Promise<void>
  requestAttention(): Promise<void>
  requestState(state: WindowState): void
  requestMode(mode: WindowMode): void
  setGrabHint(rect: WindowRect, edges?: readonly WindowEdge[]): void
  setBackdrop(
    rect: WindowRect | null,
    effect?: import('./window-look/index').BackdropEffect,
  ): void
  requestActivate(): void
  requestGrab(kind: 'pointer' | 'keyboard'): void
  releaseGrab(kind: 'pointer' | 'keyboard'): void
  shortcut(combo: string): Promise<void>
  unshortcut(combo: string): Promise<void>
  on(name: 'beforeclose', handler: () => boolean | Promise<boolean>): () => boolean
  on(name: 'beforeclose', handler: () => void | Promise<void>): () => boolean
  on<R extends AppRequest = AppRequest>(name: 'request', handler: (request: SystemRequest | R) => void): () => boolean
  on(name: 'suspend' | 'resume', handler: () => void): () => boolean
  on(name: 'activate' | 'deactivate', handler: () => void): () => boolean
  on(name: 'grab', handler: (event: WindowGrabEvent) => void): () => boolean
  on(name: 'grablost', handler: (event: WindowGrabLostEvent) => void): () => boolean
  on(name: 'state', handler: (event: WindowStateEvent) => void): () => boolean
  on(name: 'output', handler: (output: WindowOutput) => void): () => boolean
  on(name: 'hostcontrols', handler: (hostControls: boolean) => void): () => boolean
  on(name: 'minimizable', handler: (minimizable: boolean) => void): () => boolean
  on(name: 'shortcut', handler: (event: ShortcutEvent) => void): () => boolean
}

export interface NetworkDevice {
  readonly iface: string
  readonly type: 'wifi' | 'ethernet'
  readonly state: 'connected' | 'connecting' | 'disconnecting' | 'disconnected' | 'unavailable' | 'unmanaged' | 'failed'
  readonly connection: string | null
  readonly ip4: string | null
  readonly ssid?: string
  readonly strength?: number
  readonly carrier?: boolean
  readonly speed?: number
}
export interface NetworkState {
  readonly wifiEnabled: boolean
  readonly connectivity: 'full' | 'limited' | 'portal' | 'none' | 'unknown'
  readonly primary: 'wifi' | 'ethernet' | string | null
  readonly devices: NetworkDevice[]
}
export interface WifiNetwork {
  readonly ssid: string
  readonly strength: number
  readonly secure: boolean
  readonly band: '2.4GHz' | '5GHz'
  readonly active: boolean
  readonly saved: boolean
}
export interface AcdeNetwork {
  state(): Promise<NetworkState>
  networks(scan?: boolean): Promise<WifiNetwork[]>
  connect(ssid: string, password?: string): Promise<void>
  activate(iface: string): Promise<void>
  disconnect(iface: string): Promise<void>
  setWifiEnabled(on: boolean): Promise<void>
  watch(callback: (state: NetworkState) => void): void
  on(type: 'changed', handler: (state: NetworkState) => void): () => boolean
}

export interface PanelReserve {
  readonly edge: WindowEdge
  readonly thickness?: number
}

export type PanelKeyboard = 'exclusive' | 'on-demand'

export interface PinAnchor {
  readonly window: number
  readonly x: number
  readonly y: number
  readonly width?: number
  readonly height?: number
  readonly edge?: WindowEdge
  readonly align?: 'start' | 'center' | 'end'
  readonly adjust?: readonly ('flip' | 'slide')[]
}

export type PinClosed = 'parent-closed' | 'refused' | 'unpinned' | 'dismissed'

export interface PinOptions {
  readonly margin?: number
  readonly reserve?: PanelReserve
  readonly keyboard?: PanelKeyboard
  readonly anchor?: PinAnchor
  readonly userOnly?: boolean
  readonly popup?: boolean
  readonly parent?: PinnedPanel
}

export interface PinnedPanel {
  update(): void
  unpin(): void
  reserve(reserve: PanelReserve | null): void
  anchor?(next: PinAnchor): void
  readonly closed?: Promise<PinClosed>
  readonly popup: boolean
}

export type DoubleClickAction = 'fill' | 'minimize' | 'none'

export interface FrameColors {
  readonly background?: string
  readonly title?: string
  readonly line?: string
  readonly buttons?: string
}

export interface FrameTheme {
  readonly active?: FrameColors
  readonly inactive?: FrameColors
  readonly light?: { readonly active?: FrameColors; readonly inactive?: FrameColors }
  readonly buttonStyle?: 'dots' | 'glyphs'
  readonly buttonSide?: 'left' | 'right'
  readonly titleAlign?: 'center' | 'start'
}

export interface AcdeDesktop {
  apps(): Promise<{ apps: DesktopApp[] }>
  pin?(element: Element, options?: PinOptions): PinnedPanel
  setDoubleClick?(action: DoubleClickAction): void
  setFrameTheme?(theme: FrameTheme | null): void
  requestGrab?(kind: 'pointer' | 'keyboard'): void
  releaseGrab?(kind: 'pointer' | 'keyboard'): void
  on<K extends keyof DesktopEventMap>(type: K, handler: (event: DesktopEventMap[K]) => void): () => boolean
}

export interface ShellModuleTypes {
  readonly windows: AcdeWindows
  readonly packages: AcdePackages
  readonly session: AcdeSession
  readonly desktops: AcdeDesktops
  readonly inspect: AcdeInspect
  readonly desktop: AcdeDesktop
  readonly setDefault: AcdeOpenFiles['setDefault']
  readonly permissions: AcdePermissions
  readonly software: AcdeSoftware
}

export interface ServiceModuleTypes {
  readonly windows: Required<AcdeWindows>
  readonly packages: AcdePackages
  readonly session: AcdeSession
  readonly desktops: AcdeDesktops
  readonly inspect: AcdeInspect
  readonly desktop: Pick<AcdeDesktop, 'apps'>
  readonly setDefault: AcdeOpenFiles['setDefault']
  readonly voice: AcdeVoice
  readonly software: Omit<AcdeSoftware, 'on'>
}

export type VoiceEvent =
  | { readonly type: 'partial'; readonly text: string }
  | { readonly type: 'final'; readonly text: string }
  | { readonly type: 'error'; readonly message: string }

export interface AcdeVoice {
  listen(onEvent: (event: VoiceEvent) => void): number
  stop(id: number): boolean
}

export interface SystemInfo {
  readonly hostname: string
  readonly os: string
  readonly kernel: string
  readonly cpu: string
  readonly cores: number
  readonly memory: number
  readonly gpu: readonly string[]
  readonly npu: string | null
  readonly uptimeSeconds: number
  readonly serial: string | null
}

export interface SystemVolume {
  readonly path: string
  readonly label: string
  readonly size: number
  readonly free: number
}

export interface SystemTime {
  readonly timezone: string
  readonly ntp: boolean
  readonly canSetTime: boolean
}

export interface SystemUser {
  readonly name: string
  readonly fullName: string
  readonly avatar: string | null
}

export interface AcdeSystem {
  info(): Promise<SystemInfo>
  storage(): Promise<SystemVolume[]>
  time(): Promise<SystemTime>
  timezones(): Promise<string[]>
  setTimezone(zone: string): Promise<void>
  setNtp(on: boolean): Promise<void>
  setHostname(name: string): Promise<void>
  user(): Promise<SystemUser>
  on(type: 'changed', handler: (change: SystemChange) => void): () => boolean
}

export interface SystemChange {
  readonly storage: SystemVolume[]
  readonly hostname: string
  readonly timezone: string
  readonly ntp: boolean
}

export type AcdeErrorCode = 'bad_request' | 'forbidden' | 'unsupported' | 'service_not_running' | 'failed'

export interface AcdeError extends Error {
  readonly name: 'AcdeError'
  readonly status: number
  readonly code: AcdeErrorCode
  readonly module: string
  readonly reason?: string
}

export interface AcdePage {
  readonly fs?: AcdeFs
  readonly office?: AcdeOffice
  readonly terminal?: AcdeTerminals
  readonly network?: AcdeNetwork
  readonly system?: AcdeSystem
  readonly bluetooth?: AcdeBluetooth
  readonly audio?: AcdeAudio
  readonly devices?: AcdeDevices
  readonly power?: AcdePower
  readonly input?: AcdeInput
  readonly display?: AcdeDisplay
  readonly windows?: AcdeWindows
  readonly apps?: AcdeApps
  readonly launch?: AcdeLaunch
  readonly packages?: AcdePackages
  readonly desktop?: AcdeDesktop
  readonly desktops?: AcdeDesktops
  readonly session?: AcdeSession
  readonly lock?: AcdeLock
  readonly icons?: AcdeIcons
  readonly inspect?: AcdeInspect
  readonly open?: AcdeOpen
  readonly openPath?: AcdeOpenFiles['openPath']
  readonly handlers?: AcdeOpenFiles['handlers']
  readonly setDefault?: AcdeOpenFiles['setDefault']
  readonly permissions?: AcdePermissions
  readonly software?: AcdeSoftware
  readonly window?: AcdeWindow
  readonly services?: AcdeServices
  readonly capabilities?: import('./capabilities').AcdeCapabilities
  readonly [service: string]: unknown
}
