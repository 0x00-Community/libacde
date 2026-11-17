import type { DevToolsDomain } from './index'

export class PageDomain {
  static create(frameId: string, url: () => string): DevToolsDomain {
    const frame = () => PageDomain.frame(frameId, url())
    return {
      name: 'Page',
      methods: {
        getFrameTree: () => ({ frameTree: { frame: frame() } }),
        getResourceTree: () => ({ frameTree: { frame: frame(), resources: [] } }),
      },
    }
  }

  static frame(id: string, url: string): Record<string, unknown> {
    let origin = ''
    try {
      origin = new URL(url).origin
    } catch {}
    return {
      id,
      loaderId: id,
      url,
      domainAndRegistry: '',
      securityOrigin: origin,
      mimeType: 'text/html',
      secureContextType: 'Secure',
      crossOriginIsolatedContextType: 'NotIsolated',
      gatedAPIFeatures: [],
    }
  }
}
