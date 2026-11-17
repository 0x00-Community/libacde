import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createWeb, type Web } from './harness.ts'

type Call = unknown[]

const DEVICES = [
  { deviceId: 'mic1', kind: 'audioinput', label: 'Mic', groupId: 'g1', facingMode: null, modes: [], channelCount: 1 },
  { deviceId: 'cam1', kind: 'videoinput', label: 'Front', groupId: 'g1', facingMode: 'user', modes: [[1280, 720, 30], [640, 480, 30], [320, 240, 15]], channelCount: 0 },
  { deviceId: 'cam2', kind: 'videoinput', label: 'Back', groupId: 'g2', facingMode: 'environment', modes: [[1920, 1080, 60]], channelCount: 0 },
]

function fakeCapture(state: { camera: string; microphone: string } = { camera: 'granted', microphone: 'granted' }) {
  const calls: Call[] = []
  let onEnded: ((source: number, reason: string) => void) | null = null
  let next = 1
  const native = {
    devices(full: boolean) {
      calls.push(['devices', full])
      const shown = full ? DEVICES : DEVICES.map((d) => (state[d.kind === 'audioinput' ? 'microphone' : 'camera'] === 'granted' ? d : { ...d, deviceId: '', label: '', groupId: '' }))
      return Promise.resolve(shown)
    },
    permission: (kind: 'camera' | 'microphone') => state[kind],
    open(kind: string, deviceId: string, width: number, height: number, frameRate: number) {
      calls.push(['open', kind, deviceId, width, height, frameRate])
      const device = DEVICES.find((d) => d.deviceId === deviceId)
      if (!device) return Promise.reject(new Error('NotFoundError: gone'))
      if (state[kind === 'audioinput' ? 'microphone' : 'camera'] === 'prompt') state[kind === 'audioinput' ? 'microphone' : 'camera'] = 'granted'
      return Promise.resolve({ ...device, source: next++, width, height, frameRate })
    },
    close: (source: number) => calls.push(['close', source]),
    onEnded: (fn: (source: number, reason: string) => void) => (onEnded = fn),
  }
  return { native, calls, end: (source: number) => onEnded?.(source, 'unplugged'), named: (name: string) => calls.filter((c) => c[0] === name).map((c) => c.slice(1)) }
}

function fakeAudio() {
  const calls: Call[] = []
  const answers: Record<string, unknown> = {
    sampleRate: 48000,
    videoTypes: [],
    openStream: () => ({ duration: Infinity, width: 640, height: 480, audio: 1 }),
    mediaTime: () => 0,
    time: () => 0,
    paramValue: () => 0,
  }
  const native = new Proxy({}, {
    get: (_target, name: string) => {
      const answer = answers[name]
      if (answer !== undefined && typeof answer !== 'function') return answer
      return (...args: unknown[]) => {
        calls.push([name, ...args])
        return typeof answer === 'function' ? (answer as (...a: unknown[]) => unknown)(...args) : undefined
      }
    },
  })
  return { native, calls, named: (name: string) => calls.filter((c) => c[0] === name).map((c) => c.slice(1)) }
}

function setup(state?: { camera: string; microphone: string }): { web: Web; capture: ReturnType<typeof fakeCapture>; audio: ReturnType<typeof fakeAudio> } {
  const capture = fakeCapture(state)
  const audio = fakeAudio()
  return { web: createWeb({ capture: capture.native as never, audio: audio.native as never }), capture, audio }
}

const gum = (w: Web, constraints: string) => w.run<Promise<unknown>>(`navigator.mediaDevices.getUserMedia(${constraints}).then((s) => { globalThis.stream = s; return 'ok' }, (e) => e.name + (e.constraint ? ':' + e.constraint : ''))`)

test('getUserMedia needs audio or video, and is refused without the capability', async () => {
  const { web: w } = setup()
  assert.equal(await w.run<Promise<string>>('navigator.mediaDevices.getUserMedia({}).catch((e) => e.name)'), 'TypeError')
  assert.equal(await w.run<Promise<string>>('navigator.mediaDevices.getUserMedia().catch((e) => e.name)'), 'TypeError')
  w.close()
  const bare = createWeb()
  assert.equal(await bare.run<Promise<string>>('navigator.mediaDevices.getUserMedia({ video: true }).catch((e) => e.name)'), 'NotAllowedError')
  assert.equal(await bare.run<Promise<string>>('navigator.permissions.query({ name: "camera" }).then((s) => s.state)'), 'denied')
  bare.close()
})

test("constraints pick the device and mode the spec's fitness distance prefers", async () => {
  const cases: [string, unknown[]][] = [
    ['{ video: true }', ['videoinput', 'cam1', 1280, 720, 30]],
    ['{ video: { width: 640 } }', ['videoinput', 'cam1', 640, 480, 30]],
    ['{ video: { width: { exact: 1920 } } }', ['videoinput', 'cam2', 1920, 1080, 60]],
    ['{ video: { facingMode: "environment" } }', ['videoinput', 'cam2', 1920, 1080, 60]],
    ['{ video: { deviceId: { exact: "cam1" }, frameRate: { max: 20 } } }', ['videoinput', 'cam1', 320, 240, 15]],
    ['{ video: { advanced: [{ width: 320 }] } }', ['videoinput', 'cam1', 320, 240, 15]],
    ['{ video: { aspectRatio: 4 / 3, height: { ideal: 1000 } } }', ['videoinput', 'cam2', 1920, 1080, 60]],
    ['{ video: { aspectRatio: { exact: 4 / 3 }, height: { ideal: 1000 } } }', ['videoinput', 'cam1', 640, 480, 30]],
    ['{ audio: true }', ['audioinput', 'mic1', 0, 0, 0]],
  ]
  for (const [constraints, open] of cases) {
    const { web: w, capture } = setup()
    assert.equal(await gum(w, constraints), 'ok', constraints)
    assert.deepEqual(capture.named('open'), [open], constraints)
    assert.deepEqual(capture.named('devices'), [[true]])
    w.close()
  }
  const { web: w, capture } = setup()
  assert.equal(await gum(w, '{ video: { width: { min: 4000 } } }'), 'OverconstrainedError:width')
  assert.equal(await gum(w, '{ video: { facingMode: { exact: "left" } } }'), 'OverconstrainedError:facingMode')
  assert.equal(await gum(w, '{ audio: { echoCancellation: { exact: true } } }'), 'OverconstrainedError:echoCancellation')
  assert.equal(await gum(w, '{ video: { width: 640, unknownThing: { exact: 5 } } }'), 'ok')
  assert.equal(capture.named('open').length, 1)
  w.close()
})

test('a track: its settings, capabilities and constraints; clones share the device until the last stops', async () => {
  const { web: w, capture } = setup()
  assert.equal(await gum(w, '{ audio: true, video: { width: 640 } }'), 'ok')
  const info = w.json<Record<string, unknown>>(`(() => {
    const [audio] = stream.getAudioTracks(), [video] = stream.getVideoTracks()
    globalThis.video = video
    return {
      tracks: stream.getTracks().map((t) => t.kind), active: stream.active, label: video.label, state: video.readyState,
      settings: video.getSettings(), constraints: video.getConstraints(), caps: video.getCapabilities(),
      audio: audio.getSettings(), byId: stream.getTrackById(video.id) === video, idLength: stream.id.length,
    }
  })()`)
  assert.deepEqual(info['tracks'], ['audio', 'video'])
  assert.equal(info['active'], true)
  assert.equal(info['label'], 'Front')
  assert.deepEqual(info['settings'], { deviceId: 'cam1', groupId: 'g1', width: 640, height: 480, frameRate: 30, aspectRatio: 640 / 480, resizeMode: 'none', facingMode: 'user' })
  assert.deepEqual(info['constraints'], { width: 640 })
  assert.deepEqual((info['caps'] as Record<string, unknown>)['width'], { min: 320, max: 1280 })
  assert.equal((info['audio'] as Record<string, unknown>)['sampleRate'], 48000)
  assert.equal(info['byId'], true)
  assert.equal(info['idLength'], 36)
  w.run('globalThis.twin = video.clone(); video.stop()')
  assert.equal(w.run('video.readyState'), 'ended')
  assert.equal(w.run('twin.readyState'), 'live')
  assert.deepEqual(capture.named('close'), [])
  w.run('twin.stop()')
  assert.deepEqual(capture.named('close'), [[2]])
  assert.equal(w.run('stream.active'), true)
  w.run('stream.getAudioTracks()[0].stop()')
  assert.equal(w.run('stream.active'), false)
  w.close()
})

test('a device that goes away ends its tracks with an ended event', async () => {
  const { web: w, capture } = setup()
  assert.equal(await gum(w, '{ video: true }'), 'ok')
  w.run('globalThis.ended = []; const [t] = stream.getVideoTracks(); t.onended = () => ended.push(t.readyState); globalThis.copy = t.clone(); copy.addEventListener("ended", () => ended.push("copy"))')
  capture.end(1)
  assert.deepEqual(w.json('ended'), ['ended', 'copy'])
  assert.deepEqual(capture.named('close'), [[1]])
  w.close()
})

test('applyConstraints reopens the device in the new mode, or rejects what it cannot meet', async () => {
  const { web: w, capture } = setup()
  assert.equal(await gum(w, '{ video: { width: 640 } }'), 'ok')
  assert.equal(await w.run<Promise<string>>('stream.getVideoTracks()[0].applyConstraints({ width: { exact: 9999 } }).then(() => "ok", (e) => e.name + ":" + e.constraint)'), 'OverconstrainedError:width')
  assert.equal(await w.run<Promise<string>>('stream.getVideoTracks()[0].applyConstraints({ width: 320 }).then(() => "ok", (e) => e.name)'), 'ok')
  assert.deepEqual(capture.named('open').at(-1), ['videoinput', 'cam1', 320, 240, 15])
  assert.deepEqual(capture.named('close'), [[1]])
  assert.deepEqual(w.json('[stream.getVideoTracks()[0].getSettings().width, stream.getVideoTracks()[0].getConstraints()]'), [320, { width: 320 }])
  assert.equal(await w.run<Promise<string>>('stream.getVideoTracks()[0].applyConstraints({ height: 240 }).then(() => "ok")'), 'ok')
  assert.equal(capture.named('open').length, 2)
  w.close()
})

test('permissions: denied refuses at once; a prompt answered turns the status to granted with a change event', async () => {
  const denied = setup({ camera: 'denied', microphone: 'granted' })
  assert.equal(await gum(denied.web, '{ video: true }'), 'NotAllowedError')
  assert.deepEqual(denied.capture.named('open'), [])
  denied.web.close()
  const { web: w } = setup({ camera: 'prompt', microphone: 'granted' })
  await w.run<Promise<unknown>>('navigator.permissions.query({ name: "camera" }).then((s) => { globalThis.perm = s; globalThis.changes = []; s.onchange = () => changes.push(s.state) })')
  assert.equal(w.run('perm.state'), 'prompt')
  assert.equal(w.run('perm.name'), 'camera')
  assert.equal(await gum(w, '{ video: true }'), 'ok')
  assert.deepEqual(w.json('changes'), ['granted'])
  assert.equal(await w.run<Promise<string>>('navigator.permissions.query({ name: "nonsense" }).catch((e) => e.name)'), 'TypeError')
  assert.equal(await w.run<Promise<string>>('navigator.permissions.query({ name: "geolocation" }).then((s) => s.state)'), 'denied')
  assert.equal(w.run('Object.prototype.toString.call(perm)'), '[object PermissionStatus]')
  w.close()
})

type Name = 'camera' | 'microphone'

function asking(state: { camera: string; microphone: string }) {
  const capture = fakeCapture(state)
  const asked: Name[] = []
  const waiting = new Map<Name, (allowed: boolean) => void>()
  let changed: (() => void) | null = null
  const decide = (kind: Name, allowed: boolean) => {
    state[kind] = allowed ? 'granted' : 'denied'
    changed?.()
  }
  const native = {
    ...capture.native,
    request(kind: Name) {
      if (state[kind] !== 'prompt') return Promise.resolve(state[kind] === 'granted')
      asked.push(kind)
      return new Promise<boolean>((resolve) => waiting.set(kind, resolve)).then((allowed) => (decide(kind, allowed), allowed))
    },
    onPermission: (fn: () => void) => (changed = fn),
  }
  const web = createWeb({ capture: native as never, audio: fakeAudio().native as never })
  const answer = async (kind: Name, allowed: boolean) => {
    for (let i = 0; i < 50 && !waiting.has(kind); i++) await new Promise((resolve) => setTimeout(resolve, 1))
    const settle = waiting.get(kind)
    assert.ok(settle, `no prompt for ${kind}`)
    waiting.delete(kind)
    settle(allowed)
  }
  return { web, capture, asked, answer, decide }
}

const watch = (w: Web, name: Name) =>
  w.run<Promise<unknown>>(`navigator.permissions.query({ name: "${name}" }).then((s) => { globalThis.${name} = s; globalThis.${name}Changes = []; s.onchange = () => ${name}Changes.push(s.state) })`)

test('getUserMedia raises the prompt of a capability that was not asked for, and opens the device once it is allowed', async () => {
  const { web: w, capture, asked, answer } = asking({ camera: 'prompt', microphone: 'granted' })
  await watch(w, 'camera')
  assert.equal(w.run('camera.state'), 'prompt')
  const stream = gum(w, '{ video: true }')
  await answer('camera', true)
  assert.equal(await stream, 'ok')
  assert.deepEqual(asked, ['camera'])
  assert.deepEqual(capture.calls.map((c) => c[0]), ['devices', 'open'])
  assert.deepEqual(w.json('cameraChanges'), ['granted'])
  assert.equal(await gum(w, '{ video: true }'), 'ok')
  assert.deepEqual(asked, ['camera'], 'an allowed capability is not asked for again')
  w.close()
})

test('getUserMedia rejects NotAllowedError when the prompt is denied, opens nothing, and does not ask again', async () => {
  const { web: w, capture, asked, answer } = asking({ camera: 'prompt', microphone: 'prompt' })
  await watch(w, 'camera')
  const stream = gum(w, '{ video: true }')
  await answer('camera', false)
  assert.equal(await stream, 'NotAllowedError')
  assert.deepEqual(capture.named('open'), [])
  assert.equal(w.run('camera.state'), 'denied')
  assert.deepEqual(w.json('cameraChanges'), ['denied'])
  assert.equal(await gum(w, '{ video: true }'), 'NotAllowedError')
  assert.deepEqual(asked, ['camera'])
  w.close()
})

test('audio and video are asked for one after the other; a denial stops there', async () => {
  const both = asking({ camera: 'prompt', microphone: 'prompt' })
  const stream = gum(both.web, '{ audio: true, video: true }')
  await both.answer('microphone', true)
  await both.answer('camera', true)
  assert.equal(await stream, 'ok')
  assert.deepEqual(both.asked, ['microphone', 'camera'])
  assert.deepEqual(both.capture.named('open').map((c) => c[0]), ['audioinput', 'videoinput'])
  both.web.close()
  const first = asking({ camera: 'prompt', microphone: 'prompt' })
  const refused = gum(first.web, '{ audio: true, video: true }')
  await first.answer('microphone', false)
  assert.equal(await refused, 'NotAllowedError')
  assert.deepEqual(first.asked, ['microphone'])
  assert.deepEqual(first.capture.named('open'), [])
  assert.equal(await first.web.run<Promise<string>>('navigator.permissions.query({ name: "camera" }).then((s) => s.state)'), 'prompt')
  first.web.close()
})

test('constraints no device meets are refused before anybody is asked', async () => {
  const { web: w, asked } = asking({ camera: 'prompt', microphone: 'prompt' })
  assert.equal(await gum(w, '{ video: { width: { exact: 9999 } } }'), 'OverconstrainedError:width')
  assert.deepEqual(asked, [])
  w.close()
})

test('a permission status follows a decision made elsewhere', async () => {
  const { web: w, decide } = asking({ camera: 'prompt', microphone: 'prompt' })
  await watch(w, 'camera')
  await watch(w, 'microphone')
  decide('camera', true)
  assert.deepEqual(w.json('cameraChanges'), ['granted'])
  assert.deepEqual(w.json('microphoneChanges'), [])
  decide('camera', false)
  decide('microphone', false)
  assert.deepEqual(w.json('cameraChanges'), ['granted', 'denied'])
  assert.deepEqual(w.json('microphoneChanges'), ['denied'])
  w.close()
})

test('enumerateDevices: InputDeviceInfo with capabilities; labels hidden until permission is granted', async () => {
  const { web: w } = setup({ camera: 'prompt', microphone: 'granted' })
  const list = JSON.parse(await w.run<Promise<string>>('navigator.mediaDevices.enumerateDevices().then((l) => JSON.stringify(l.map((d) => [d.constructor.name, d.kind, d.label, d.deviceId, JSON.stringify(d.getCapabilities().width ?? null)])))'))
  assert.deepEqual(list, [
    ['InputDeviceInfo', 'audioinput', 'Mic', 'mic1', 'null'],
    ['InputDeviceInfo', 'videoinput', '', '', 'null'],
    ['InputDeviceInfo', 'videoinput', '', '', 'null'],
  ])
  assert.equal(await gum(w, '{ video: true }'), 'ok')
  const after = JSON.parse(await w.run<Promise<string>>('navigator.mediaDevices.enumerateDevices().then((l) => JSON.stringify(l.filter((d) => d.kind === "videoinput").map((d) => [d.label, JSON.stringify(d.getCapabilities().width), JSON.stringify(d)])))'))
  assert.deepEqual(after, [
    ['Front', '{"min":320,"max":1280}', '{"deviceId":"cam1","kind":"videoinput","label":"Front","groupId":"g1"}'],
    ['Back', '{"min":1920,"max":1920}', '{"deviceId":"cam2","kind":"videoinput","label":"Back","groupId":"g2"}'],
  ])
  assert.equal(w.json<Record<string, boolean>>('navigator.mediaDevices.getSupportedConstraints()')['facingMode'], true)
  w.close()
})

test('video.srcObject plays a camera stream: live, unseekable, following its tracks', async () => {
  const { web: w, audio } = setup()
  assert.equal(await gum(w, '{ audio: true, video: true }'), 'ok')
  w.run(`
    globalThis.v = document.createElement('video')
    document.body.append(v)
    globalThis.events = []
    for (const type of ['loadstart', 'loadedmetadata', 'loadeddata', 'play', 'playing', 'pause', 'ended', 'resize', 'error']) v.addEventListener(type, () => events.push(type))
    v.srcObject = stream
  `)
  await w.wait(10)
  const [[media, node, video, sound]] = audio.named('openStream') as [[number, number, number, number]]
  assert.equal(node, w.internals.idOf(w.run('v')))
  assert.deepEqual([video, sound], [2, 1])
  assert.deepEqual(w.json('[v.srcObject === stream, v.currentSrc, v.duration, v.videoWidth, v.videoHeight, v.readyState, v.seekable.length]'), [true, '', null, 640, 480, 4, 0])
  assert.equal(w.run('v.duration === Infinity'), true)
  assert.equal(await w.run<Promise<string>>('v.play().then(() => "ok", (e) => e.name)'), 'ok')
  w.run('v.currentTime = 5')
  assert.deepEqual(audio.named('media').filter((m) => m[1] === 2), [])
  w.run('stream.getVideoTracks()[0].enabled = false')
  assert.deepEqual(audio.named('streamEnabled').at(-1), [media, false])
  w.run('stream.getAudioTracks()[0].enabled = false')
  assert.deepEqual(audio.named('media').filter((m) => m[1] === 3).at(-1), [media, 3, 0])
  w.run('stream.getTracks().forEach((t) => t.stop())')
  await w.wait(10)
  assert.deepEqual(w.json('events'), ['loadstart', 'loadedmetadata', 'loadeddata', 'play', 'playing', 'pause', 'ended'])
  assert.equal(w.run('v.ended'), true)
  w.run('v.srcObject = null')
  assert.equal(w.run('v.srcObject'), null)
  assert.throws(() => w.run('v.srcObject = {}'), /MediaStream/)
  w.close()
})

test("createMediaStreamSource feeds the graph from the stream's first audio track", async () => {
  const { web: w, audio } = setup()
  assert.equal(await gum(w, '{ audio: true }'), 'ok')
  w.run('globalThis.ctx = new AudioContext(); globalThis.src = ctx.createMediaStreamSource(stream); src.connect(ctx.destination)')
  const created = audio.named('createNode').at(-1) as number[]
  assert.equal(created[2], 7)
  assert.deepEqual(audio.named('streamNode'), [[created[1], 1]])
  assert.deepEqual(w.json('[src.mediaStream === stream, src.numberOfInputs, src.numberOfOutputs, src instanceof MediaStreamAudioSourceNode]'), [true, 0, 1, true])
  w.run('stream.getAudioTracks()[0].enabled = false')
  assert.deepEqual(audio.named('streamNode').at(-1), [created[1], 0])
  assert.equal(w.run('(() => { try { ctx.createMediaStreamSource(new MediaStream()) } catch (e) { return e.name } })()'), 'InvalidStateError')
  w.close()
})
