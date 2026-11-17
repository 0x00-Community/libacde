import { Manifest } from '@dopame/acde-api/manifest'
import type { Capabilities } from '../../contract'
import { Permissions, type Approval } from './approval/permissions'

export type FsGrant = true | { readonly places: string[]; readonly paths: boolean } | null

export interface FileOrigin {
  readonly level: number
  readonly system?: boolean
}

export class FileAccess {
  static grant(caps: Capabilities | null, origin: FileOrigin | null): FsGrant {
    if (!caps) return true
    const fs = caps.fs
    if (!fs) return null
    const places = fs === true ? [] : Manifest.places(caps)
    return { places, paths: !!origin?.system && places.length > 0 }
  }

  static view(caps: Capabilities, origin: FileOrigin | null): 'roots' | 'url' | 'handle' {
    const fs = FileAccess.grant(caps, origin)
    if (fs === true) return 'roots'
    const paths = fs ? fs.paths : !!origin?.system
    return paths ? 'url' : 'handle'
  }

  static guard<T extends object>(fs: T, approval: (capability: string) => Approval | null, label: string, office = false): T {
    const refusal = (root: unknown): Error | null => {
      const capability = typeof root === 'string' ? `fs.${root}` : ''
      const state = Manifest.place(capability) || Permissions.folder(capability) ? approval(capability) : null
      return state && state !== 'granted' ? Permissions.error(label, capability, state) : null
    }
    const blocked = (root: unknown): boolean => refusal(root) !== null
    const folder = (id: unknown) => typeof id === 'string' && !!Permissions.folder(`fs.${id}`)
    const shown = (root: { place?: unknown; id?: unknown }) => !blocked(root?.place) && !(folder(root?.id) && blocked(root?.id))
    const visible = (roots: unknown) => (Array.isArray(roots) ? roots.filter(shown) : roots)
    const rootOf = (key: string | symbol, args: unknown[]) => {
      if (!office || key === 'state') return args[0]
      return key === 'new' ? (args[0] as { root?: unknown } | null)?.root : null
    }
    const shape = (key: string | symbol, result: unknown) => {
      if (!office) return key === 'roots' ? Promise.resolve(result).then(visible) : result
      if (key !== 'state') return result
      const narrowed = (state: unknown) => (state && typeof state === 'object' ? { ...state, roots: visible((state as { roots?: unknown }).roots) } : state)
      return Promise.resolve(result).then(narrowed)
    }
    return new Proxy(fs, {
      get: (target, key, receiver) => {
        const member = Reflect.get(target, key, receiver) as unknown
        if (typeof member !== 'function') return member
        return (...args: unknown[]) => {
          const refused = refusal(rootOf(key, args)) ?? (!office && key === 'copy' ? refusal(args[2]) : null)
          if (refused) return Promise.reject(refused)
          return shape(key, (member as (...a: unknown[]) => unknown).apply(target, args))
        }
      },
    })
  }
}
