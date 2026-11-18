import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { ServiceShape, WebInit } from '../src/contract.ts'
import { RemoteBridge, WindowList } from '../src/remote.ts'
import { Rpc } from '../src/rpc.ts'
import { bindNatives } from '../src/sys/natives.ts'

type Services = { names(): string[]; on(type: string, handler: (event: unknown) => void): () => boolean }

const shape = (methods: string[]): ServiceShape => ({ methods, sync: {}, values: {}, callable: false })

const started = (services: Record<string, ServiceShape>) => {
  const rpc = new Rpc(() => {})
  const init = { kind: 'desktop', name: 'dopame', services } as unknown as WebInit
  const bridge = new RemoteBridge(rpc, init, null, new WindowList(rpc))
  const defined: string[] = []
  const refused: string[] = []
  bridge.define({ defineModule: (url) => void defined.push(url), refuseModule: (url) => void refused.push(url) })
  return { bridge, defined, refused, services: bridge.value['services'] as Services }
}

test('a service that comes up after the page is defined as a module and announced', () => {
  const { bridge, defined, services } = started({})
  const seen: unknown[] = []
  services.on('changed', (event) => void seen.push(event))
  assert.deepEqual(services.names(), [])
  defined.length = 0
  bridge.replaceServices({ state: shape(['watch']), desks: shape(['add']) })
  assert.deepEqual(services.names(), ['state', 'desks'])
  assert.deepEqual(defined, ['acde:state', 'acde:desks'], 'importable before the handlers run')
  assert.deepEqual(seen, [{ names: ['state', 'desks'], added: ['state', 'desks'], removed: [] }])
})

test('the same list again says nothing; a stopped service is refused', () => {
  const { bridge, refused, services } = started({ state: shape(['watch']) })
  const seen: unknown[] = []
  services.on('changed', (event) => void seen.push(event))
  bridge.replaceServices({ state: shape(['watch']) })
  assert.deepEqual(seen, [])
  bridge.replaceServices({})
  assert.deepEqual(refused, ['acde:state'])
  assert.equal(bridge.value['state'], undefined)
  assert.deepEqual(seen, [{ names: [], added: [], removed: ['state'] }])
})

test('a service cannot take the services namespace, and other events are refused', () => {
  const { bridge, services } = started({ services: shape(['x']) })
  assert.deepEqual(services.names(), [])
  assert.equal(typeof (bridge.value['services'] as Services).names, 'function')
  assert.throws(() => services.on('ready', () => {}), /acde:services has no ready event/)
})

type Session = { own(): boolean; logout(): Promise<unknown>; setLanguage(tag: string): Promise<unknown> }
type Desktops = { list(): Promise<unknown>; choose(id: string, remember?: boolean): Promise<unknown> }

const bridged = (init: Partial<WebInit>) => {
  bindNatives({ host: { logEnabled: () => false }, sys: { platform: 'linux', argv: [], pid: 1 } } as never)
  const sent: unknown[] = []
  const rpc = new Rpc((message) => void sent.push(message))
  const full = { name: 'page', services: {}, tier: { level: 1, shell: [], refused: {} }, ...init } as unknown as WebInit
  return { bridge: new RemoteBridge(rpc, full, null, new WindowList(rpc)), sent }
}

test('a desktop page gets acde:session and acde:desktops; own comes from the init, calls go out over rpc', () => {
  const { bridge, sent } = bridged({ kind: 'desktop', session: { own: true } })
  const session = bridge.value['session'] as Session
  const desktops = bridge.value['desktops'] as Desktops
  assert.equal(session.own(), true)
  void session.logout()
  void session.setLanguage('zh-CN')
  void desktops.choose('dopame', true)
  const calls = sent.map((m) => { const c = m as { service: string; method: string; args: unknown[] }; return [c.service, c.method, c.args] })
  assert.deepEqual(calls, [['session', 'logout', []], ['session', 'setLanguage', ['zh-CN']], ['desktops', 'choose', ['dopame', true]]])
})

test('without session in the init own() is false', () => {
  const { bridge } = bridged({ kind: 'desktop' })
  assert.equal((bridge.value['session'] as Session).own(), false)
})

test('an app page gets them only when its tier lists them', () => {
  assert.equal('session' in bridged({ kind: 'window' }).bridge.value, false)
  const shell = { level: 1, shell: ['session', 'desktops'], refused: {} } as unknown as WebInit['tier']
  const { bridge } = bridged({ kind: 'window', tier: shell })
  assert.equal('session' in bridge.value && 'desktops' in bridge.value, true)
})
