import type { WindowRect } from '../page'

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
  readonly lockOnScreenOff: boolean
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

export interface InputMethodItem {
  readonly id: string
  readonly name: string
  readonly short: string
}

export interface InputMethodState {
  readonly running: boolean
  readonly current: string | null
  readonly group: readonly InputMethodItem[]
}

export interface InputState {
  readonly keyboard: InputKeyboard
  readonly mouse: InputPointer
  readonly touchpad: InputTouchpad
  readonly devices: readonly { readonly name: string; readonly kind: 'mouse' | 'touchpad' }[]
  readonly ime?: InputMethodState
}

type InputNullable<T> = { readonly [K in keyof T]?: T[K] | null }

export interface InputPatch {
  readonly keyboard?: InputNullable<Omit<InputKeyboard, 'active' | 'current'>>
  readonly mouse?: InputNullable<Omit<InputPointer, 'present'>>
  readonly touchpad?: InputNullable<Omit<InputTouchpad, 'present' | 'scrollMethods' | 'clickMethods'>>
  readonly ime?: { readonly current?: string; readonly group?: readonly string[] }
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
  readonly imes?: readonly (InputMethodItem & { readonly languages: readonly string[] })[]
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
  readonly bounds?: WindowRect | null
  readonly workArea?: WindowRect | null
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
  on(type: 'prompt', handler: (event: PowerPromptEvent) => void): () => boolean
  on(type: 'critical', handler: (event: PowerCriticalEvent) => void): () => boolean
}

export interface PowerPromptEvent {
  readonly type: 'prompt'
}

export interface PowerCriticalEvent {
  readonly type: 'critical'
  readonly action: PowerIdleAction
  readonly seconds: number
}
