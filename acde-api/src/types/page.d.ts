import type { AppsChangedEvent, DesktopEventMap, ShortcutEvent, TerminalEvent, WindowGrabEvent, WindowGrabLostEvent, WindowsEvents } from './events'
import type { AcdeOffice } from './office'
import type { AcdePermissions } from './permissions'
import type { SharedPlace } from './places'

export type * from './events'
export type * from './office'
export type * from './permissions'

export interface FinderRoot {
  id: string
  path?: string
  label: string
  source: 'desktop' | 'downloads' | 'default' | 'granted' | 'private' | 'place'
  place?: SharedPlace
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
}

export interface NativeWindow {
  readonly id: number
  readonly appId: string | null
  readonly title: string | null
  readonly web?: boolean
  readonly state: WindowState
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
}

export interface AcdeTerminals {
  open(cols: number, rows: number): Promise<{ id: string }>
  subscribe(id: string, callback: (events: TerminalEvent[]) => void): Promise<void>
  input(id: string, data: string): Promise<{ ok: true }>
  resize(id: string, cols: number, rows: number): Promise<{ cols: number; rows: number }>
  close(id: string): Promise<{ closed: string }>
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
  ): Promise<{ kind: 'acde' | 'native'; id: string; window?: number }>
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

export interface OpenWindowOptions {
  readonly parent?: number
  readonly modal?: boolean
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
}

export interface AcdeOpen {
  (request: SystemRequest): Promise<Opened>
  (app: string, request: SystemRequest): Promise<Opened>
  <R extends AppRequest>(app: string, request: R, options?: OpenWindowOptions): Promise<{ window: number }>
  (app: string, request?: null, options?: OpenWindowOptions): Promise<{ window: number }>
}

export interface AcdeAppList {
  on?(type: 'changed', handler: (event: AppsChangedEvent) => void): () => boolean
  list(): Promise<DesktopApp[]>
  open: AcdeOpen
}

export interface AcdeApps extends AcdeAppList, AcdeOpenFiles {
  refresh(): Promise<DesktopApp[]>
  install(dir: string): Promise<{ id: string; dir: string; apps: DesktopApp[] }>
  natives(): Promise<NativeApp[]>
  runNative(id: string): Promise<number>
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

export interface AcdeSession {
  own(): boolean
  logout(): Promise<void>
  reboot(): Promise<void>
  poweroff(): Promise<void>
  setLanguage(tag: string): Promise<void>
}

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
    effect?: { radius?: number; blur?: number; saturate?: number; brightness?: number },
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
}

export interface PanelReserve {
  readonly edge: WindowEdge
  readonly thickness?: number
}

export interface PinOptions {
  readonly margin?: number
  readonly reserve?: PanelReserve
}

export interface PinnedPanel {
  update(): void
  unpin(): void
  reserve(reserve: PanelReserve | null): void
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
}

export interface ServiceModuleTypes {
  readonly windows: Required<AcdeWindows>
  readonly packages: AcdePackages
  readonly session: AcdeSession
  readonly desktops: AcdeDesktops
  readonly inspect: AcdeInspect
  readonly desktop: Pick<AcdeDesktop, 'apps'>
  readonly setDefault: AcdeOpenFiles['setDefault']
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
}

export interface BluetoothDevice {
  readonly id: string
  readonly address: string
  readonly name: string
  readonly named: boolean
  readonly kind: 'audio' | 'headset' | 'keyboard' | 'mouse' | 'phone' | 'computer' | 'gamepad' | 'other'
  readonly icon: string
  readonly paired: boolean
  readonly connected: boolean
  readonly trusted: boolean
  readonly blocked: boolean
  readonly battery: number | null
  readonly rssi: number | null
}

export interface BluetoothState {
  readonly available: boolean
  readonly powered: boolean
  readonly discoverable: boolean
  readonly discoverableTimeout: number
  readonly pairable: boolean
  readonly discovering: boolean
  readonly name: string
  readonly address: string
  readonly blocked: 'soft' | 'hard' | null
  readonly onLogin: 'remember' | 'on' | 'off'
  readonly devices: readonly BluetoothDevice[]
}

export interface BluetoothPairing {
  readonly device: string
  readonly name: string
  readonly kind: 'confirm' | 'display' | 'pin' | 'passkey' | 'authorize' | 'service' | 'cancel'
  readonly passkey?: string
  readonly entered?: number
  readonly uuid?: string
}

export interface AcdeBluetooth {
  state(): Promise<BluetoothState>
  setPowered(on: boolean): Promise<void>
  setDiscoverable(on: boolean, timeout?: number): Promise<void>
  setName(name: string): Promise<void>
  scan(on: boolean): Promise<void>
  pair(id: string): Promise<void>
  answer(id: string, accept: boolean, pin?: string): Promise<void>
  connect(id: string): Promise<void>
  disconnect(id: string): Promise<void>
  forget(id: string): Promise<void>
  setTrusted(id: string, on: boolean): Promise<void>
  setBlocked(id: string, on: boolean): Promise<void>
  rename(id: string, name: string): Promise<void>
  setOnLogin(mode: 'remember' | 'on' | 'off'): Promise<void>
  on(type: 'changed', handler: (state: BluetoothState) => void): () => boolean
  on(type: 'pairing', handler: (request: BluetoothPairing) => void): () => boolean
}

export interface AudioPort {
  readonly id: string
  readonly name: string
  readonly available: boolean | null
}

export interface AudioDevice {
  readonly id: string
  readonly name: string
  readonly port: 'speaker' | 'headphones' | 'hdmi' | 'bluetooth' | 'usb' | 'mic' | 'other'
  readonly isDefault: boolean
  readonly volume: number
  readonly muted: boolean
  readonly balance: number | null
  readonly ports: readonly AudioPort[]
  readonly activePort: string | null
  readonly card: string | null
  readonly virtual: boolean
}

export interface AudioStream {
  readonly id: string
  readonly kind: 'playback' | 'capture'
  readonly app: string
  readonly title: string
  readonly icon: string
  readonly volume: number
  readonly muted: boolean
  readonly balance: number | null
  readonly device: string | null
}

export interface AudioCard {
  readonly id: string
  readonly name: string
  readonly icon: string
  readonly bus: string
  readonly profile: string | null
  readonly profiles: readonly { readonly id: string; readonly name: string; readonly available: boolean | null }[]
}

export interface AudioState {
  readonly outputs: readonly AudioDevice[]
  readonly inputs: readonly AudioDevice[]
  readonly streams: readonly AudioStream[]
  readonly cards: readonly AudioCard[]
  readonly maxVolume: number
}

export interface AcdeAudio {
  state(): Promise<AudioState>
  setDefault(id: string): Promise<void>
  setVolume(id: string, volume: number): Promise<void>
  setMuted(id: string, muted: boolean): Promise<void>
  setBalance(id: string, balance: number): Promise<void>
  setPort(id: string, port: string): Promise<void>
  setProfile(card: string, profile: string): Promise<void>
  moveStream(id: string, device: string | null): Promise<void>
  on(type: 'changed', handler: (state: AudioState) => void): () => boolean
}

export interface Drive {
  readonly id: string
  readonly device: string
  readonly label: string
  readonly kind: 'usb' | 'card' | 'disc' | 'phone' | 'disk'
  readonly size: number
  readonly free: number | null
  readonly mounted: boolean
  readonly path: string | null
  readonly ejectable: boolean
}

export interface Printer {
  readonly id: string
  readonly name: string
  readonly state: 'idle' | 'printing' | 'stopped' | 'offline'
  readonly isDefault: boolean
  readonly jobs: number
  readonly model: string
  readonly location: string
  readonly message: string
  readonly reasons: readonly string[]
  readonly accepting: boolean
  readonly shared: boolean
}

export interface PrintJob {
  readonly id: number
  readonly name: string
  readonly user: string
  readonly state: 'pending' | 'held' | 'processing' | 'stopped' | 'canceled' | 'aborted' | 'completed'
  readonly size: number
  readonly created: number
  readonly message: string
}

export interface AutomountDevice {
  readonly id: string
  readonly name: string
  readonly attached: boolean
  readonly login: boolean
  readonly attach: boolean
  readonly everMounted: boolean
}

export interface Automount {
  readonly onLogin: boolean
  readonly onAttach: boolean
  readonly unknown: boolean
  readonly devices: readonly AutomountDevice[]
}

export interface DevicesState {
  readonly drives: readonly Drive[]
  readonly printers: readonly Printer[]
  readonly printing: boolean
  readonly automount: Automount
}

export interface AcdeDevices {
  state(): Promise<DevicesState>
  mount(id: string): Promise<string>
  unmount(id: string): Promise<void>
  eject(id: string): Promise<void>
  setAutomount(patch: Partial<Pick<Automount, 'onLogin' | 'onAttach' | 'unknown'>>): Promise<void>
  setDeviceAutomount(id: string, patch: Partial<Pick<AutomountDevice, 'login' | 'attach'>>): Promise<void>
  forgetDevice(id: string): Promise<void>
  setDefaultPrinter(id: string): Promise<void>
  pausePrinter(id: string): Promise<void>
  resumePrinter(id: string): Promise<void>
  jobs(printer: string): Promise<PrintJob[]>
  cancelJob(printer: string, job: number): Promise<void>
  on(type: 'changed', handler: (state: DevicesState) => void): () => boolean
  on(type: 'added', handler: (drive: Drive) => void): () => boolean
}

export type PowerProfile = 'power-saver' | 'balanced' | 'performance'

export type PowerAction = 'none' | 'sleep' | 'hibernate' | 'shutdown' | 'prompt' | 'lock' | 'screen-off'

export type PowerIdleAction = 'none' | 'sleep' | 'hibernate' | 'shutdown'

export interface ChargeLimit {
  readonly start: number | null
  readonly end: number | null
  readonly writable: boolean
  readonly upower: boolean
}

export interface PowerBattery {
  readonly id: string
  readonly name: string
  readonly percent: number
  readonly state: 'charging' | 'discharging' | 'full' | 'empty' | 'unknown'
  readonly secondsLeft: number | null
  readonly health: number | null
  readonly rate: number | null
  readonly chargeLimit: ChargeLimit | null
}

export interface PowerPolicy {
  readonly dim: number | null
  readonly screenOff: number | null
  readonly idleAction: PowerIdleAction
  readonly idleAfter: number
  readonly lidAction: PowerAction
  readonly lidExternal: boolean
  readonly powerButton: PowerAction
  readonly sleepMode: 'suspend' | 'hybrid' | 'suspend-then-hibernate'
  readonly profile: PowerProfile | null
  readonly brightness: number | null
}

export type PowerGroup = 'ac' | 'battery' | 'lowBattery'

export interface PowerSettings {
  readonly ac: PowerPolicy
  readonly battery: PowerPolicy
  readonly lowBattery: PowerPolicy
  readonly lowLevel: number
  readonly criticalLevel: number
  readonly criticalAction: PowerIdleAction
}

export type PowerSettingsPatch = { readonly [G in PowerGroup]?: Partial<PowerPolicy> } &
  Partial<Pick<PowerSettings, 'lowLevel' | 'criticalLevel' | 'criticalAction'>>

export interface PowerState {
  readonly onBattery: boolean
  readonly battery: PowerBattery | null
  readonly batteries: readonly PowerBattery[]
  readonly lid: boolean
  readonly profile: PowerProfile | null
  readonly profiles: readonly PowerProfile[]
  readonly degraded: string | null
  readonly screenOffSeconds: number | null
  readonly sleepSeconds: number | null
  readonly current: PowerGroup
  readonly settings: PowerSettings
  readonly can: { readonly suspend: boolean; readonly hibernate: boolean; readonly hybrid: boolean; readonly suspendThenHibernate: boolean }
  readonly brightness: number | null
  readonly inhibitors: readonly { readonly who: string; readonly why: string; readonly what: readonly ('sleep' | 'idle')[] }[]
  readonly policy: boolean
}

export interface InputLayout {
  readonly layout: string
  readonly variant: string
}

export interface InputKeyboard {
  readonly layouts: readonly InputLayout[]
  readonly model: string
  readonly options: readonly string[]
  readonly repeat: boolean
  readonly repeatDelay: number
  readonly repeatRate: number
  readonly numlock: 'on' | 'off' | 'unchanged'
  readonly active: readonly string[]
  readonly current: number
}

export type InputScroll = 'two-finger' | 'edge' | 'button' | 'none'
export type InputClick = 'areas' | 'fingers'

export interface InputPointer {
  readonly present: boolean
  readonly speed?: number
  readonly acceleration?: 'adaptive' | 'flat'
  readonly leftHanded?: boolean
  readonly naturalScroll?: boolean
  readonly scrollSpeed?: number
  readonly middleEmulation?: boolean
}

export interface InputTouchpad extends InputPointer {
  readonly tapToClick?: boolean
  readonly tapAndDrag?: boolean
  readonly dragLock?: boolean
  readonly tapButtons?: 'lrm' | 'lmr'
  readonly disableWhileTyping?: boolean
  readonly scroll?: InputScroll
  readonly click?: InputClick
  readonly scrollMethods: readonly InputScroll[]
  readonly clickMethods: readonly InputClick[]
}

export interface InputState {
  readonly keyboard: InputKeyboard
  readonly mouse: InputPointer
  readonly touchpad: InputTouchpad
  readonly devices: readonly { readonly name: string; readonly kind: 'mouse' | 'touchpad' }[]
}

type InputNullable<T> = { readonly [K in keyof T]?: T[K] | null }

export interface InputPatch {
  readonly keyboard?: InputNullable<Omit<InputKeyboard, 'active' | 'current'>>
  readonly mouse?: InputNullable<Omit<InputPointer, 'present'>>
  readonly touchpad?: InputNullable<Omit<InputTouchpad, 'present' | 'scrollMethods' | 'clickMethods'>>
}

export interface InputCatalog {
  readonly models: readonly { readonly id: string; readonly name: string; readonly vendor: string }[]
  readonly layouts: readonly {
    readonly id: string
    readonly name: string
    readonly short: string
    readonly languages: readonly string[]
    readonly variants: readonly { readonly id: string; readonly name: string }[]
  }[]
  readonly options: readonly {
    readonly id: string
    readonly name: string
    readonly multiple: boolean
    readonly options: readonly { readonly id: string; readonly name: string }[]
  }[]
}

export interface AcdeInput {
  state(): Promise<InputState>
  set(patch: InputPatch): Promise<void>
  catalog(): Promise<InputCatalog>
  on(type: 'changed', handler: (state: InputState) => void): () => boolean
}

export interface DisplayMode {
  readonly width: number
  readonly height: number
  readonly refresh: number
  readonly preferred?: boolean
}

export interface DisplayInfo {
  readonly id: string
  readonly name: string
  readonly builtin: boolean
  readonly primary: boolean
  readonly enabled: boolean
  readonly mode: DisplayMode
  readonly modes: readonly DisplayMode[]
  readonly scale: number
  readonly scales: readonly number[]
  readonly brightness: number | null
  readonly rotation: 0 | 90 | 180 | 270
}

export interface NightLight {
  readonly on: boolean
  readonly warmth: number
  readonly schedule: 'manual' | 'sunset'
}

export interface DisplayState {
  readonly displays: readonly DisplayInfo[]
  readonly nightLight: NightLight | null
}

export interface DisplayPatch {
  readonly scale?: number
}

export interface AcdeDisplay {
  state(): Promise<DisplayState>
  configure(id: string, patch: DisplayPatch): Promise<void>
  setBrightness(id: string, value: number): Promise<void>
  setNightLight(patch: Partial<NightLight>): Promise<void>
  on(type: 'changed', handler: (state: DisplayState) => void): () => boolean
}

export interface AcdePower {
  state(): Promise<PowerState>
  setProfile(profile: PowerProfile): Promise<void>
  configure(patch: PowerSettingsPatch): Promise<void>
  setScreenOff(seconds: number | null): Promise<void>
  setSleep(seconds: number | null): Promise<void>
  suspend(): Promise<void>
  hibernate(): Promise<void>
  setBrightness(value: number): Promise<void>
  setChargeLimit(percent: number | null): Promise<void>
  on(type: 'changed', handler: (state: PowerState) => void): () => boolean
  on(type: 'prompt', handler: (event: { readonly type: 'prompt' }) => void): () => boolean
  on(type: 'critical', handler: (event: { readonly type: 'critical'; readonly action: PowerIdleAction; readonly seconds: number }) => void): () => boolean
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
  readonly inspect?: AcdeInspect
  readonly open?: AcdeOpen
  readonly openPath?: AcdeOpenFiles['openPath']
  readonly handlers?: AcdeOpenFiles['handlers']
  readonly setDefault?: AcdeOpenFiles['setDefault']
  readonly permissions?: AcdePermissions
  readonly window?: AcdeWindow
  readonly capabilities?: import('./capabilities').AcdeCapabilities
  readonly [service: string]: unknown
}
