import assert from 'node:assert/strict'
import { test } from 'node:test'
import { checkedModule, SystemWatch, systemError } from '../src/sysmodules.ts'
import { Watched } from '../src/watched.ts'

test('system module failures become AcdeError with a status and a code', async () => {
  const module = checkedModule('network', {
    denied: () => {
      throw new Error('acde.network: not given to this web')
    },
    missing: async () => {
      throw new Error('org.freedesktop.DBus.Error.ServiceUnknown: NetworkManager')
    },
    wrong: () => {
      throw new TypeError('acde.network.connect: the ssid is a string')
    },
    here: () => {
      throw new Error('acde.input: this runtime has no compositor to configure')
    },
    broke: async () => {
      throw new Error('it went wrong')
    },
    fine: async () => 7,
  })
  const caught = async (call: () => unknown) => {
    try {
      await call()
    } catch (error) {
      const e = error as { name: string; status: number; code: string; module: string; message: string }
      return [e.name, e.status, e.code, e.module]
    }
    return null
  }
  assert.deepEqual(await caught(module.denied), ['AcdeError', 403, 'forbidden', 'network'])
  assert.deepEqual(await caught(module.missing), ['AcdeError', 503, 'service_not_running', 'network'])
  assert.deepEqual(await caught(module.wrong), ['AcdeError', 400, 'bad_request', 'network'])
  assert.deepEqual(await caught(module.here), ['AcdeError', 501, 'unsupported', 'network'])
  assert.deepEqual(await caught(module.broke), ['AcdeError', 500, 'failed', 'network'])
  assert.equal(await module.fine(), 7)
  const given = Object.assign(new Error('nope'), { status: 409, code: 'failed' })
  const converted = systemError('power', given) as Error & { status: number }
  assert.deepEqual([converted.name, converted.status], ['AcdeError', 409])
})

test('network on(changed) is cancellable and watch still works', () => {
  const callbacks: ((state: unknown) => void)[] = []
  const natives = { watch: (callback: (state: unknown) => void) => void callbacks.push(callback), state: async () => ({}) }
  const network = { ...Watched.module('network', natives), watch: natives.watch } as {
    on(type: string, handler: (state: unknown) => void): () => boolean
    watch(callback: (state: unknown) => void): void
  }
  const seen: unknown[] = []
  const off = network.on('changed', (state) => void seen.push(state))
  network.watch((state) => void seen.push(['watch', state]))
  for (const callback of callbacks) callback({ online: true })
  assert.equal(off(), true)
  callbacks[0]!({ online: false })
  assert.deepEqual(seen, [{ online: true }, ['watch', { online: true }]])
})

test('storage, hostname, time zone or automatic time changing reaches subscribers in one snapshot', async () => {
  let free = 100
  let hostname = 'dopame'
  let reads = 0
  const source = {
    storage: async () => {
      reads++
      return [{ path: '/', label: 'disk', size: 1000, free }]
    },
    info: async () => ({ hostname, uptimeSeconds: reads }),
    time: async () => ({ timezone: 'Asia/Shanghai', ntp: true, canSetTime: true }),
  }
  const watch = new SystemWatch(source, 60_000)
  const seen: unknown[] = []
  const off = watch.on('changed', (change: unknown) => void seen.push(change))
  await watch.check(true)
  await watch.check(false)
  assert.deepEqual(seen, [], 'uptime moving alone is no change')
  free = 90
  await watch.check(false)
  const storage = [{ path: '/', label: 'disk', size: 1000, free: 90 }]
  assert.deepEqual(seen, [{ storage, hostname: 'dopame', timezone: 'Asia/Shanghai', ntp: true }])
  await watch.after((async () => {
    hostname = 'studio'
  })())
  assert.deepEqual(seen[1], { storage, hostname: 'studio', timezone: 'Asia/Shanghai', ntp: true }, 'a change made here is checked at once')
  assert.equal(off(), true)
  assert.ok(reads >= 4)
  assert.throws(() => watch.on('mounted', () => undefined), /no mounted event/)
})
