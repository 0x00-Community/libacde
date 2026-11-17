import type { HostNative } from '@dopame/acde-dom'
import type { StoragePlace } from '../contract'

export interface FileStat {
  readonly isFile: boolean
  readonly isDirectory: boolean
  readonly isSymbolicLink: boolean
  readonly size: number
  readonly mtimeMs: number
  readonly mode: number
}

export interface WebThreadHandle {
  post(message: unknown): void
  terminate(): void
  busyMs(): number
  cpuMs(): number
}

interface SysNative {
  readonly argv: readonly string[]
  readonly platform: string
  readonly pid: number
  env(): Record<string, string>
  proxyEnv(): Record<string, string>
  exit(code: number): never
  cwd(): string
  homedir(): string
  languages(): string[]
  getuid(): number
  readFile(path: string): Uint8Array
  readText(path: string): string
  writeFile(path: string, data: string | Uint8Array): void
  exists(path: string): boolean
  stat(path: string): FileStat | null
  readDir(path: string): string[]
  mkdir(path: string, recursive?: boolean): void
  rename(from: string, to: string): void
  remove(path: string, recursive?: boolean): void
  chmod(path: string, mode: number): void
  realpath(path: string): string
  stdout(text: string): void
  stderr(text: string): void
  kill(pid: number, signal?: string): boolean
  run(argv: readonly string[], timeoutMs?: number): { status: number | null; stdout: string; stderr: string }
  onSignal(callback: (name: 'SIGINT' | 'SIGTERM' | 'SIGHUP') => void): void
  spawn?(
    argv: readonly string[],
    options: { cwd?: string; env: Record<string, string>; prefix: string; stdin?: boolean; onStdout?: (line: string) => void },
    onExit: (code: number | null, signal: number | null) => void,
  ): number
  spawnBackend?(
    argv: readonly string[],
    options: { cwd?: string; env: Record<string, string>; prefix: string; app: string; mode: 'channel' | 'sockets'; preload: string },
    onExit: (code: number | null, signal: number | null) => void,
  ): number
  stopBackend?(pid: number, ms: number): boolean
  write?(pid: number, text: string): boolean
  endInput?(pid: number): void
  spawnWeb(
    name: string,
    init: unknown,
    heapMb: number,
    env: Record<string, string>,
    onMessage: (message: unknown) => void,
    onExit: (code: number, reason: string | null) => void,
    grants: WebGrants,
  ): WebThreadHandle
  openOffice?(): string | null
  restoreBluetooth?(): void
  startDevices?(): void
  closeOffice?(): void
  contentType?(extension: string): string | null
}

export interface WebGrants {
  readonly app: string | null
  readonly fs: true | string[] | { readonly places: string[]; readonly paths: boolean } | null
  readonly terminal: boolean
  readonly network: boolean
  readonly system: boolean
  readonly bluetooth: boolean
  readonly audio: boolean
  readonly devices: boolean
  readonly power: boolean
  readonly input: boolean
  readonly display: boolean
  readonly office: boolean
  readonly camera: boolean
  readonly microphone: boolean
  readonly storage: StoragePlace
}

export interface Natives {
  readonly host: HostNative
  readonly sys: SysNative
  readonly native?: Readonly<Record<string, unknown>>
}

let current: Natives | null = null

export function bindNatives(natives: Natives): void {
  current = natives
}

export function natives(): Natives {
  if (!current) throw new Error('acde-host: natives used before the entry bound them')
  return current
}

export const sys = (): SysNative => natives().sys
