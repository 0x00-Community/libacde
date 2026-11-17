import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { DevRoutes, type DevRoute } from '../src/devroutes.ts'
import { SourceMaps } from '../src/devlog.ts'
import { DevToolsPage } from '../src/devtools/index.ts'
import { CssDomain } from '../src/devtools/css.ts'
import { DomDomain, type DomNode, type DomRecord } from '../src/devtools/dom.ts'
import { NO_NETWORK } from '@dopame/acde-dom'
import { NetworkDomain } from '../src/devtools/network.ts'
import { PageDomain } from '../src/devtools/page.ts'

const route = (args: Record<string, unknown>, id = 1): DevRoute => DevRoutes.parse(args, id)
const text = (reply: unknown) => {
  assert.ok(reply && typeof reply === 'object' && 'body' in reply)
  return new TextDecoder().decode((reply as { body: Uint8Array }).body)
}

describe('DevRoutes', () => {
  test('glob: ** crosses /, * stays within a segment; a leading / matches path and query, others the whole URL', () => {
    const match = (url: string, href: string) => DevRoutes.matcher(url)(href)
    assert.equal(match('/api/*', 'http://acde.local/api/todos'), true)
    assert.equal(match('/api/*', 'http://acde.local/api/todos?page=2'), true)
    assert.equal(match('/api/*', 'http://acde.local/api/todos/7'), false)
    assert.equal(match('/api/**', 'http://acde.local/api/todos/7'), true)
    assert.equal(match('/api/todos', 'http://acde.local/api/todos?x'), false)
    assert.equal(match('https://example.com/**', 'https://example.com/a/b?c'), true)
    assert.equal(match('https://example.com/**', 'http://example.com/a'), false)
    assert.equal(match('**/v1/(items)', 'https://x.test/v1/(items)'), true)
    assert.equal(DevRoutes.matcher({ regex: 'todos$', flags: 'gi' })('http://acde.local/api/TODOS'), true)
  })

  test('parse fills defaults, lower-cases header names and refuses what would not make a response', () => {
    const r = route({ url: '/x', status: 404, headers: { 'Content-Type': 'text/plain' }, body: btoa('missing'), method: 'post', times: 2 })
    assert.deepEqual({ ...r, url: r.url }, {
      id: 1, url: '/x', method: 'POST', web: null, times: 2, status: 404, statusText: 'Not Found',
      headers: [['content-type', 'text/plain']], body: btoa('missing'), abort: null,
    })
    assert.equal(route({ url: '/x' }).status, 200)
    assert.equal(route({ url: '/x', abort: true }).abort, 'aborted')
    const bad = (args: Record<string, unknown>, message: RegExp) => assert.throws(() => route(args), message)
    bad({}, /url is a glob/)
    bad({ url: { regex: '(' } }, /url: /)
    bad({ url: '/x', status: 101 }, /status is a whole number from 200 to 599/)
    bad({ url: '/x', times: 0 }, /times is a whole number above 0/)
    bad({ url: '/x', body: 'not base64!' }, /body is base64/)
    bad({ url: '/x', headers: { 'bad name': 'v' } }, /is not a token/)
    bad({ url: '/x', abort: true, status: 500 }, /abort cannot come with status/)
    bad({ url: '/x', method: 'GET /' }, /method is an HTTP method/)
  })

  test('match: the newest matching route answers; method filters; times run out and report each use', () => {
    const used: number[] = []
    const routes = new DevRoutes(
      [
        route({ url: '/api/**', body: btoa('old') }, 1),
        route({ url: '/api/todos', body: btoa('new'), times: 2 }, 2),
        route({ url: '/api/todos', method: 'DELETE', abort: 'offline' }, 3),
      ],
      (id) => used.push(id),
    )
    assert.equal(text(routes.match('GET', 'http://acde.local/api/todos')), 'new')
    assert.deepEqual(routes.match('delete', 'http://acde.local/api/todos'), { error: 'offline' })
    assert.equal(text(routes.match('GET', 'http://acde.local/api/todos')), 'new')
    assert.equal(text(routes.match('GET', 'http://acde.local/api/todos')), 'old')
    assert.deepEqual(used, [2, 2])
    assert.equal(routes.match('GET', 'http://acde.local/other'), null)
    routes.set([])
    assert.equal(routes.match('GET', 'http://acde.local/api/todos'), null)
  })
})

describe('SourceMaps', () => {
  const encode = (text: string) => new TextEncoder().encode(text)
  const files: Record<string, string> = {
    'http://acde.local/assets/main.js': 'function f(){throw new Error("x")}\n//# sourceMappingURL=main.js.map\n',
    'http://acde.local/assets/main.js.map': JSON.stringify({ version: 3, sources: ['../src/main.ts'], mappings: 'UAEI' }),
    'http://acde.local/assets/plain.js': 'function g(){}\n',
  }
  const read = (url: string) => (url in files ? encode(files[url]!) : null)
  const maps = new SourceMaps((url) => (url.endsWith('.map') ? null : read(url)), (url) => (url.endsWith('.map') ? read(url) : null))

  test('stack frames in files with a map point at the source', () => {
    const stack = 'Error: x\n    at f (http://acde.local/assets/main.js:1:12)\n    at http://acde.local/assets/plain.js:1:3'
    assert.equal(maps.stack(stack), 'Error: x\n    at f (src/main.ts:3:5)\n    at http://acde.local/assets/plain.js:1:3')
  })

  test('text without frames, or frames before any mapping, is kept', () => {
    assert.equal(maps.stack('just text'), 'just text')
    assert.equal(maps.stack('at http://acde.local/assets/main.js:1:3'), 'at http://acde.local/assets/main.js:1:3')
  })
})

describe('DevTools domains served by the page', () => {
  const devtools = () => {
    const sent: [number, Record<string, unknown>][] = []
    let handler: ((session: number, text: string | null) => void) | null = null
    const host = {
      devtoolsHandler: (fn: (session: number, text: string | null) => void) => ((handler = fn), 'alpha#1'),
      devtoolsSend: (session: number, text: string) => void sent.push([session, JSON.parse(text) as Record<string, unknown>]),
    }
    const page = DevToolsPage.install(host, (page) => [PageDomain.create(page.frameId, () => 'http://app.acde/index.html')])
    const ask = async (session: number, message: Record<string, unknown>) => {
      handler!(session, JSON.stringify(message))
      await new Promise((resolve) => setTimeout(resolve, 0))
    }
    return { sent, page: page!, ask, end: (session: number) => handler!(session, null) }
  }

  test('Page answers the frame tree with the web name as the frame id', async () => {
    const { sent, ask } = devtools()
    await ask(4, { id: 1, method: 'Page.enable' })
    await ask(4, { id: 2, method: 'Page.getFrameTree', sessionId: 's' })
    await ask(4, { id: 3, method: 'Page.getResourceTree' })
    assert.deepEqual(sent[0], [4, { id: 1, result: {} }])
    const [, tree] = sent[1]!
    assert.equal(tree['sessionId'], 's')
    const frame = (tree['result'] as { frameTree: { frame: Record<string, unknown> } }).frameTree.frame
    assert.deepEqual([frame['id'], frame['url'], frame['securityOrigin']], ['alpha#1', 'http://app.acde/index.html', 'http://app.acde'])
    assert.deepEqual((sent[2]![1]['result'] as { frameTree: { resources: unknown[] } }).frameTree.resources, [])
  })

  test('an unknown method answers -32601 and the session keeps working', async () => {
    const { sent, ask } = devtools()
    await ask(1, { id: 1, method: 'Page.reload' })
    await ask(1, { id: 2, method: 'DOM.getDocument' })
    await ask(1, { id: 3, method: 'Page.getFrameTree' })
    assert.deepEqual(sent.map(([, m]) => (m['error'] as { code?: number } | undefined)?.code ?? 'ok'), [-32601, -32601, 'ok'])
  })

  test('events reach only the sessions that enabled the domain, and an ended session is forgotten', async () => {
    const { sent, page, ask, end } = devtools()
    await ask(1, { id: 1, method: 'Page.enable' })
    await ask(2, { id: 1, method: 'Page.getFrameTree' })
    await ask(3, { id: 1, method: 'Page.enable' })
    end(3)
    sent.length = 0
    page.emit('Page', 'frameNavigated', { frame: { id: 'alpha#1' } })
    assert.deepEqual(sent, [[1, { method: 'Page.frameNavigated', params: { frame: { id: 'alpha#1' } } }]])
  })

  test('without the host side nothing is installed', () => {
    assert.equal(DevToolsPage.install({}, () => []), null)
    assert.equal(DevToolsPage.install({ devtoolsHandler: () => undefined, devtoolsSend: () => {} }, () => []), null)
  })
})

class FakeNode implements DomNode {
  static #next = 1
  readonly id = FakeNode.#next++
  readonly childNodes: FakeNode[] = []
  parentNode: FakeNode | null = null
  attributes: { name: string; value: string }[] | null

  constructor(
    readonly nodeType: number,
    readonly nodeName: string,
    public nodeValue: string | null = null,
    attributes: Record<string, string> | null = nodeType === 1 ? {} : null,
  ) {
    this.attributes = attributes && Object.entries(attributes).map(([name, value]) => ({ name, value }))
  }

  get localName(): string {
    return this.nodeName.toLowerCase()
  }

  get previousSibling(): FakeNode | null {
    const siblings = this.parentNode?.childNodes ?? []
    return siblings[siblings.indexOf(this) - 1] ?? null
  }

  append(...nodes: FakeNode[]): this {
    for (const node of nodes) {
      node.parentNode = this
      this.childNodes.push(node)
    }
    return this
  }
}

describe('DOM domain served by the page', () => {
  const tree = () => {
    const title = new FakeNode(1, 'H1', null, { class: 'big' }).append(new FakeNode(3, '#text', 'Hello'))
    const body = new FakeNode(1, 'BODY').append(new FakeNode(3, '#text', '\n  '), title)
    const html = new FakeNode(1, 'HTML', null, { lang: 'en' }).append(new FakeNode(1, 'HEAD'), body)
    const document = new FakeNode(9, '#document').append(new FakeNode(10, 'html'), html)
    const nodes = new Map<number, FakeNode>()
    const index = (node: FakeNode) => {
      nodes.set(node.id, node)
      node.childNodes.forEach(index)
    }
    index(document)
    let changed: ((records: readonly DomRecord[]) => void) | null = null
    const box: [string, string][] = [
      ['display', 'block'],
      ...['top', 'right', 'bottom', 'left'].flatMap((side): [string, string][] => [
        [`margin-${side}`, '8px'],
        [`border-${side}-width`, '1px'],
        [`padding-${side}`, '4px'],
      ]),
    ]
    const styles = new Map<FakeNode, [string, string][]>([[title, [['color', 'rgb(255, 0, 0)'], ...box]]])
    const rects = new Map<FakeNode, [number, number, number, number]>([[title, [10, 20, 100, 50]]])
    let lookup: ((id: number) => DomNode) | null = null
    const source = {
      document: () => document,
      idOf: (node: DomNode) => (node as FakeNode).id,
      nodeOf: (id: number) => nodes.get(id) ?? null,
      observe: (fn: (records: readonly DomRecord[]) => void) => ((changed = fn), () => void (changed = null)),
      expose: (fn: (id: number) => DomNode) => void (lookup = fn),
      computed: (node: DomNode) => styles.get(node as FakeNode) ?? [],
      inline: (node: DomNode): [string, string, boolean][] => (node === title ? [['color', 'red', true]] : []),
      rect: (node: DomNode) => rects.get(node as FakeNode) ?? null,
      query: (root: DomNode, selector: string) => {
        if (/[[\]]/.test(selector)) throw new SyntaxError(`bad selector ${selector}`)
        const match = (node: FakeNode) => {
          const attribute = (name: string) => node.attributes?.find((a) => a.name === name)?.value ?? ''
          if (selector.startsWith('.')) return attribute('class').split(' ').includes(selector.slice(1))
          if (selector.startsWith('#')) return attribute('id') === selector.slice(1)
          return /^[a-z][a-z0-9]*$/i.test(selector) && node.nodeType === 1 && node.localName === selector.toLowerCase()
        }
        const out: FakeNode[] = []
        const walk = (node: FakeNode) => node.childNodes.forEach((child) => (match(child) && out.push(child), walk(child)))
        walk(root as FakeNode)
        return out
      },
    }
    const sent: Record<string, unknown>[] = []
    const to = new WeakMap<object, number>()
    let handler: ((session: number, text: string | null) => void) | null = null
    const host = {
      devtoolsHandler: (fn: (session: number, text: string | null) => void) => ((handler = fn), 'alpha#1'),
      devtoolsSend: (session: number, text: string) => {
        const message = JSON.parse(text) as Record<string, unknown>
        to.set(message, session)
        sent.push(message)
      },
    }
    const dom = (page: DevToolsPage) => DomDomain.create(source, (session, method, params) => page.emitTo(session, 'DOM', method, params))
    DevToolsPage.install(host, (page) => [dom(page), CssDomain.create(source)])
    const ask = async (message: Record<string, unknown>, session = 1) => {
      handler!(session, JSON.stringify(message))
      await new Promise((resolve) => setTimeout(resolve, 0))
      return sent.at(-1)!
    }
    const mutate = (records: DomRecord[]) => changed!(records)
    const end = (session: number) => handler!(session, null)
    const events = (session: number) => sent.filter((m) => to.get(m) === session && typeof m['method'] === 'string')
    return { document, html, body, title, sent, events, ask, end, mutate, styles, lookup: () => lookup, observing: () => changed !== null }
  }
  type Described = { nodeId: number; nodeName: string; attributes?: string[]; childNodeCount: number; children?: Described[]; nodeValue: string }

  test('getDocument gives the tree with node ids, attributes and text; blank text is left out', async () => {
    const { document, html, body, title, ask, observing, lookup } = tree()
    const reply = await ask({ id: 1, method: 'DOM.getDocument', params: { depth: -1 } })
    const root = (reply['result'] as { root: Described }).root
    assert.equal(root.nodeId, document.id)
    assert.deepEqual(
      root.children!.map((c) => c.nodeName),
      ['html', 'HTML'],
    )
    const htmlNode = root.children![1]!
    assert.deepEqual([htmlNode.nodeId, htmlNode.attributes], [html.id, ['lang', 'en']])
    const bodyNode = htmlNode.children![1]!
    assert.deepEqual([bodyNode.nodeId, bodyNode.childNodeCount], [body.id, 1])
    const h1 = bodyNode.children![0]!
    assert.deepEqual([h1.nodeId, h1.attributes, h1.children![0]!.nodeValue], [title.id, ['class', 'big'], 'Hello'])
    assert.ok(observing(), 'getDocument starts watching the page')
    assert.equal(lookup()!(title.id), title)
  })

  test('requestChildNodes and describeNode answer from the page', async () => {
    const { html, body, sent, ask } = tree()
    await ask({ id: 1, method: 'DOM.getDocument' })
    await ask({ id: 2, method: 'DOM.requestChildNodes', params: { nodeId: body.id } })
    const event = sent.find((m) => m['method'] === 'DOM.setChildNodes')!
    assert.equal((event['params'] as { parentId: number }).parentId, body.id)
    const described = await ask({ id: 3, method: 'DOM.describeNode', params: { backendNodeId: html.id } })
    assert.equal((described['result'] as { node: Described }).node.nodeName, 'HTML')
    const missing = await ask({ id: 4, method: 'DOM.describeNode', params: { nodeId: 9999 } })
    assert.equal((missing['error'] as { code: number }).code, -32000)
  })

  test('querySelector and querySelectorAll answer render ids and push the path to unseen nodes', async () => {
    const { document, html, body, title, sent, ask } = tree()
    await ask({ id: 1, method: 'DOM.enable' })
    sent.length = 0
    const one = await ask({ id: 2, method: 'DOM.querySelector', params: { nodeId: document.id, selector: 'h1' } })
    assert.deepEqual(one['result'], { nodeId: title.id })
    const pushed = sent.filter((m) => m['method'] === 'DOM.setChildNodes').map((m) => (m['params'] as { parentId: number }).parentId)
    assert.deepEqual(pushed, [document.id, html.id, body.id])
    sent.length = 0
    const all = await ask({ id: 3, method: 'DOM.querySelectorAll', params: { nodeId: body.id, selector: '.big' } })
    assert.deepEqual(all['result'], { nodeIds: [title.id] })
    assert.equal(sent.filter((m) => m['method'] === 'DOM.setChildNodes').length, 0, 'the path is known by now')
    const none = await ask({ id: 4, method: 'DOM.querySelector', params: { nodeId: document.id, selector: 'p' } })
    assert.deepEqual(none['result'], { nodeId: 0 })
    const bad = await ask({ id: 5, method: 'DOM.querySelectorAll', params: { nodeId: document.id, selector: 'a[[' } })
    assert.deepEqual(bad['error'], { code: -32000, message: "'a[[' is not a valid selector." })
  })

  test('search finds by text, tag name and selector, per session, until discarded or disconnected', async () => {
    const { html, title, ask, end } = tree()
    const search = async (query: string, session = 1) => {
      const reply = await ask({ id: 1, method: 'DOM.performSearch', params: { query } }, session)
      return reply['result'] as { searchId: string; resultCount: number }
    }
    const results = async (searchId: string, count: number, session = 1) =>
      (await ask({ id: 2, method: 'DOM.getSearchResults', params: { searchId, fromIndex: 0, toIndex: count } }, session))
    const text = await search('hello')
    const textNode = title.childNodes[0]!
    assert.deepEqual((await results(text.searchId, text.resultCount))['result'], { nodeIds: [textNode.id] })
    const tag = await search('<h1>')
    assert.deepEqual((await results(tag.searchId, tag.resultCount))['result'], { nodeIds: [title.id] })
    const prefix = await search('<h')
    assert.equal(prefix.resultCount, 3, 'html, head and h1')
    const selector = await search('.big')
    assert.deepEqual((await results(selector.searchId, selector.resultCount))['result'], { nodeIds: [title.id] })
    const attribute = await search('lang')
    assert.deepEqual((await results(attribute.searchId, 1))['result'], { nodeIds: [html.id] })
    assert.equal((await search('nothing here')).resultCount, 0)
    const range = await ask({ id: 3, method: 'DOM.getSearchResults', params: { searchId: tag.searchId, fromIndex: 0, toIndex: 5 } })
    assert.equal((range['error'] as { code: number }).code, -32000)
    const other = await results(tag.searchId, 1, 2)
    assert.equal((other['error'] as { message: string }).message, 'No search session with given id found')
    await ask({ id: 4, method: 'DOM.discardSearchResults', params: { searchId: tag.searchId } })
    assert.equal(((await results(tag.searchId, 1))['error'] as { code: number }).code, -32000)
    end(1)
    assert.equal(((await results(text.searchId, 1))['error'] as { code: number }).code, -32000, 'dropped with the session')
  })

  test('a search keeps at most 1000 results', async () => {
    const { body, ask } = tree()
    body.append(...Array.from({ length: 1200 }, () => new FakeNode(1, 'P')))
    const reply = await ask({ id: 1, method: 'DOM.performSearch', params: { query: '<p>' } })
    const { searchId, resultCount } = reply['result'] as { searchId: string; resultCount: number }
    assert.equal(resultCount, 1000)
    const last = await ask({ id: 2, method: 'DOM.getSearchResults', params: { searchId, fromIndex: 999, toIndex: 1000 } })
    assert.equal((last['result'] as { nodeIds: number[] }).nodeIds.length, 1)
    const over = await ask({ id: 3, method: 'DOM.getSearchResults', params: { searchId, fromIndex: 0, toIndex: 1001 } })
    assert.equal((over['error'] as { code: number }).code, -32000)
  })

  test('each session keeps its own expanded nodes and gets only the events for them', async () => {
    const { document, html, body, title, sent, events, ask, end, mutate, observing } = tree()
    await ask({ id: 1, method: 'DOM.getDocument', params: { depth: -1 } }, 1)
    await ask({ id: 1, method: 'DOM.getDocument', params: { depth: 1 } }, 2)
    sent.length = 0
    await ask({ id: 2, method: 'DOM.getDocument', params: { depth: 1 } }, 2)
    const added = new FakeNode(1, 'P')
    body.append(added)
    mutate([{ type: 'childList', target: body, addedNodes: [added], removedNodes: [], attributeName: null }])
    title.attributes = [{ name: 'class', value: 'small' }]
    mutate([{ type: 'attributes', target: title, addedNodes: [], removedNodes: [], attributeName: 'class' }])
    assert.deepEqual(
      events(1).map((m) => m['method']),
      ['DOM.childNodeInserted', 'DOM.attributeModified'],
      'getDocument of session 2 did not forget what session 1 expanded',
    )
    assert.deepEqual(events(2), [], 'session 2 never expanded html, so body and the h1 are unknown to it')
    await ask({ id: 3, method: 'DOM.requestChildNodes', params: { nodeId: html.id } }, 2)
    const pushed = events(2).filter((m) => m['method'] === 'DOM.setChildNodes')
    assert.deepEqual(pushed.map((m) => (m['params'] as { parentId: number }).parentId), [html.id])
    assert.equal(events(1).filter((m) => m['method'] === 'DOM.setChildNodes').length, 0, 'session 1 is not told of it')
    sent.length = 0
    const p = new FakeNode(1, 'P')
    body.append(p)
    mutate([{ type: 'childList', target: body, addedNodes: [p], removedNodes: [], attributeName: null }])
    assert.deepEqual(events(1).map((m) => m['method']), ['DOM.childNodeInserted'])
    assert.deepEqual(events(2).map((m) => m['params']), [{ nodeId: body.id, childNodeCount: 3 }])
    sent.length = 0
    await ask({ id: 4, method: 'DOM.getDocument', params: { depth: 1 } }, 1)
    const late = await ask({ id: 5, method: 'DOM.querySelector', params: { nodeId: document.id, selector: 'h1' } }, 1)
    assert.deepEqual(late['result'], { nodeId: title.id })
    assert.deepEqual(
      events(1).filter((m) => m['method'] === 'DOM.setChildNodes').map((m) => (m['params'] as { parentId: number }).parentId),
      [html.id, body.id],
      'after its own getDocument session 1 is sent the path again',
    )
    end(1)
    end(2)
    assert.equal(observing(), false, 'no session left, nothing watched')
  })

  test('a removed node takes the expanded nodes below it out of the session', async () => {
    const { body, events, sent, ask, mutate } = tree()
    const inner = new FakeNode(1, 'P').append(new FakeNode(3, '#text', 'deep'))
    const section = new FakeNode(1, 'SECTION').append(inner)
    body.append(section)
    await ask({ id: 1, method: 'DOM.getDocument', params: { depth: -1 } })
    body.childNodes.splice(body.childNodes.indexOf(section), 1)
    section.parentNode = null
    mutate([{ type: 'childList', target: body, addedNodes: [], removedNodes: [section], attributeName: null }])
    body.append(section)
    mutate([{ type: 'childList', target: body, addedNodes: [section], removedNodes: [], attributeName: null }])
    sent.length = 0
    const more = new FakeNode(1, 'B')
    inner.append(more)
    mutate([{ type: 'childList', target: inner, addedNodes: [more], removedNodes: [], attributeName: null }])
    assert.deepEqual(events(1), [], 'the p inside the removed section is no longer counted as expanded')
  })

  test('page changes reach the open tree as DOM events', async () => {
    const { body, title, sent, ask, mutate } = tree()
    await ask({ id: 1, method: 'DOM.getDocument', params: { depth: -1 } })
    sent.length = 0
    const added = new FakeNode(1, 'P')
    body.append(added)
    mutate([{ type: 'childList', target: body, addedNodes: [added], removedNodes: [], attributeName: null }])
    title.attributes = [{ name: 'class', value: 'small' }]
    mutate([{ type: 'attributes', target: title, addedNodes: [], removedNodes: [], attributeName: 'class' }])
    const text = title.childNodes[0]!
    text.nodeValue = 'Bye'
    mutate([{ type: 'characterData', target: text, addedNodes: [], removedNodes: [], attributeName: null }])
    body.childNodes.splice(body.childNodes.indexOf(added), 1)
    mutate([{ type: 'childList', target: body, addedNodes: [], removedNodes: [added], attributeName: null }])
    assert.deepEqual(
      sent.map((m) => m['method']),
      ['DOM.childNodeInserted', 'DOM.attributeModified', 'DOM.characterDataModified', 'DOM.childNodeRemoved'],
    )
    const inserted = sent[0]!['params'] as { parentNodeId: number; previousNodeId: number; node: Described }
    assert.deepEqual([inserted.parentNodeId, inserted.previousNodeId, inserted.node.nodeId], [body.id, title.id, added.id])
    assert.deepEqual(sent[1]!['params'], { nodeId: title.id, name: 'class', value: 'small' })
    assert.deepEqual(sent[2]!['params'], { nodeId: text.id, characterData: 'Bye' })
    assert.deepEqual(sent[3]!['params'], { parentNodeId: body.id, nodeId: added.id })
  })

  test('editing is refused as read only, and disable stops watching', async () => {
    const { title, ask, observing } = tree()
    await ask({ id: 1, method: 'DOM.enable' })
    const edit = await ask({ id: 2, method: 'DOM.setAttributeValue', params: { nodeId: title.id, name: 'id', value: 'x' } })
    assert.match((edit['error'] as { message: string }).message, /read only/)
    assert.deepEqual(title.attributes, [{ name: 'class', value: 'big' }])
    await ask({ id: 3, method: 'DOM.disable' })
    assert.equal(observing(), false)
  })

  test('computed style and box model come from the page; matched rules are only the inline style', async () => {
    const { body, title, ask, styles } = tree()
    const computed = await ask({ id: 1, method: 'CSS.getComputedStyleForNode', params: { nodeId: title.id } })
    const values = (computed['result'] as { computedStyle: { name: string; value: string }[] }).computedStyle
    assert.deepEqual(values[0], { name: 'color', value: 'rgb(255, 0, 0)' })
    const box = await ask({ id: 2, method: 'DOM.getBoxModel', params: { nodeId: title.id } })
    assert.deepEqual(box['result'], {
      model: {
        content: [15, 25, 105, 25, 105, 65, 15, 65],
        padding: [11, 21, 109, 21, 109, 69, 11, 69],
        border: [10, 20, 110, 20, 110, 70, 10, 70],
        margin: [2, 12, 118, 12, 118, 78, 2, 78],
        width: 100,
        height: 50,
      },
    })
    const matched = await ask({ id: 3, method: 'CSS.getMatchedStylesForNode', params: { nodeId: title.id } })
    const result = matched['result'] as { inlineStyle: { cssProperties: unknown[]; cssText: string }; matchedCSSRules: unknown[] }
    assert.deepEqual(result.inlineStyle.cssProperties, [{ name: 'color', value: 'red', important: true, implicit: false, disabled: false }])
    assert.equal(result.inlineStyle.cssText, 'color: red !important;')
    assert.deepEqual(result.matchedCSSRules, [])
    styles.set(title, [['color', 'rgb(0, 0, 255)']])
    const again = await ask({ id: 4, method: 'CSS.getComputedStyleForNode', params: { nodeId: title.id } })
    assert.deepEqual((again['result'] as { computedStyle: unknown[] }).computedStyle, [{ name: 'color', value: 'rgb(0, 0, 255)' }])
    const unrendered = await ask({ id: 5, method: 'DOM.getBoxModel', params: { nodeId: body.id } })
    assert.equal((unrendered['error'] as { code: number }).code, -32000)
    const edit = await ask({ id: 6, method: 'CSS.setStyleTexts', params: { edits: [] } })
    assert.match((edit['error'] as { message: string }).message, /read only/)
  })
})

describe('DevTools Network domain', () => {
  const network = (limit = NetworkDomain.BODIES, clock = () => 1) => {
    const sent: Record<string, unknown>[] = []
    let handler: ((session: number, text: string | null) => void) | null = null
    let domain: NetworkDomain | null = null
    const host = {
      devtoolsHandler: (fn: (session: number, text: string | null) => void) => ((handler = fn), 'alpha#1'),
      devtoolsSend: (_: number, text: string) => void sent.push(JSON.parse(text) as Record<string, unknown>),
    }
    DevToolsPage.install(host, (page) => {
      domain = new NetworkDomain(page, () => 'http://app.acde/', limit, clock)
      return [domain.domain()]
    })
    const ask = async (message: Record<string, unknown>) => {
      handler!(1, JSON.stringify(message))
      await new Promise((resolve) => setTimeout(resolve, 0))
      return sent.find((m) => m['id'] === message['id'])
    }
    const events = (method: string) => sent.filter((m) => m['method'] === `Network.${method}`).map((m) => m['params'] as Record<string, unknown>)
    return { sent, ask, events, observer: domain!.observer }
  }
  const bytes = (text: string) => new TextEncoder().encode(text)

  test('nothing is recorded until a client enables Network', async () => {
    const { observer, ask, sent } = network()
    assert.equal(observer.recording(), false)
    await ask({ id: 1, method: 'Network.enable' })
    assert.equal(observer.recording(), true)
    await ask({ id: 2, method: 'Network.disable' })
    assert.equal(observer.recording(), false)
    assert.deepEqual(sent.map((m) => m['id']), [1, 2])
  })

  test('a fetch reports its request, response, data and end, and its text body is kept', async () => {
    const { observer, ask, events } = network()
    await ask({ id: 1, method: 'Network.enable' })
    observer.start(7, 'fetch', 'POST', 'http://app.acde/api', [['content-type', 'application/json']], bytes('{"a":1}'))
    observer.response(7, 201, 'Created', [['Content-Type', 'application/json; charset=utf-8'], ['x-a', '1']])
    observer.data(7, bytes('{"ok":'))
    observer.data(7, bytes('true}'))
    observer.end(7, 11)
    const [sent] = events('requestWillBeSent')
    assert.deepEqual([sent!['requestId'], sent!['type'], sent!['frameId'], sent!['loaderId']], ['7', 'Fetch', 'alpha#1', 'alpha#1'])
    assert.deepEqual(sent!['request'], {
      url: 'http://app.acde/api', method: 'POST', headers: { 'content-type': 'application/json' }, initialPriority: 'High',
      referrerPolicy: 'strict-origin-when-cross-origin', hasPostData: true, postData: '{"a":1}',
    })
    const response = events('responseReceived')[0]!['response'] as Record<string, unknown>
    const headers = { 'Content-Type': 'application/json; charset=utf-8', 'x-a': '1' }
    assert.deepEqual([response['status'], response['statusText'], response['mimeType'], response['headers']], [201, 'Created', 'application/json', headers])
    assert.equal(events('dataReceived').length, 2)
    assert.deepEqual(events('loadingFinished'), [{ requestId: '7', timestamp: 1, encodedDataLength: 11 }])
    const body = await ask({ id: 2, method: 'Network.getResponseBody', params: { requestId: '7' } })
    assert.deepEqual(body!['result'], { body: '{"ok":true}', base64Encoded: false })
  })

  test('a binary body comes back as base64, an XHR is typed XHR, and a failure carries its error text', async () => {
    const { observer, ask, events } = network()
    await ask({ id: 1, method: 'Network.enable' })
    observer.start(1, 'xhr', 'GET', 'http://app.acde/a.png', [], null)
    observer.response(1, 200, 'OK', [['content-type', 'image/png']])
    observer.data(1, new Uint8Array([0x89, 0x50, 0x4e, 0x47]))
    observer.end(1, 4)
    observer.start(2, 'fetch', 'GET', 'https://elsewhere.example/', [], null)
    observer.error(2, 'this app has no network capability')
    observer.start(3, 'fetch', 'GET', 'http://app.acde/slow', [], null)
    observer.error(3, 'aborted')
    assert.equal(events('requestWillBeSent')[0]!['type'], 'XHR')
    const image = await ask({ id: 2, method: 'Network.getResponseBody', params: { requestId: '1' } })
    assert.deepEqual(image!['result'], { body: 'iVBORw==', base64Encoded: true })
    assert.deepEqual(events('loadingFailed'), [
      { requestId: '2', timestamp: 1, type: 'Fetch', errorText: 'this app has no network capability' },
      { requestId: '3', timestamp: 1, type: 'Fetch', errorText: 'aborted', canceled: true },
    ])
    assert.equal(events('requestWillBeSent').length, 3)
  })

  test('kept bodies are bounded per page and the oldest go first', async () => {
    const { observer, ask } = network(10)
    await ask({ id: 1, method: 'Network.enable' })
    for (const id of [1, 2]) {
      observer.start(id, 'fetch', 'GET', `http://app.acde/${id}`, [], null)
      observer.response(id, 200, 'OK', [['content-type', 'text/plain']])
      observer.data(id, bytes('123456'))
      observer.end(id, 6)
    }
    assert.equal(((await ask({ id: 2, method: 'Network.getResponseBody', params: { requestId: '1' } }))!['error'] as { code: number }).code, -32000)
    assert.deepEqual((await ask({ id: 3, method: 'Network.getResponseBody', params: { requestId: '2' } }))!['result'], { body: '123456', base64Encoded: false })
    observer.start(3, 'fetch', 'GET', 'http://app.acde/big', [], null)
    observer.data(3, bytes('1234567'))
    observer.data(3, bytes('89ab'))
    observer.end(3, 11)
    assert.equal(((await ask({ id: 4, method: 'Network.getResponseBody', params: { requestId: '3' } }))!['error'] as { code: number }).code, -32000)
    assert.deepEqual((await ask({ id: 5, method: 'Network.getResponseBody', params: { requestId: '2' } }))!['result'], { body: '123456', base64Encoded: false })
  })

  test('a cache hit is marked fromMemoryCache, a fetched response is not', async () => {
    const { observer, ask, events } = network()
    await ask({ id: 1, method: 'Network.enable' })
    observer.start(1, 'image', 'GET', 'http://app.acde/pic.svg', [], null)
    observer.response(1, 200, 'OK', [['content-type', 'image/svg+xml']], true)
    observer.end(1, 0)
    observer.start(2, 'image', 'GET', 'http://app.acde/other.svg', [], null)
    observer.response(2, 200, 'OK', [])
    const cached = events('responseReceived').map((e) => (e['response'] as { fromMemoryCache: boolean }).fromMemoryCache)
    assert.deepEqual(cached, [true, false])
  })

  test('a response carries its timing: start time, send at 0, headers when they arrived, unknown phases -1', async () => {
    const times = [10, 10.25]
    const { observer, ask, events } = network(NetworkDomain.BODIES, () => times.shift() ?? 11)
    await ask({ id: 1, method: 'Network.enable' })
    observer.start(1, 'fetch', 'GET', 'http://app.acde/a', [], null)
    observer.response(1, 200, 'OK', [])
    const timing = (events('responseReceived')[0]!['response'] as { timing: Record<string, number> }).timing
    assert.deepEqual([timing['requestTime'], timing['sendStart'], timing['sendEnd'], timing['receiveHeadersEnd']], [10, 0, 0, 250])
    assert.deepEqual([timing['dnsStart'], timing['connectStart'], timing['sslStart'], timing['sslEnd']], [-1, -1, -1, -1])
  })

  test('a WebSocket shows its handshake, frames both ways and its close', async () => {
    const { observer, ask, events } = network()
    await ask({ id: 1, method: 'Network.enable' })
    observer.start(5, 'websocket', 'GET', 'ws://app.acde/echo', [['origin', 'http://app.acde']], null)
    observer.response(5, 101, 'Switching Protocols', [['upgrade', 'websocket']])
    observer.frame!(5, true, 'hello')
    observer.frame!(5, false, new Uint8Array([1, 2, 3]))
    observer.end(5, 3)
    assert.deepEqual(events('webSocketCreated'), [{ requestId: '5', url: 'ws://app.acde/echo', initiator: { type: 'script' } }])
    assert.deepEqual(events('webSocketWillSendHandshakeRequest')[0]!['request'], { headers: { origin: 'http://app.acde' } })
    assert.equal((events('webSocketHandshakeResponseReceived')[0]!['response'] as { status: number }).status, 101)
    assert.deepEqual(events('webSocketFrameSent')[0]!['response'], { opcode: 1, mask: true, payloadData: 'hello' })
    assert.deepEqual(events('webSocketFrameReceived')[0]!['response'], { opcode: 2, mask: false, payloadData: 'AQID' })
    assert.deepEqual(events('webSocketClosed'), [{ requestId: '5', timestamp: 1 }])
    assert.deepEqual([events('requestWillBeSent').length, events('loadingFinished').length], [0, 0])
  })

  test('an EventSource shows its messages, loads carry their resource type, and a refused request is blocked', async () => {
    const { observer, ask, events } = network()
    await ask({ id: 1, method: 'Network.enable' })
    observer.start(1, 'eventsource', 'GET', 'http://app.acde/events', [], null)
    observer.response(1, 200, 'OK', [['content-type', 'text/event-stream']])
    observer.message!(1, 'tick', 'one', '7')
    observer.end(1, 20)
    for (const [id, kind] of [[2, 'image'], [3, 'stylesheet'], [4, 'font']] as const) {
      observer.start(id, kind, 'GET', `http://app.acde/${kind}`, [], null)
      observer.response(id, 200, 'OK', [])
      observer.end(id, 0)
    }
    observer.start(5, 'fetch', 'GET', 'https://elsewhere.example/', [], null)
    observer.error(5, NO_NETWORK)
    assert.deepEqual(events('eventSourceMessageReceived'), [{ requestId: '1', timestamp: 1, eventName: 'tick', eventId: '7', data: 'one' }])
    assert.deepEqual(events('requestWillBeSent').map((e) => e['type']), ['EventSource', 'Image', 'Stylesheet', 'Font', 'Fetch'])
    assert.equal(((await ask({ id: 2, method: 'Network.getResponseBody', params: { requestId: '1' } }))!['error'] as { code: number }).code, -32000)
    assert.deepEqual(events('loadingFailed'), [{ requestId: '5', timestamp: 1, type: 'Fetch', errorText: NO_NETWORK, blockedReason: 'other' }])
  })

  test('a request in flight when Network is disabled reports nothing more', async () => {
    const { observer, ask, sent } = network()
    await ask({ id: 1, method: 'Network.enable' })
    observer.start(1, 'fetch', 'GET', 'http://app.acde/slow', [], null)
    await ask({ id: 2, method: 'Network.disable' })
    const before = sent.length
    observer.data(1, bytes('late'))
    observer.end(1, 4)
    await ask({ id: 3, method: 'Network.enable' })
    observer.end(1, 4)
    assert.equal(sent.length, before + 1, 'only the answer to the second enable')
  })
})
