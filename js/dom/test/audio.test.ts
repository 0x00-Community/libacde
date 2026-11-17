import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createWeb, type Web } from './harness.ts'

interface Call {
  name: string
  args: unknown[]
}

function fakeAudio() {
  const calls: Call[] = []
  let callback: ((kind: number, id: number) => void) | null = null
  const record = (name: string) => (...args: unknown[]) => {
    calls.push({ name, args })
  }
  const media = new Map<number, number>()
  const presents: number[] = []
  const native = {
    sampleRate: 48000,
    onEvent(fn: (kind: number, id: number) => void) {
      callback = fn
    },
    createContext: record('createContext'),
    setRunning: record('setRunning'),
    closeContext: record('closeContext'),
    time: () => 1.5,
    createNode: record('createNode'),
    connect: record('connect'),
    disconnect: record('disconnect'),
    automate: record('automate'),
    automateCurve: record('automateCurve'),
    paramValue: () => 0.25,
    setWave: record('setWave'),
    setLoop: record('setLoop'),
    setPanner: record('setPanner'),
    setBuffer: record('setBuffer'),
    start: record('start'),
    stop: record('stop'),
    release: record('release'),
    decode(buffer: number, bytes: Uint8Array) {
      calls.push({ name: 'decode', args: [buffer, bytes.length] })
      if (bytes[0] !== 0x52) return Promise.reject(new Error('unsupported audio'))
      return Promise.resolve({ length: 4800, sampleRate: 48000, numberOfChannels: 2 })
    },
    createBuffer(buffer: number, sampleRate: number, channels: Float32Array[]) {
      calls.push({ name: 'createBuffer', args: [buffer, sampleRate, channels.length, channels[0]?.[0]] })
    },
    channelData: () => [new Float32Array(4800).fill(0.5), new Float32Array(4800)],
    releaseBuffer: record('releaseBuffer'),
    openMedia(id: number, bytes: Uint8Array) {
      calls.push({ name: 'openMedia', args: [id, bytes.length] })
      if (bytes[0] !== 0x52) return Promise.reject(new Error('unsupported audio'))
      media.set(id, 0)
      return Promise.resolve({ duration: 2, sampleRate: 44100, numberOfChannels: 2 })
    },
    media(id: number, op: number, value: number) {
      calls.push({ name: 'media', args: [id, op, value] })
      if (op === 2) media.set(id, value)
    },
    mediaTime: (id: number) => media.get(id) ?? 0,
    closeMedia: record('closeMedia'),
    videoTypes: ['video/mp4'],
    openVideo(id: number, node: number, bytes: Uint8Array) {
      calls.push({ name: 'openVideo', args: [id, node, bytes.length] })
      if (bytes[0] !== 0x52) return Promise.reject(new Error('unsupported video'))
      media.set(id, 0)
      return Promise.resolve({ duration: 3, width: 64, height: 48, audio: 1 })
    },
    present(id: number) {
      calls.push({ name: 'present', args: [id] })
      return presents.shift() ?? 0
    },
    videoSize: () => [96, 72],
    clearVideo: record('clearVideo'),
  }
  return {
    native,
    calls,
    presents,
    named: (name: string) => calls.filter((c) => c.name === name).map((c) => c.args),
    emit: (kind: number, id: number) => callback?.(kind, id),
  }
}

function web(): { web: Web; audio: ReturnType<typeof fakeAudio> } {
  const audio = fakeAudio()
  return { web: createWeb({ audio: audio.native as never }), audio }
}

const WAV = 'data:audio/wav;base64,' + Buffer.from('RIFF....WAVEfmt ').toString('base64')

test('a beep: the graph, its automation, start and stop go to the audio thread', () => {
  const { web: w, audio } = web()
  w.run(`
    const ctx = new AudioContext()
    const osc = ctx.createOscillator()
    const gain = ctx.createGain()
    osc.type = 'square'
    osc.frequency.value = 880
    gain.gain.setValueAtTime(0, ctx.currentTime)
    gain.gain.linearRampToValueAtTime(1, ctx.currentTime + 0.01)
    gain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 0.2)
    osc.connect(gain).connect(ctx.destination)
    osc.start()
    osc.stop(ctx.currentTime + 0.25)
    globalThis.result = { state: ctx.state, rate: ctx.sampleRate, time: ctx.currentTime, value: gain.gain.value }
  `)
  const [[ctx, dest]] = audio.named('createContext') as [[number, number]]
  const nodes = audio.named('createNode') as [number, number, number][]
  assert.deepEqual(nodes.map((n) => [n[0], n[2]]), [[ctx, 2], [ctx, 1]])
  const [osc, gain] = nodes.map((n) => n[1])
  assert.deepEqual(audio.named('setWave'), [[osc, 1]])
  assert.deepEqual(audio.named('automate'), [
    [osc, 0, 0, 880, 0, 0],
    [gain, 0, 1, 0, 1.5, 0],
    [gain, 0, 2, 1, 1.5 + 0.01, 0],
    [gain, 0, 3, 0.001, 1.5 + 0.2, 0],
  ])
  assert.deepEqual(audio.named('connect'), [[osc, gain, -1], [gain, dest, -1]])
  assert.deepEqual(audio.named('start'), [[osc, 0, 0, -1]])
  assert.deepEqual(audio.named('stop'), [[osc, 1.5 + 0.25]])
  assert.deepEqual(w.json('result'), { state: 'running', rate: 48000, time: 1.5, value: 0.25 })
  w.close()
})

test('ended comes back as the node\'s event, also for a node the page kept no reference to', async () => {
  const { web: w, audio } = web()
  w.run(`
    globalThis.ended = 0
    const ctx = new AudioContext()
    ;(() => {
      const osc = ctx.createOscillator()
      osc.connect(ctx.destination)
      osc.onended = () => { globalThis.ended++ }
      osc.start(0)
      osc.stop(0.1)
    })()
  `)
  const [[, osc]] = audio.named('createNode') as [[number, number]]
  audio.emit(0, osc)
  assert.equal(w.run('globalThis.ended'), 1)
  audio.emit(0, osc)
  assert.equal(w.run('globalThis.ended'), 1)
  w.close()
})

test('errors pages rely on', () => {
  const { web: w } = web()
  const errors = w.json<string[]>(`(() => {
    const ctx = new AudioContext()
    const out = []
    const osc = ctx.createOscillator()
    try { osc.stop() } catch (e) { out.push(e.name) }
    osc.start()
    try { osc.start() } catch (e) { out.push(e.name) }
    try { ctx.createGain().gain.exponentialRampToValueAtTime(0, 1) } catch (e) { out.push(e.name) }
    try { ctx.createGain().gain.setValueAtTime(1, -1) } catch (e) { out.push(e.name) }
    try { osc.type = 'custom' } catch (e) { out.push(e.name) }
    try { new AudioContext().createGain().connect(ctx.destination) } catch (e) { out.push(e.name) }
    try { new AudioBuffer({ length: 0, sampleRate: 48000 }) } catch (e) { out.push(e.name) }
    try { new BaseAudioContext() } catch (e) { out.push(e.name) }
    return out
  })()`)
  assert.deepEqual(errors, ['InvalidStateError', 'InvalidStateError', 'RangeError', 'RangeError', 'InvalidStateError', 'InvalidAccessError', 'NotSupportedError', 'TypeError'])
  w.close()
})

test('suspend freezes currentTime, resume and close change state with statechange', async () => {
  const { web: w, audio } = web()
  w.run(`
    globalThis.states = []
    globalThis.ctx = new AudioContext()
    ctx.onstatechange = () => states.push(ctx.state)
    ctx.suspend()
  `)
  assert.equal(w.run('ctx.state'), 'suspended')
  assert.equal(w.run('ctx.currentTime'), 1.5)
  await w.wait(5)
  w.run('ctx.resume()')
  await w.wait(5)
  w.run('ctx.close()')
  await w.wait(5)
  assert.deepEqual(w.json('states'), ['suspended', 'running', 'closed'])
  assert.deepEqual(audio.named('setRunning').map((a) => a[1]), [false, true])
  assert.equal(audio.named('closeContext').length, 1)
  assert.equal(await w.run<Promise<string>>('ctx.resume().catch((e) => e.name)'), 'InvalidStateError')
  w.close()
})

test('decodeAudioData and buffer sources: decoded natively, the page\'s own samples uploaded at start', async () => {
  const { web: w, audio } = web()
  const result = await w.run<Promise<unknown>>(`(async () => {
    const ctx = new AudioContext()
    const bytes = new Uint8Array([0x52, 0x49, 0x46, 0x46, 1, 2, 3]).buffer
    const decoded = await ctx.decodeAudioData(bytes)
    const src = ctx.createBufferSource()
    src.buffer = decoded
    src.loop = true
    src.playbackRate.value = 2
    src.connect(ctx.destination)
    src.start(0, 0.5)
    const made = ctx.createBuffer(1, 100, 22050)
    made.getChannelData(0)[0] = 0.75
    const src2 = new AudioBufferSourceNode(ctx, { buffer: made })
    src2.start()
    let failed = ''
    await ctx.decodeAudioData(new Uint8Array([1, 2]).buffer).catch((e) => { failed = e.name })
    return { length: decoded.length, rate: decoded.sampleRate, channels: decoded.numberOfChannels, duration: decoded.duration, first: decoded.getChannelData(0)[0], failed }
  })()`)
  assert.deepEqual(JSON.parse(JSON.stringify(result)), { length: 4800, rate: 48000, channels: 2, duration: 0.1, first: 0.5, failed: 'EncodingError' })
  const decodes = audio.named('decode') as [number, number][]
  const sets = audio.named('setBuffer') as [number, number][]
  assert.equal(sets[0]![1], decodes[0]![0])
  const uploads = audio.named('createBuffer')
  assert.deepEqual(uploads.map((u) => u.slice(1)), [[22050, 1, 0.75]])
  assert.equal(sets[1]![1], uploads[0]![0])
  assert.deepEqual((audio.named('setLoop') as unknown[][])[0]!.slice(1), [true, 0, 0])
  const starts = audio.named('start') as number[][]
  assert.deepEqual(starts[0]!.slice(1), [0, 0.5, -1])
  w.close()
})

test('pass-through nodes keep pages that use them playing', () => {
  const { web: w, audio } = web()
  w.run(`
    const ctx = new AudioContext()
    const f = ctx.createBiquadFilter()
    f.type = 'highpass'
    f.frequency.value = 1000
    const a = ctx.createAnalyser()
    const data = new Uint8Array(a.frequencyBinCount)
    a.getByteFrequencyData(data)
    f.connect(a).connect(ctx.destination)
    globalThis.result = { type: f.type, bins: data.length }
  `)
  assert.deepEqual(w.json('result'), { type: 'highpass', bins: 1024 })
  assert.deepEqual((audio.named('createNode') as number[][]).map((n) => n[2]), [6, 6])
  w.close()
})

test('a PannerNode and the context\'s AudioListener: params, attributes and their checks reach the audio thread', () => {
  const { web: w, audio } = web()
  w.run(`
    const ctx = new AudioContext()
    const p = ctx.createPanner()
    const errors = []
    const tryIt = (f) => { try { f(); errors.push('none') } catch (e) { errors.push(e.name) } }
    p.panningModel = 'HRTF'
    p.panningModel = 'bogus'
    p.distanceModel = 'linear'
    p.refDistance = 3
    p.rolloffFactor = 0.6
    p.maxDistance = 400
    p.coneOuterGain = 0.25
    tryIt(() => { p.refDistance = -1 })
    tryIt(() => { p.maxDistance = 0 })
    tryIt(() => { p.rolloffFactor = -0.1 })
    tryIt(() => { p.coneOuterGain = 2 })
    tryIt(() => { p.refDistance = NaN })
    tryIt(() => { p.channelCount = 3 })
    tryIt(() => { p.channelCountMode = 'max' })
    tryIt(() => { p.channelCount = 1 })
    tryIt(() => new PannerNode(ctx, { panningModel: 'stereo' }))
    p.positionX.linearRampToValueAtTime(5, 2)
    p.setPosition(1, 2, 3)
    p.orientationY.setValueCurveAtTime([0, 1], 0, 10)
    tryIt(() => p.setOrientation(0, 0, 1))
    const l = ctx.listener
    l.setOrientation(1, 0, 0, 0, 1, 0)
    l.positionX.value = 7
    const src = ctx.createBufferSource()
    src.connect(p).connect(ctx.destination)
    src.connect(l.positionY)
    const q = new PannerNode(ctx, { panningModel: 'HRTF', positionZ: -4, coneInnerAngle: 90 })
    globalThis.result = {
      errors,
      model: p.panningModel, distance: p.distanceModel, ref: p.refDistance, count: p.channelCount, mode: p.channelCountMode,
      same: ctx.listener === l, isNode: l instanceof AudioNode, forward: [l.forwardX.defaultValue, l.forwardY.defaultValue, l.forwardZ.defaultValue],
      up: l.upY.defaultValue, orientation: p.orientationX.defaultValue, q: [q.panningModel, q.coneInnerAngle],
      globals: [typeof PannerNode, typeof AudioListener],
    }
  `)
  assert.deepEqual(w.json('result'), {
    errors: ['RangeError', 'RangeError', 'RangeError', 'InvalidStateError', 'TypeError', 'NotSupportedError', 'NotSupportedError', 'none', 'TypeError', 'NotSupportedError'],
    model: 'HRTF', distance: 'linear', ref: 3, count: 1, mode: 'clamped-max',
    same: true, isNode: false, forward: [0, 0, -1], up: 1, orientation: 1, q: ['HRTF', 90],
    globals: ['function', 'function'],
  })
  const nodes = audio.named('createNode') as [number, number, number][]
  const panner = nodes.find((n) => n[2] === 8)![1]
  const listener = nodes.find((n) => n[2] === 9)![1]
  const source = nodes.find((n) => n[2] === 3)![1]
  const settings = (audio.named('setPanner') as unknown[][]).filter((c) => c[0] === panner)
  assert.deepEqual(settings.at(-1), [panner, true, 0, 3, 400, 0.6, 360, 360, 0.25])
  assert.deepEqual(settings[0], [panner, false, 1, 1, 10000, 1, 360, 360, 0])
  const automation = audio.named('automate') as number[][]
  assert.deepEqual(automation.filter((a) => a[0] === panner && a[2] === 0).map((a) => [a[1], a[3]]), [[0, 1], [1, 2], [2, 3]])
  assert.deepEqual(automation.filter((a) => a[0] === listener).map((a) => [a[1], a[3]]), [[3, 1], [4, 0], [5, 0], [6, 0], [7, 1], [8, 0], [0, 7]])
  assert.deepEqual(audio.named('connect'), [[source, panner, -1], [panner, (audio.named('createContext') as number[][])[0]![1], -1], [source, listener, 1]])
  w.close()
})

test('new Audio(src).play(): loads, plays, reports time, ends', async () => {
  const { web: w, audio } = web()
  w.run(`
    globalThis.events = []
    globalThis.a = new Audio(${JSON.stringify(WAV)})
    for (const type of ['loadstart', 'loadedmetadata', 'canplaythrough', 'play', 'playing', 'pause', 'ended', 'error', 'volumechange'])
      a.addEventListener(type, () => events.push(type))
    globalThis.played = a.play().then(() => 'ok', (e) => e.name)
  `)
  await w.wait(20)
  assert.equal(await w.run<Promise<string>>('played'), 'ok')
  const [[id]] = audio.named('openMedia') as [[number]]
  assert.deepEqual(audio.named('media').filter((m) => m[1] === 0), [[id, 0, 0]])
  assert.equal(w.run('a.duration'), 2)
  assert.equal(w.run('a.paused'), false)
  w.run('a.volume = 0.5; a.muted = true; a.currentTime = 1.25')
  assert.equal(w.run('a.currentTime'), 1.25)
  assert.deepEqual(audio.named('media').filter((m) => m[1] === 3).map((m) => m[2]), [1, 0.5, 0])
  audio.emit(1, id as number)
  await w.wait(5)
  assert.equal(w.run('a.ended'), true)
  assert.equal(w.run('a.paused'), true)
  assert.deepEqual(w.json<string[]>('events').filter((e) => e !== 'volumechange'), ['loadstart', 'play', 'loadedmetadata', 'canplaythrough', 'playing', 'pause', 'ended'])
  w.close()
})

test('pause() before the source is loaded rejects play() with AbortError; a bad source rejects NotSupportedError', async () => {
  const { web: w } = web()
  w.run(`
    globalThis.a = new Audio(${JSON.stringify(WAV)})
    globalThis.first = a.play().catch((e) => e.name)
    a.pause()
    globalThis.bad = new Audio('data:audio/wav;base64,AAAA')
    globalThis.badErrors = []
    bad.onerror = () => badErrors.push(bad.error.code)
    globalThis.second = bad.play().catch((e) => e.name)
  `)
  await w.wait(20)
  assert.equal(await w.run<Promise<string>>('first'), 'AbortError')
  assert.equal(await w.run<Promise<string>>('second'), 'NotSupportedError')
  assert.deepEqual(w.json('badErrors'), [4])
  assert.equal(w.run('document.createElement("audio").canPlayType("audio/mpeg")'), 'maybe')
  assert.equal(w.run('document.createElement("audio").canPlayType("audio/webm; codecs=opus")'), '')
  assert.equal(await w.run<Promise<string>>('document.createElement("video").play().catch((e) => e.name)'), 'NotSupportedError')
  w.close()
})

test('without native audio: no AudioContext, play() rejects NotSupportedError', async () => {
  const w = createWeb()
  assert.equal(w.run('typeof AudioContext'), 'undefined')
  assert.equal(await w.run<Promise<string>>(`new Audio(${JSON.stringify(WAV)}).play().catch((e) => e.name)`), 'NotSupportedError')
  w.close()
})

const LISTEN = (id: string) => `
  globalThis.events = []
  for (const type of ['loadstart', 'suspend', 'durationchange', 'resize', 'loadedmetadata', 'loadeddata', 'canplay', 'canplaythrough', 'play', 'playing', 'pause', 'error', 'emptied'])
    document.getElementById(${JSON.stringify(id)}).addEventListener(type, () => events.push(type))
`

test('a parsed <video src> loads by itself, and knows its size by loadedmetadata without a resize event', async () => {
  const { web: w, audio } = web()
  w.run(`document.body.innerHTML = '<video id="v" src="${WAV}"></video>'; ${LISTEN('v')}; globalThis.v = document.getElementById('v')`)
  await w.wait(20)
  const [[, node]] = audio.named('openVideo') as [[number, number]]
  assert.equal(node, w.internals.idOf(w.run('v')))
  assert.deepEqual(w.json('events'), ['loadstart', 'durationchange', 'loadedmetadata', 'loadeddata', 'canplay', 'canplaythrough'])
  assert.deepEqual(w.json('[v.videoWidth, v.videoHeight, v.readyState, v.duration]'), [64, 48, 4, 3])
  w.close()
})

test('a playing video shows a frame each animation frame; a new frame size fires resize', async () => {
  const { web: w, audio } = web()
  w.run(`document.body.innerHTML = '<video id="v" src="${WAV}"></video>'; ${LISTEN('v')}; globalThis.v = document.getElementById('v')`)
  await w.wait(20)
  w.run('globalThis.played = v.play().then(() => "ok", (e) => e.name)')
  assert.equal(await w.run<Promise<string>>('played'), 'ok')
  audio.presents.push(1, 2)
  w.frame()
  w.frame()
  await w.wait(5)
  assert.ok(audio.named('present').length >= 2)
  assert.deepEqual(w.json('[v.videoWidth, v.videoHeight, v.getVideoPlaybackQuality().totalVideoFrames]'), [96, 72, 3])
  assert.ok(w.json<string[]>('events').includes('resize'))
  w.run('v.removeAttribute("src"); v.load()')
  assert.deepEqual(w.json('[v.videoWidth, v.videoHeight, v.readyState]'), [0, 0, 0])
  assert.equal(audio.named('clearVideo').at(-1)?.[0], w.internals.idOf(w.run('v')))
  w.close()
})

test('preload=none: the element waits for play() before it fetches', async () => {
  const { web: w, audio } = web()
  w.run(`document.body.innerHTML = '<audio id="a" preload="none" src="${WAV}"></audio>'; ${LISTEN('a')}; globalThis.a = document.getElementById('a')`)
  await w.wait(20)
  assert.equal(audio.named('openMedia').length, 0)
  assert.deepEqual(w.json('events'), ['loadstart', 'suspend'])
  assert.equal(w.run('a.networkState'), 1)
  assert.equal(await w.run<Promise<string>>('a.play().then(() => "ok", (e) => e.name)'), 'ok')
  assert.equal(audio.named('openMedia').length, 1)
  w.close()
})

test('a cloned or imported <video src> loads too; canPlayType knows the native containers', async () => {
  const { web: w, audio } = web()
  w.run(`
    const t = document.createElement('template')
    t.innerHTML = '<div><video src="${WAV}"></video></div>'
    document.body.append(document.importNode(t.content, true), t.content.firstChild.firstChild.cloneNode())
  `)
  await w.wait(20)
  assert.equal(audio.named('openVideo').length, 2)
  assert.deepEqual(w.json('["video/mp4", "video/mp4; codecs=avc1", "video/webm", "audio/mpeg"].map((t) => document.createElement("video").canPlayType(t))'), ['maybe', 'maybe', '', 'maybe'])
  w.close()
})

test('a playing media element taken out of the document pauses; moving it does not', async () => {
  const { web: w } = web()
  w.run(`document.body.innerHTML = '<div id="d"><video id="v" src="${WAV}"></video></div><p id="p"></p>'; ${LISTEN('v')}; globalThis.v = document.getElementById('v')`)
  await w.wait(20)
  assert.equal(await w.run<Promise<string>>('v.play().then(() => "ok", (e) => e.name)'), 'ok')
  w.run('document.getElementById("p").append(v)')
  await w.wait(5)
  assert.equal(w.run('v.paused'), false)
  w.run('v.remove()')
  await w.wait(5)
  assert.equal(w.run('v.paused'), true)
  assert.ok(w.json<string[]>('events').includes('pause'))
  w.close()
})
