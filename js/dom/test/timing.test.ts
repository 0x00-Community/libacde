import assert from 'node:assert/strict'
import { test } from 'node:test'
import { page } from './harness.ts'

test('timers, microtasks and animation frames run in browser order', async () => {
  using web = await page()
  web.run(`
    globalThis.log = []
    setTimeout(() => log.push('timeout 10'), 10)
    setTimeout((a, b) => log.push('timeout 0 ' + a + b), 0, 'x', 'y')
    const cancelled = setTimeout(() => log.push('cancelled'), 0)
    clearTimeout(cancelled)
    Promise.resolve().then(() => log.push('promise'))
    queueMicrotask(() => log.push('microtask'))
    requestAnimationFrame((t) => { log.push('raf 1 ' + (typeof t)); requestAnimationFrame(() => log.push('raf next frame')) })
    requestAnimationFrame(() => log.push('raf 2'))
    const dropped = requestAnimationFrame(() => log.push('dropped'))
    cancelAnimationFrame(dropped)
    let n = 0
    globalThis.interval = setInterval(() => { log.push('interval ' + ++n); if (n === 2) clearInterval(interval) }, 1)
    log.push('sync')
  `)
  assert.ok(web.host.frameRequests > 0, 'rAF asks the host for a frame')
  await web.wait(0)
  web.frame()
  await web.wait(20)
  web.frame()
  const log = web.json<string[]>('log')
  assert.deepEqual(log.slice(0, 3), ['sync', 'promise', 'microtask'])
  assert.ok(log.indexOf('timeout 0 xy') > 2)
  assert.ok(log.indexOf('raf 1 number') < log.indexOf('raf 2'))
  assert.ok(log.indexOf('raf next frame') > log.indexOf('raf 2'))
  assert.ok(log.includes('timeout 10') && log.includes('interval 2') && !log.includes('interval 3'))
  assert.ok(!log.includes('cancelled') && !log.includes('dropped'))
})

test('rAF timestamps share the page time origin with performance.now(), whatever time the host reports', async () => {
  using web = await page()
  web.run(`
    globalThis.stamps = []
    globalThis.before = performance.now()
    requestAnimationFrame((t) => stamps.push([t, performance.now()]))
    requestAnimationFrame((t) => stamps.push([t, performance.now()]))
  `)
  web.frame(589583.85)
  web.run('requestAnimationFrame((t) => stamps.push([t, performance.now()]))')
  await web.wait(5)
  web.frame(589599.85)
  const before = web.run<number>('before')
  const [first, second, third] = web.json<[number, number][]>('stamps')
  assert.ok(first && second && third, 'three callbacks ran')
  assert.equal(first[0], second[0], 'one timestamp for every callback of a frame')
  for (const [t, now] of [first, second, third]) assert.ok(t >= before && t <= now, `rAF time ${t} is on the page clock (${before}..${now})`)
  assert.ok(third[0] > first[0], 'later frames have later timestamps')
})

test('frame order: scroll events, rAF, ResizeObserver, IntersectionObserver; image load events', async () => {
  using web = await page('<!doctype html><html><body><div id="scroller" data-scroll-height="1000"><div id="box"></div></div><img id="img"></body></html>')
  const id = (sel: string) => web.internals.idOf(web.run(`document.querySelector('${sel}')`))
  web.dom.boxes.set(id('#scroller'), { x: 0, y: 0, width: 300, height: 200 })
  web.dom.boxes.set(id('#box'), { x: 0, y: 100, width: 100, height: 50 })
  web.run(`
    globalThis.log = []
    const scroller = document.getElementById('scroller'), box = document.getElementById('box')
    scroller.addEventListener('scroll', () => log.push('scroll ' + scroller.scrollTop))
    requestAnimationFrame(() => log.push('raf'))
    new ResizeObserver((entries) => log.push('resize ' + entries.map((e) => e.contentRect.width + 'x' + e.contentRect.height).join())).observe(box)
    new IntersectionObserver((entries) => log.push('intersect ' + entries.map((e) => e.isIntersecting + ' ' + e.intersectionRatio).join())).observe(box)
    scroller.scrollTop = 120
    const img = document.getElementById('img')
    img.onload = () => log.push('load ' + img.naturalWidth + 'x' + img.naturalHeight + ' ' + img.complete)
    img.addEventListener('error', () => log.push('error'))
    img.src = '/pic.png'
  `)
  assert.equal(web.run('document.getElementById("scroller").scrollTop'), 120)
  await web.wait(5)
  web.frame()
  assert.deepEqual(web.json('log'), ['scroll 120', 'raf', 'resize 100x50', 'intersect true 1', 'load 16x8 true'])
  web.dom.boxes.set(id('#box'), { x: 0, y: 900, width: 120, height: 50 })
  web.run('log.length = 0; document.getElementById("box").setAttribute("data-x", "1")')
  web.endTask()
  web.frame()
  assert.deepEqual(web.json('log'), ['resize 120x50', 'intersect false 0'])
  web.run('log.length = 0')
  web.frame()
  assert.deepEqual(web.json('log'), [])
})

test('Web Animations: animate() runs its clock; finished, onfinish, cancel, getAnimations', async () => {
  using web = await page('<!doctype html><html><body><div id="a"></div></body></html>')
  web.run(`
    globalThis.log = []
    const a = document.getElementById('a')
    const anim = a.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 10, fill: 'forwards' })
    anim.onfinish = () => log.push('onfinish ' + anim.playState)
    anim.finished.then(() => log.push('finished'))
    log.push('running ' + anim.playState + ' ' + a.getAnimations().length + ' ' + document.getAnimations().length)
    const other = a.animate({ transform: ['none', 'scale(2)'] }, 1000)
    other.cancel()
    log.push('cancelled ' + other.playState)
  `)
  await web.wait(30)
  assert.deepEqual(web.json('log'), ['running running 1 1', 'cancelled idle', 'onfinish finished', 'finished'])
})

test('Web Animations run as CSS animations: @keyframes rules and the inline animation lists', async () => {
  using web = await page('<!doctype html><html><head></head><body><div id="a" style="color: red; animation: spin 1s"></div></body></html>')
  const result = web.json<{ style: string; before: string; rules: string[] }>(`(() => {
    const a = document.getElementById('a')
    a.animate([{ transform: 'scale(.8)', boxShadow: '0 0 1px red', marginBottom: '2px' }, { transform: 'scale(1.1)', offset: .5, easing: 'ease-in' }, { transform: 'none' }],
      { duration: 420, delay: 20, easing: 'cubic-bezier(.32,1.36,.48,1)', fill: 'both', iterations: Infinity, direction: 'alternate' })
    const before = a.getAttribute('style')
    a.animate({ opacity: [0, 1] }, 300)
    const sheet = document.querySelector('style[data-acde-animations]').sheet
    return { style: a.getAttribute('style'), before, rules: [...sheet.cssRules].map((r) => r.cssText.replace(/\\s+/g, ' ')) }
  })()`)
  const declared = Object.fromEntries(result.style.split(';').filter((d) => d.trim()).map((d) => d.split(/:(.*)/s).map((s) => s.trim()) as [string, string]))
  const names = declared['animation-name']!.split(', ')
  assert.equal(names.length, 2)
  const kept = (key: string) => new RegExp(`${key}: ([^,;]*)`).exec(result.before)![1]
  assert.equal(names[0], kept('animation-name'), 'the running animation keeps its name')
  assert.equal(declared['animation-delay']!.split(', ')[0], kept('animation-delay'), 'and its delay')
  assert.equal(result.rules.length, 2)
  assert.equal(declared['color'], 'red')
  assert.equal(declared['animation'], undefined)
  assert.equal(declared['animation-duration'], '420ms, 300ms')
  assert.equal(declared['animation-timing-function'], 'cubic-bezier(.32,1.36,.48,1), linear')
  assert.equal(declared['animation-iteration-count'], 'infinite, 1')
  assert.equal(declared['animation-direction'], 'alternate, normal')
  assert.equal(declared['animation-fill-mode'], 'both, none')
  assert.equal(declared['animation-play-state'], 'running, running')
  const [first, second] = declared['animation-delay']!.split(', ').map(parseFloat)
  assert.ok(first! > 0 && first! <= 20 && second! <= 0 && second! > -20, declared['animation-delay'] ?? '')
  const rule = (name: string) => result.rules.find((r) => r.startsWith(`@keyframes ${name} `))
  assert.equal(rule(names[0]!), `@keyframes ${names[0]} { 0% { transform: scale(.8); box-shadow: 0 0 1px red; margin-bottom: 2px; } 50% { transform: scale(1.1); animation-timing-function: ease-in; } 100% { transform: none; } }`)
  assert.equal(rule(names[1]!), `@keyframes ${names[1]} { 0% { opacity: 0; } 100% { opacity: 1; } }`)
})

test('Web Animations lifecycle on the inline style: pause, finish, fill, commitStyles, cancel, reverse, rule cleanup', async () => {
  using web = await page('<!doctype html><html><head></head><body><div id="a" style="color: red; animation: spin 1s"></div><div id="b"></div></body></html>')
  web.run(`
    globalThis.log = []
    globalThis.$ = (id) => document.getElementById(id)
    globalThis.css = (el, key) => (el.getAttribute('style') ?? '').split(';').map((d) => d.split(/:(.*)/s).map((s) => s.trim())).find(([k]) => k === key)?.[1] ?? null
    globalThis.rules = () => [...document.querySelector('style[data-acde-animations]').sheet.cssRules].map((r) => r.cssText.split(' ')[1])
    const anim = $('a').animate([{ opacity: 0 }, { opacity: 1, transform: 'translateX(4px)' }], { duration: 10, fill: 'forwards' })
    const started = css($('a'), 'animation-name')
    anim.pause()
    log.push(css($('a'), 'animation-play-state'), css($('a'), 'animation-name') !== started)
    anim.play()
    log.push(css($('a'), 'animation-play-state'))
    anim.onfinish = () => log.push('onfinish ' + anim.playState + ' ' + css($('a'), 'animation-fill-mode'))
    anim.finished.then(() => log.push('finished'))
    globalThis.anim = anim
    const plain = $('b').animate([{ opacity: 0 }, { opacity: 1 }], 10)
    plain.onfinish = () => log.push('plain ' + css($('b'), 'animation-name'))
  `)
  await web.wait(40)
  const committed = web.json(`(() => {
    anim.commitStyles()
    const out = [css($('a'), 'opacity'), css($('a'), 'transform'), css($('a'), 'animation-name') !== null]
    anim.cancel()
    out.push(css($('a'), 'animation-name'), css($('a'), 'animation'), css($('a'), 'color'), css($('a'), 'opacity'))
    return out
  })()`)
  const reversed = web.json<unknown[]>(`(() => {
    const r = $('b').animate([{ opacity: 0 }, { opacity: 1 }], { duration: 1000, fill: 'forwards' })
    r.currentTime = 400
    r.reverse()
    const t = r.currentTime
    const out = [Math.abs(t - 400) < 10, r.playbackRate, css($('b'), 'animation-direction'), css($('b'), 'animation-fill-mode'), parseFloat(css($('b'), 'animation-delay'))]
    r.finish()
    out.push(r.playState, r.currentTime, css($('b'), 'animation-delay'), css($('b'), 'animation-fill-mode'))
    r.cancel()
    out.push(css($('b'), 'animation-name'), rules().length)
    const f = $('b').animate([{ opacity: 0 }, { opacity: 1 }], { duration: 500, fill: 'forwards' })
    f.finish()
    out.push(f.playState, css($('b'), 'animation-delay'), css($('b'), 'animation-fill-mode'))
    f.cancel()
    return out
  })()`)
  await web.wait(1100)
  const cleaned = web.json('rules()')
  const log = web.json<unknown[]>('log')
  assert.deepEqual(log.slice(0, 3), ['paused', true, 'running'])
  assert.deepEqual(log.slice(3).filter((l) => l !== 'plain null'), ['onfinish finished forwards', 'finished'])
  assert.ok(log.includes('plain null'))
  assert.deepEqual(committed, ['1', 'translateX(4px)', true, null, 'spin 1s', 'red', '1'])
  assert.equal(reversed[0], true)
  assert.deepEqual(reversed.slice(1, 4), [-1, 'reverse', 'backwards'])
  assert.ok((reversed[4] as number) <= -590 && (reversed[4] as number) > -610)
  assert.deepEqual(reversed.slice(5, 9), ['finished', 0, null, null])
  assert.equal(reversed[9], null)
  assert.ok((reversed[10] as number) > 0, 'rules are dropped later, not one by one')
  assert.deepEqual(reversed.slice(11), ['finished', '-500ms', 'forwards'])
  assert.deepEqual(cleaned, [])
})

test('CSS animation and transition events: asked for once a page listens, fired on the element, the runtime\'s own keyframes left out', async () => {
  using web = await page('<!doctype html><html><body><div id="a"></div><p id="t"></p></body></html>')
  const id = (sel: string) => web.internals.idOf(web.run(`document.querySelector('${sel}')`))
  const record = (type: string, target: number, name: string, elapsedTime: number, pseudoElement = '') =>
    ({ type, transition: type.startsWith('transition'), target, name, elapsedTime, pseudoElement })
  web.frame()
  const asked = web.dom.calls.get('takeAnimationEvents') ?? 0
  assert.equal(asked, 0, 'a page without such listeners never asks')
  const before = web.host.frameRequests
  web.run(`
    globalThis.log = []
    document.body.addEventListener('animationend', (e) => log.push([e.type, e.target.id, e.animationName, e.elapsedTime, e.pseudoElement, e.bubbles, e.cancelable, e.isTrusted, e.constructor.name].join(' ')))
    document.getElementById('t').ontransitionend = (e) => log.push([e.type, e.propertyName, e.elapsedTime, e.constructor.name].join(' '))
  `)
  assert.ok(web.host.frameRequests > before, 'the first listener asks for a frame')
  web.dom.animationEvents.push(
    record('animationend', id('#a'), 'fade', 0.15, '::before'),
    record('animationend', id('#a'), 'acde-animation-3', 0.2),
    record('transitionend', id('#t'), 'opacity', 0.3),
  )
  web.frame()
  assert.deepEqual(web.json('log'), [
    'animationend a fade 0.15 ::before true false true AnimationEvent',
    'transitionend opacity 0.3 TransitionEvent',
  ])
})
