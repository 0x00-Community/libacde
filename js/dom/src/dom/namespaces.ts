import { DOMException } from '../core/exception'
import { NONE, NodeType, Ns } from '../native'
import { dom } from './realm'

export const HTML_NS = 'http://www.w3.org/1999/xhtml'
export const SVG_NS = 'http://www.w3.org/2000/svg'
export const MATHML_NS = 'http://www.w3.org/1998/Math/MathML'
export const XML_NS = 'http://www.w3.org/XML/1998/namespace'
export const XMLNS_NS = 'http://www.w3.org/2000/xmlns/'

export const KNOWN: Record<string, Ns> = { [HTML_NS]: Ns.Html, [SVG_NS]: Ns.Svg, [MATHML_NS]: Ns.MathMl }
export const URIS = [HTML_NS, SVG_NS, MATHML_NS] as const

export const documents = { others: false, xml: false }

export function inHtmlDocument(id: number): boolean {
  return !documents.xml || dom.inHtmlDocument(id)
}

export function nullable(namespace: unknown): string | null {
  if (namespace === null || namespace === undefined) return null
  const ns = String(namespace)
  return ns === '' ? null : ns
}

const ELEMENT_REST = /^[A-Za-z0-9\-.:_\u0080-\u{10FFFF}]*$/u
const FORBIDDEN_PREFIX = /[\t\n\f\r \0/>]/
const FORBIDDEN_ATTRIBUTE = /[\t\n\f\r \0/=>]/

export function validElementLocalName(name: string): boolean {
  if (name.length === 0) return false
  const first = name.codePointAt(0)!
  if ((first >= 0x41 && first <= 0x5a) || (first >= 0x61 && first <= 0x7a)) return !FORBIDDEN_PREFIX.test(name)
  if (first !== 0x3a && first !== 0x5f && first < 0x80) return false
  return ELEMENT_REST.test(name.slice(String.fromCodePoint(first).length))
}

export function validAttributeLocalName(name: string): boolean {
  return name.length > 0 && !FORBIDDEN_ATTRIBUTE.test(name)
}

export function validNamespacePrefix(prefix: string): boolean {
  return prefix.length > 0 && !FORBIDDEN_PREFIX.test(prefix)
}

export interface Extracted {
  namespace: string | null
  prefix: string | null
  local: string
}

export function validateAndExtract(namespace: unknown, qualifiedName: string, context: 'element' | 'attribute', method: string): Extracted {
  const ns = nullable(namespace)
  const name = String(qualifiedName)
  let prefix: string | null = null
  let local = name
  const colon = name.indexOf(':')
  if (colon >= 0) {
    prefix = name.slice(0, colon)
    local = name.slice(colon + 1)
  }
  const invalid = (what: string) => new DOMException(`Failed to execute '${method}': ${what}`, 'InvalidCharacterError')
  if (prefix !== null && !validNamespacePrefix(prefix)) throw invalid(`The prefix of '${name}' is not valid.`)
  if (context === 'attribute' && !validAttributeLocalName(local)) throw invalid(`The qualified name provided ('${name}') contains the invalid name-start character '${local[0] ?? ''}'.`)
  if (context === 'element' && !validElementLocalName(local)) throw invalid(`The qualified name provided ('${name}') contains the invalid name-start character '${local[0] ?? ''}'.`)
  const namespaceError = (what: string) => new DOMException(`Failed to execute '${method}': ${what}`, 'NamespaceError')
  if (prefix !== null && ns === null) throw namespaceError(`The namespace URI provided ('') is not valid for the qualified name provided ('${name}').`)
  if (prefix === 'xml' && ns !== XML_NS) throw namespaceError(`The namespace URI provided ('${ns}') is not valid for the qualified name provided ('${name}').`)
  if ((name === 'xmlns' || prefix === 'xmlns') && ns !== XMLNS_NS) throw namespaceError(`The qualified name provided ('${name}') contains the prefix 'xmlns', but the namespace URI provided is not '${XMLNS_NS}'.`)
  if (ns === XMLNS_NS && name !== 'xmlns' && prefix !== 'xmlns') throw namespaceError(`The namespace URI provided ('${XMLNS_NS}') is reserved for the 'xmlns' prefix.`)
  return { namespace: ns, prefix, local }
}

export function namespaceOf(id: number): string | null {
  const ns = dom.namespace(id)
  return ns === Ns.Other ? nullable(dom.namespaceURI(id)) : URIS[ns]!
}

function parentElement(id: number): number {
  const parent = dom.parent(id)
  return parent !== NONE && dom.nodeType(parent) === NodeType.Element ? parent : NONE
}

function documentElementOfNode(id: number): number {
  const children = dom.children(id)
  return children.length ? children[0]! : NONE
}

export function locateNamespace(id: number, prefix: string | null): string | null {
  switch (dom.nodeType(id)) {
    case NodeType.Element: {
      for (let el = id; el !== NONE; el = parentElement(el)) {
        if (prefix === 'xml') return XML_NS
        if (prefix === 'xmlns') return XMLNS_NS
        const ns = namespaceOf(el)
        if (ns !== null && nullable(dom.prefix(el)) === prefix) return ns
        const attrs = dom.attributesNS(el)
        for (let i = 0; i < attrs.length; i += 4) {
          if (attrs[i] !== XMLNS_NS) continue
          const attrPrefix = attrs[i + 1]!, local = attrs[i + 2]!
          if ((attrPrefix === 'xmlns' && local === prefix) || (prefix === null && attrPrefix === '' && local === 'xmlns')) return nullable(attrs[i + 3])
        }
      }
      return null
    }
    case NodeType.Document: {
      const root = documentElementOfNode(id)
      return root === NONE ? null : locateNamespace(root, prefix)
    }
    case NodeType.DocumentType:
    case NodeType.DocumentFragment:
      return null
    default: {
      const parent = parentElement(id)
      return parent === NONE ? null : locateNamespace(parent, prefix)
    }
  }
}

export function locatePrefix(id: number, namespace: string): string | null {
  for (let el = id; el !== NONE; el = parentElement(el)) {
    const prefix = nullable(dom.prefix(el))
    if (namespaceOf(el) === namespace && prefix !== null) return prefix
    const attrs = dom.attributesNS(el)
    for (let i = 0; i < attrs.length; i += 4) {
      if (attrs[i] === XMLNS_NS && attrs[i + 1] === 'xmlns' && attrs[i + 3] === namespace) return attrs[i + 2]!
    }
  }
  return null
}

export function lookupPrefixOf(id: number, namespace: string | null): string | null {
  if (namespace === null) return null
  switch (dom.nodeType(id)) {
    case NodeType.Element:
      return locatePrefix(id, namespace)
    case NodeType.Document: {
      const root = documentElementOfNode(id)
      return root === NONE ? null : locatePrefix(root, namespace)
    }
    case NodeType.DocumentType:
    case NodeType.DocumentFragment:
      return null
    default: {
      const parent = parentElement(id)
      return parent === NONE ? null : locatePrefix(parent, namespace)
    }
  }
}
