import type { TerminalEvent } from '../events'

export interface AcdeTerminals {
  open(cols: number, rows: number, options?: TerminalOpenOptions): Promise<{ id: string }>
  subscribe(id: string, callback: (events: TerminalEvent[]) => void): Promise<void>
  input(id: string, data: string): Promise<{ ok: true }>
  resize(id: string, cols: number, rows: number): Promise<{ cols: number; rows: number }>
  close(id: string): Promise<{ closed: string }>
  shells(): Promise<TerminalShell[]>
  limits(): Promise<TerminalLimits>
}

export interface TerminalOpenOptions {
  readonly command?: readonly string[]
  readonly cwd?: string
  readonly env?: Readonly<Record<string, string>>
}

export interface TerminalShell {
  readonly path: string
  readonly name: string
  readonly login: boolean
}

export interface TerminalLimits {
  readonly max: number
  readonly open: number
}
