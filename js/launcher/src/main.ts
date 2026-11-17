import { installCore } from '@dopame/acde-dom/core'
import type { WebGrants } from '../../page/src/sys/natives'
import { bindNatives, sys, type Natives } from '../../page/src/sys/natives'
import { readDirOr, readTextOr } from '../../page/src/sys/fs'
import { xdgHome } from '../../page/src/sys/os'
import { join } from '../../page/src/sys/path'
import { process } from '../../page/src/sys/process'
import { Rpc } from '../../page/src/rpc'
import { SessionLanguage } from '../../page/src/language'
import { AppLookup, webInit, type Found } from './app'
import { LaunchArgs } from './args'

interface Grant {
  readonly window: number
  readonly token: bigint
}

interface LauncherShell {
  readonly socketName?: string
  pump(): { readonly type: string; readonly window?: number }[]
  onEvents(callback: () => void): void
  grant(options: { window: number; mode: 'fullscreen' | 'free'; size?: readonly number[]; host?: boolean }): Grant
  screenshot(path: string): Promise<{ width: number; height: number }>
}

interface LauncherNatives extends Natives {
  readonly native: { readonly Shell: { broker(options?: { title?: string }): LauncherShell } }
}

const HEAP_MB = 256
const STOP = new Set(['close', 'exit', 'quit', 'windowcloserequest'])
const ACKNOWLEDGED = new Set(['window.ready', 'window.setTitle', 'page.shown'])

class Launcher {
  readonly #shell: LauncherShell
  #wake: (() => void) | null = null
  #ended: number | null = null

  constructor(natives: LauncherNatives, readonly found: Found, readonly args: LaunchArgs) {
    this.#shell = natives.native.Shell.broker({ title: found.app.label })
    this.#shell.onEvents(() => this.#wake?.())
  }

  start(): void {
    const { app } = this.found
    const grant = this.#shell.grant({ window: 1, mode: 'fullscreen', host: true, ...(app.size && { size: app.size }) })
    const init = webInit(this.found, grant.token, {
      width: this.args.width,
      height: this.args.height,
      scale: this.args.scale,
      languages: SessionLanguage.of(process.env, sys().languages()),
      navigator: { userAgent: '', platform: process.platform === 'darwin' ? 'MacIntel' : process.platform === 'win32' ? 'Win32' : 'Linux x86_64' },
    })
    const rpc = new Rpc((message) => thread.post(message))
    rpc.serve((service, method) => {
      if (ACKNOWLEDGED.has(`${service}.${method}`)) return undefined
      throw Object.assign(new Error(`acdev: ${service}.${method} is not supported by the single-app launcher`), { status: 501, code: 'unsupported' })
    })
    const env = { ...Launcher.env(), ...(this.#shell.socketName && { WAYLAND_DISPLAY: this.#shell.socketName }) }
    const thread = sys().spawnWeb(init.name, init, app.heapMb ?? HEAP_MB, env, (message) => void rpc.receive(message), (code, reason) => {
      if (reason) process.stderr.write(`acdev: ${init.name} stopped: ${reason}\n`)
      this.#end(code)
    }, this.#grants())
  }

  #grants(): WebGrants {
    const { app } = this.found
    const root = process.env['ACDE_STORAGE'] ?? join(xdgHome('data'), 'acde', 'storage')
    return {
      app: app.id, fs: null, terminal: false, network: !!app.capabilities.network, system: false, bluetooth: false, audio: !!app.capabilities.audio,
      devices: false, power: false, input: false, display: false, office: false, camera: false, microphone: false,
      storage: { key: `app-${app.id}`, dir: root ? join(root, `app-${app.id}`) : null, legacy: null },
    }
  }

  static env(): Record<string, string> {
    const keep = /^(ACDE_|XDG_|LANG$|LC_|LANGUAGE$|HOME$|USER$|DBUS_SESSION_BUS_ADDRESS$|RUST_LOG$|TZ$|PATH$|RUST_BACKTRACE$)/
    return Object.fromEntries(Object.entries(process.env).filter((entry): entry is [string, string] => keep.test(entry[0]) && entry[1] !== undefined))
  }

  #end(code: number): void {
    this.#ended ??= code
    this.#wake?.()
  }

  async run(): Promise<number> {
    if (this.args.shot) this.#shoot(this.args.shot).catch((error: Error) => process.stderr.write(`acdev: screenshot: ${error.message}\n`))
    if (this.args.seconds !== null) setTimeout(() => this.#end(0), this.args.seconds * 1000)
    while (this.#ended === null) {
      for (const event of this.#shell.pump()) if (STOP.has(event.type)) this.#end(0)
      if (this.#ended !== null) break
      await new Promise<void>((wake) => {
        this.#wake = wake
      })
    }
    return this.#ended
  }

  async #shoot(file: string): Promise<void> {
    await new Promise((later) => setTimeout(later, this.args.shotAfterMs))
    const { width, height } = await this.#shell.screenshot(file)
    process.stderr.write(`acdev: screenshot ${width}x${height} -> ${file}\n`)
  }
}

export default async function launch(natives: LauncherNatives): Promise<void> {
  installCore(globalThis, natives.host, { nodeTimers: true })
  bindNatives(natives)
  try {
    const args = LaunchArgs.parse(process.argv)
    const path = (process.env['ACDE_PATH'] ?? '').split(':').filter(Boolean)
    const found = new AppLookup({ readText: (file) => readTextOr(file, null), readDir: readDirOr }).find(args.app, path, sys().cwd())
    const launcher = new Launcher(natives, found, args)
    launcher.start()
    process.exit(await launcher.run())
  } catch (error) {
    process.stderr.write(`acdev: ${(error as Error).message}\n`)
    process.exit(2)
  }
}
