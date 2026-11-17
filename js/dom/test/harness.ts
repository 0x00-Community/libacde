import { buildSync } from 'esbuild'
import { fileURLToPath } from 'node:url'
import vm from 'node:vm'
import type { InstallOptions, WebInternals } from '../src/index.ts'
import { MockDom } from './mock/dom.ts'
import { MockHost } from './mock/host.ts'

const root = fileURLToPath(new URL('..', import.meta.url))
const bundles = new Map<string, string>()

export function bundle(entry: string, name: string, define: Record<string, string> = {}): string {
  const key = `${entry}:${name}`
  let code = bundles.get(key)
  if (!code) {
    const result = buildSync({ absWorkingDir: root, entryPoints: [entry], bundle: true, write: false, format: 'iife', globalName: name, platform: 'browser', target: 'es2023', define, keepNames: true, logLevel: 'error' })
    code = result.outputFiles[0]!.text
    bundles.set(key, code)
  }
  return code
}

const domBundle = bundle('src/index.ts', '__acdeDom')

export interface Web {
  readonly context: vm.Context
  readonly internals: WebInternals
  readonly dom: MockDom
  readonly host: MockHost
  run<T = unknown>(code: string): T
  json<T = unknown>(code: string): T
  frame(time?: number): void
  endTask(): void
  wait(ms?: number): Promise<void>
  close(): void
  [Symbol.dispose](): void
}

export function createWeb(options: Partial<InstallOptions> = {}): Web {
  const dom = new MockDom()
  const host = new MockHost()
  const context = vm.createContext({})
  host.context = context
  vm.runInContext(domBundle, context, { filename: 'acde-dom.js' })
  context['__args'] = [dom, host, { url: 'http://acde.local/', logNetwork: false, ...options }]
  const internals = vm.runInContext('__acdeDom.install(globalThis, ...globalThis.__args)', context) as WebInternals
  vm.runInContext('delete globalThis.__args', context)
  host.afterTask = () => internals.endTask()
  let time = 0
  return {
    context,
    internals,
    dom,
    host,
    run: <T>(code: string) => vm.runInContext(code, context) as T,
    json: <T>(code: string) => JSON.parse(vm.runInContext(`JSON.stringify(${code})`, context) as string) as T,
    frame: (t?: number) => internals.frame(t ?? (time += 16)),
    endTask: () => internals.endTask(),
    wait: (ms = 0) => new Promise((resolve) => setTimeout(() => setImmediate(resolve), ms)),
    close: () => host.dispose(),
    [Symbol.dispose]: () => host.dispose(),
  }
}

export async function page(html = '<!doctype html><html><body></body></html>', options: Partial<InstallOptions> = {}): Promise<Web> {
  const web = createWeb(options)
  await web.internals.loadPage(html)
  return web
}

export function callsOf(web: Web, code: string): Map<string, number> {
  const before = new Map(web.dom.calls)
  web.run(code)
  const out = new Map<string, number>()
  for (const [name, n] of web.dom.calls) if (n !== (before.get(name) ?? 0)) out.set(name, n - (before.get(name) ?? 0))
  return out
}

export function loadReact(web: Web): void {
  vm.runInContext(bundle('test/fixtures/react.ts', '__react', { 'process.env.NODE_ENV': '"development"' }), web.context, { filename: 'react.js' })
}
