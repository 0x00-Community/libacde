import type { Header } from './backend'

const SIMPLE_METHODS = new Set(['GET', 'HEAD', 'POST'])
const SIMPLE_TYPES = new Set(['application/x-www-form-urlencoded', 'multipart/form-data', 'text/plain'])
const SAFE_RESPONSE = new Set(['cache-control', 'content-language', 'content-length', 'content-type', 'expires', 'last-modified', 'pragma'])
const UNSAFE_BYTE = /[\x00-\x08\x0a-\x1f"():<>?@[\\\]{}\x7f]/

function valueOf(headers: readonly Header[], name: string): string | null {
  const values = headers.filter(([n]) => n.toLowerCase() === name).map(([, v]) => v)
  return values.length ? values.join(', ') : null
}

function listOf(headers: readonly Header[], name: string): string[] {
  return (valueOf(headers, name) ?? '').split(',').map((item) => item.trim()).filter(Boolean)
}

export class Cors {
  static simpleMethod(method: string): boolean {
    return SIMPLE_METHODS.has(method)
  }

  static safelisted(name: string, value: string): boolean {
    if (value.length > 128) return false
    switch (name.toLowerCase()) {
      case 'accept':
        return !UNSAFE_BYTE.test(value)
      case 'accept-language':
      case 'content-language':
        return /^[0-9A-Za-z *,\-.;=]*$/.test(value)
      case 'content-type':
        return !UNSAFE_BYTE.test(value) && SIMPLE_TYPES.has(value.split(';')[0]!.trim().toLowerCase())
      default:
        return false
    }
  }

  static unsafeNames(headers: readonly Header[]): string[] {
    return [...new Set(headers.filter(([name, value]) => !Cors.safelisted(name, value)).map(([name]) => name.toLowerCase()))].sort()
  }

  static check(headers: readonly Header[], origin: string, credentials: boolean): string | null {
    const allow = valueOf(headers, 'access-control-allow-origin')
    if (allow === null) return "No 'Access-Control-Allow-Origin' header is present on the requested resource."
    if (allow === '*' && !credentials) return null
    if (allow === '*') return "The value of the 'Access-Control-Allow-Origin' header in the response must not be the wildcard '*' when the request's credentials mode is 'include'."
    if (allow !== origin) return `The 'Access-Control-Allow-Origin' header has a value '${allow}' that is not equal to the supplied origin.`
    if (credentials && valueOf(headers, 'access-control-allow-credentials') !== 'true') {
      return "The value of the 'Access-Control-Allow-Credentials' header in the response must be 'true' when the request's credentials mode is 'include'."
    }
    return null
  }

  static preflight(headers: readonly Header[], method: string, names: readonly string[], credentials: boolean): string | null {
    const methods = listOf(headers, 'access-control-allow-methods')
    if (!Cors.simpleMethod(method) && !methods.includes(method) && !(methods.includes('*') && !credentials)) {
      return `Method ${method} is not allowed by Access-Control-Allow-Methods in preflight response.`
    }
    const allowed = new Set(listOf(headers, 'access-control-allow-headers').map((name) => name.toLowerCase()))
    const any = allowed.has('*') && !credentials
    for (const name of names) {
      if (!allowed.has(name) && !(any && name !== 'authorization')) return `Request header field ${name} is not allowed by Access-Control-Allow-Headers in preflight response.`
    }
    return null
  }

  static exposed(headers: readonly Header[], credentials: boolean): Header[] {
    const expose = new Set(listOf(headers, 'access-control-expose-headers').map((name) => name.toLowerCase()))
    const all = expose.has('*') && !credentials
    return headers.filter(([name]) => {
      const lower = name.toLowerCase()
      return !/^set-cookie2?$/.test(lower) && (all || SAFE_RESPONSE.has(lower) || expose.has(lower))
    })
  }
}
