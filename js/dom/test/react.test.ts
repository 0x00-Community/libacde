import assert from 'node:assert/strict'
import { test } from 'node:test'
import { loadReact, page, type Web } from './harness.ts'

const PAGE = '<!doctype html><html><head></head><body><div id="root"></div></body></html>'

async function reactWeb(): Promise<Web> {
  using web = await page(PAGE)
  loadReact(web)
  return web
}

test('React 19 createRoot renders, updates state from event handlers, unmounts', async () => {
  using web = await reactWeb()
  web.run(`
    const h = React.createElement
    globalThis.log = []
    function Counter() {
      const [n, setN] = React.useState(0)
      const [text, setText] = React.useState('')
      React.useEffect(() => { log.push('effect ' + n) }, [n])
      return h('div', { className: 'counter', 'data-n': n },
        h('button', { id: 'inc', onClick: () => setN((v) => v + 1) }, 'count ', n),
        h('input', { id: 'field', value: text, onChange: (e) => setText(e.target.value) }),
        h('p', { id: 'echo', style: { color: n > 1 ? 'red' : 'blue', marginTop: 4 } }, text),
        n % 2 === 1 ? h('span', { id: 'odd' }, 'odd') : null,
        h('svg', { viewBox: '0 0 10 10', id: 'icon' }, h('path', { d: 'M0 0L10 10', strokeWidth: 2 })))
    }
    globalThis.root = ReactDOMClient.createRoot(document.getElementById('root'))
    React.act(() => root.render(h(Counter)))
  `)
  assert.equal(web.run('document.querySelector("#inc").textContent'), 'count 0')
  assert.equal(web.run('document.querySelector(".counter").getAttribute("data-n")'), '0')
  assert.equal(web.run('document.getElementById("echo").getAttribute("style")'), 'color: blue; margin-top: 4px;')
  assert.equal(web.run('document.getElementById("icon").namespaceURI'), 'http://www.w3.org/2000/svg')
  assert.equal(web.run('document.querySelector("#icon path").getAttribute("stroke-width")'), '2')

  web.run(`React.act(() => document.getElementById('inc').dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true })))`)
  assert.equal(web.run('document.querySelector("#inc").textContent'), 'count 1')
  assert.equal(web.run('!!document.getElementById("odd")'), true)
  web.run(`React.act(() => document.getElementById('inc').click())`)
  assert.equal(web.run('document.querySelector("#inc").textContent'), 'count 2')
  assert.equal(web.run('document.getElementById("odd")'), null)
  assert.equal(web.run('document.getElementById("echo").style.color'), 'red')

  web.run(`
    const field = document.getElementById('field')
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set
    React.act(() => {
      setter.call(field, 'hello')
      field.dispatchEvent(new InputEvent('input', { bubbles: true, data: 'hello', inputType: 'insertText' }))
    })
  `)
  assert.equal(web.run('document.getElementById("echo").textContent'), 'hello')
  assert.equal(web.run('document.getElementById("field").value'), 'hello')
  assert.deepEqual(web.json('log'), ['effect 0', 'effect 1', 'effect 2'])

  web.run('React.act(() => root.unmount())')
  assert.equal(web.run('document.getElementById("root").childNodes.length'), 0)
  assert.deepEqual(web.host.errors, [])
})

test('React renders through its scheduler (MessageChannel) without act', async () => {
  using web = await reactWeb()
  web.run(`
    globalThis.IS_REACT_ACT_ENVIRONMENT = false
    const h = React.createElement
    function List({ items }) { return h('ul', null, items.map((t) => h('li', { key: t }, t))) }
    globalThis.root2 = ReactDOMClient.createRoot(document.getElementById('root'))
    root2.render(h(List, { items: ['a', 'b', 'c'] }))
  `)
  for (let i = 0; i < 20 && web.run<number>('document.querySelectorAll("li").length') === 0; i++) await web.wait(5)
  assert.equal(web.run('[...document.querySelectorAll("li")].map((li) => li.textContent).join()'), 'a,b,c')
  web.run(`root2.render(React.createElement(function List({ items }) { return React.createElement('ul', null, items.map((t) => React.createElement('li', { key: t }, t))) }, { items: ['c', 'a'] }))`)
  for (let i = 0; i < 20 && web.run<number>('document.querySelectorAll("li").length') !== 2; i++) await web.wait(5)
  assert.equal(web.run('[...document.querySelectorAll("li")].map((li) => li.textContent).join()'), 'c,a')
  assert.deepEqual(web.host.errors, [])
})

test('React portals, refs and many nodes', async () => {
  using web = await reactWeb()
  web.run(`
    const h = React.createElement
    globalThis.refs = {}
    function App() {
      const ref = React.useRef(null)
      React.useLayoutEffect(() => { refs.box = ref.current })
      return h(React.Fragment, null,
        h('section', { ref }, Array.from({ length: 2000 }, (_, i) => h('i', { key: i, className: 'c' + (i % 7) }, i))),
        ReactDOM.createPortal(h('aside', { id: 'portal' }, 'over'), document.body))
    }
    globalThis.root3 = ReactDOMClient.createRoot(document.getElementById('root'))
    React.act(() => root3.render(h(App)))
  `)
  assert.equal(web.run('document.querySelectorAll("section i").length'), 2000)
  assert.equal(web.run('refs.box === document.querySelector("section")'), true)
  assert.equal(web.run('document.getElementById("portal").parentNode === document.body'), true)
  assert.equal(web.run('Object.keys(document.querySelector("section")).some((k) => k.startsWith("__reactFiber$"))'), true)
  assert.deepEqual(web.host.errors, [])
})

test('React controlled input: typed text undoes and redoes, and the state follows', async () => {
  using web = await reactWeb()
  web.run(`
    const h = React.createElement
    function Field() {
      const [text, setText] = React.useState('')
      return h('div', null, h('input', { id: 'field', value: text, onChange: (e) => setText(e.target.value) }), h('p', { id: 'echo' }, text))
    }
    globalThis.root = ReactDOMClient.createRoot(document.getElementById('root'))
    React.act(() => root.render(h(Field)))
  `)
  const field = web.run<never>(`document.getElementById('field')`)
  const key = (k: string, ctrl = false, shift = false) => web.internals.editing.editKey(field, k, ctrl, shift)
  const state = () => web.json(`[document.getElementById('field').value, document.getElementById('echo').textContent]`)
  for (const c of 'hi there') key(c)
  await web.wait(20)
  assert.deepEqual(state(), ['hi there', 'hi there'])
  key('z', true)
  await web.wait(20)
  assert.deepEqual(state(), ['', ''])
  key('z', true, true)
  await web.wait(20)
  assert.deepEqual(state(), ['hi there', 'hi there'])
  web.run('React.act(() => root.unmount())')
  assert.deepEqual(web.host.errors, [])
})
