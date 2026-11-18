export interface ServiceContext {
  readonly name: string
  readonly package: string
  readonly state: string
  readonly native: Readonly<Record<string, unknown>> | null
  readonly env: Readonly<Record<string, string | undefined>>
  readonly inspect?: import('./page').AcdeInspect
  log(line: string): void
  spawn(argv: readonly string[], options?: SpawnOptions): ServiceProcess
  import<K extends keyof import('./page').ServiceModuleTypes>(
    specifier: `acde:${K}`,
  ): Promise<{ readonly default: import('./page').ServiceModuleTypes[K] } & { readonly [N in K]: import('./page').ServiceModuleTypes[K] }>
}

export interface SpawnOptions {
  readonly cwd?: string
  readonly env?: Readonly<Record<string, string>>
  onLine?(line: string): void
  onExit?(code: number | null, signal: number | null): void
}

export interface ServiceProcess {
  readonly pid: number
  write(text: string): boolean
  end(): void
  kill(signal?: 'SIGTERM' | 'SIGKILL' | 'SIGINT' | 'SIGHUP'): boolean
}

export interface ServiceInstance {
  readonly api: Readonly<Record<string, object>>
  readonly sync?: readonly string[]
  close?(): unknown
}

export type ServiceFactory = (context: ServiceContext) => ServiceInstance | Promise<ServiceInstance>
