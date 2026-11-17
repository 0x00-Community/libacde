import { Event, fire, type EventTarget } from '../core/event'
import { G, host, report } from '../core/host'
import { scriptSource } from '../core/net'
import { URL } from '../core/url'
import { noteMarkup, parsed, type Element } from './element'
import type { Node } from './node'
import { env, resolveUrl } from './env'
import { documentState } from './document'
import { HTMLScriptElement } from './html'
import { detach, documentId, dom, ID, lifecycle, roots, strongIds, treeChanged, wrap, type Wrapper } from './realm'

const STARTED = Symbol('started')
type Script = HTMLScriptElement & { [STARTED]?: boolean }

const JS_TYPE = /^(text|application)\/(x-)?(java|ecma)script$/i

function kind(script: Element): 'classic' | 'module' | 'importmap' | null {
  const type = dom.getAttribute(script[ID], 'type')
  if (type === null || type === '' || JS_TYPE.test(type.trim())) return dom.hasAttribute(script[ID], 'nomodule') ? null : 'classic'
  const lower = type.trim().toLowerCase()
  return lower === 'module' ? 'module' : lower === 'importmap' ? 'importmap' : null
}

function specifierMap(value: unknown, base: string): [string, string][] {
  if (!value || typeof value !== 'object') return []
  const out: [string, string][] = []
  for (const [key, address] of Object.entries(value as Record<string, unknown>)) {
    if (typeof address !== 'string' || !key) continue
    const urlLike = /^(\.{0,2}\/)/.test(key) || URL.canParse(key)
    const from = urlLike ? new URL(key, base).href : key
    if (!URL.canParse(address, base)) continue
    out.push([from, new URL(address, base).href])
  }
  return out
}

function runImportMap(script: Script): void {
  try {
    const map = JSON.parse(dom.textContent(script[ID])) as { imports?: unknown; scopes?: Record<string, unknown> }
    const base = env.url
    const scopes = Object.entries(map.scopes ?? {}).map(([prefix, value]) => [new URL(prefix, base).href, specifierMap(value, base)] as [string, [string, string][]])
    host.addImportMap?.(specifierMap(map.imports, base), scopes)
  } catch (error) {
    report(error)
    host.setTimer(0, () => fire(script, new Event('error')))
  }
}

async function classicSource(url: string): Promise<string> {
  const bytes = host.readAsset(url)
  if (bytes) return host.decode(bytes, 'utf-8', false, false)
  return scriptSource(url)
}

let inlineCount = 0

function inlineUrl(extension: string): string {
  return new URL(`inline-${inlineCount++}.${extension}`, env.url).href
}

function execute(script: Script, source: string, url: string): void {
  documentState.currentScript = script
  try {
    host.runScript(source, url)
  } catch (error) {
    report(error)
  } finally {
    documentState.currentScript = null
  }
}

async function runClassic(script: Script): Promise<void> {
  const src = dom.getAttribute(script[ID], 'src')
  if (src === null) return execute(script, dom.textContent(script[ID]), inlineUrl('js'))
  const url = resolveUrl(src)
  let source: string
  try {
    source = await classicSource(url)
  } catch (error) {
    report(error)
    fire(script, new Event('error'))
    return
  }
  execute(script, source, url)
  fire(script, new Event('load'))
}

function blocking(script: Script): boolean {
  return !(dom.hasAttribute(script[ID], 'src') && dom.hasAttribute(script[ID], 'async'))
}

async function runModule(script: Script): Promise<void> {
  const src = dom.getAttribute(script[ID], 'src')
  try {
    if (src !== null) await host.importModule(resolveUrl(src))
    else await host.runModule(dom.textContent(script[ID]), inlineUrl('mjs'))
    if (src !== null) fire(script, new Event('load'))
  } catch (error) {
    report(error)
    if (src !== null) fire(script, new Event('error'))
  }
}

export async function loadPage(html: string): Promise<void> {
  const old = strongIds().filter((id) => id !== documentId)
  if (old.length) detach(old)
  documentState.readyState = 'loading'
  noteMarkup(html)
  dom.parseDocument(html)
  treeChanged()
  roots()
  parsed(wrap<Node>(documentId)!)
  const scripts = [...dom.querySelectorAll(documentId, 'script')].map((id) => wrap<Script>(id)!)
  for (const script of scripts) script[STARTED] = true
  for (const script of scripts) if (kind(script) === 'importmap') runImportMap(script)
  for (const script of scripts) {
    if (kind(script) !== 'classic') continue
    if (blocking(script)) await runClassic(script)
    else void runClassic(script)
  }
  for (const script of scripts) if (kind(script) === 'module') void runModule(script)
  const document = wrap<EventTarget>(documentId)!
  documentState.readyState = 'interactive'
  fire(document, new Event('readystatechange'))
  fire(document, new Event('DOMContentLoaded', { bubbles: true }))
  documentState.readyState = 'complete'
  fire(document, new Event('readystatechange'))
  fire(G as unknown as EventTarget, new Event('load'))
  fire(G as unknown as EventTarget, new Event('pageshow'))
}

lifecycle.connected.push((node: Wrapper) => {
  if (!(node instanceof HTMLScriptElement)) return
  const script = node as Script
  if (script[STARTED] || documentState.readyState === 'loading') return
  const type = kind(script)
  if (!type) return
  if (!dom.hasAttribute(script[ID], 'src') && dom.textContent(script[ID]) === '') return
  script[STARTED] = true
  if (type === 'classic') void runClassic(script)
  else if (type === 'importmap') runImportMap(script)
  else void runModule(script)
})
