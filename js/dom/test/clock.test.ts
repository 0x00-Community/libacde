import assert from 'node:assert/strict'
import { test } from 'node:test'
import { page, type Web } from './harness.ts'

const START = Date.UTC(2026, 0, 2, 3, 4, 5)

async function clocked(): Promise<Web> {
  const web = await page()
  web.run(`globalThis.__log = []
    globalThis.__at = (label) => __log.push([label, Date.now() - ${START}])`)
  return web
}

const log = (web: Web) => web.json<[string, number][]>('__log.splice(0)')
const clock = (web: Web, request: object) => web.internals.clock(request as never)

test('clock: install sets Date; paused, fastForward fires each due timer once at the new time', async () => {
  using web = await clocked()
  const before = web.run<number>('setTimeout(() => __at("made before install"), 60_000)')
  assert.ok(before > 0)
  await clock(web, { action: 'install', time: START })
  await clock(web, { action: 'pauseAt', time: START + 1000 })
  web.run(`setTimeout(() => __at("timeout 5s"), 5000)
    setInterval(() => __at("every 2s"), 2000)
    __at("now")`)
  assert.deepEqual(log(web), [['now', 1000]])
  assert.equal(web.run('new Date().getTime()'), START + 1000)
  assert.equal(web.run('typeof Date()'), 'string')
  assert.ok(web.run<boolean>('new Date(0) instanceof Date && new Date(0).getTime() === 0'))
  const state = await clock(web, { action: 'fastForward', ms: 60_000 })
  assert.deepEqual(log(web), [['every 2s', 61_000], ['timeout 5s', 61_000], ['made before install', 61_000]], 'once each, at the new time')
  assert.equal(state.paused, true)
  assert.equal(state.now, START + 61_000)
})

test('clock: runFor fires timers in order at their own times, intervals every time', async () => {
  using web = await clocked()
  await clock(web, { action: 'install', time: START })
  await clock(web, { action: 'pauseAt', time: START })
  web.run(`setInterval(() => __at("tick"), 1000)
    setTimeout(() => { __at("late"); Promise.resolve().then(() => __at("microtask")) }, 2500)
    const t = performance.now()
    globalThis.__perf = () => performance.now() - t`)
  await clock(web, { action: 'runFor', ms: 3000 })
  assert.deepEqual(log(web), [['tick', 1000], ['tick', 2000], ['late', 2500], ['microtask', 2500], ['tick', 3000]])
  assert.equal(web.run('__perf()'), 3000, 'performance.now moves with it')
})

test('clock: resume lets time flow again; setFixedTime pins Date; setSystemTime moves Date only', async () => {
  using web = await clocked()
  await clock(web, { action: 'install', time: START })
  await clock(web, { action: 'pauseAt', time: START + 10 })
  web.run('setTimeout(() => __at("soon"), 30)')
  await web.wait(80)
  assert.deepEqual(log(web), [], 'paused: nothing fires on its own')
  await clock(web, { action: 'resume' })
  await web.wait(150)
  assert.equal(log(web)[0]?.[0], 'soon')
  await clock(web, { action: 'setFixedTime', time: START + 5 })
  await web.wait(20)
  assert.equal(web.run('Date.now()'), START + 5)
  await clock(web, { action: 'setSystemTime', time: START + 86_400_000 })
  assert.ok(web.run<number>('Date.now()') >= START + 86_400_000)
  assert.equal(web.run<string>('new Intl.DateTimeFormat("en-US", { timeZone: "UTC", day: "numeric", month: "numeric" }).format()'), '1/3')
})

test('clock: mistakes are explained', async () => {
  using web = await clocked()
  await assert.rejects(async () => clock(web, { action: 'runFor', ms: 10 }), /install it first/)
  await clock(web, { action: 'install' })
  assert.throws(() => clock(web, { action: 'install' }), /already installed/)
  await assert.rejects(async () => clock(web, { action: 'runFor', ms: -1 }), /not a duration/)
  const paused = await clock(web, { action: 'pauseAt', time: START - 1000 })
  assert.equal(paused.now, START - 1000, 'a time already passed: paused there, performance.now still monotonic')
})
