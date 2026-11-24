import type { FileBytes, FsChange, FsRootsEvent, FsWatchEvent } from '@acde/api'
import { FileTypes } from './mimedb'

interface NativeChange {
  readonly type: FsChange['type']
  readonly name: string
  readonly from?: string
  readonly directory: boolean
}

export interface FsWatchNatives {
  watch(root: string, path: string, callback: (changes: NativeChange[]) => void): Promise<number>
  unwatch(id: number): void
}

type NativeDownload = (root: string, path: string) => Promise<Omit<FileBytes, 'contentType'>>
type Typed<T> = Omit<T, 'download'> & { download(root: string, path: string): Promise<FileBytes> }

export class FsModule {
  static readonly OCTETS = 'application/octet-stream'

  static contentType(name: string, types: Pick<FileTypes, 'byName'>): string {
    return types.byName(name) || FsModule.OCTETS
  }

  static typed<T extends { download: NativeDownload }>(fs: T, types: () => Pick<FileTypes, 'byName'> = () => FileTypes.system()): Typed<T> {
    const download = async (root: string, path: string): Promise<FileBytes> => {
      const file = await fs.download(root, path)
      return { name: file.name, contentType: FsModule.contentType(file.name, types()), data: file.data }
    }
    return { ...fs, download }
  }

  static async watch(fs: FsWatchNatives, root: unknown, path: unknown, handler: unknown, report: (error: unknown) => void): Promise<() => boolean> {
    if (typeof handler !== 'function') throw new TypeError('acde:fs watch: the handler is not a function')
    const at = String(root)
    const dir = String(path ?? '').replace(/^\/+|\/+$/g, '')
    const under = (name: string) => (name === '' ? dir : dir === '' ? name : `${dir}/${name}`)
    const id = await fs.watch(at, dir, (changes) => {
      const event: FsWatchEvent = {
        root: at,
        path: dir,
        changes: changes.map((c) => ({ type: c.type, path: under(c.name), ...(c.from !== undefined && { from: under(c.from) }), kind: c.directory ? 'directory' : 'file' })),
      }
      try {
        ;(handler as (event: FsWatchEvent) => void)(event)
      } catch (error) {
        report(error)
      }
    })
    let open = true
    return () => {
      if (!open) return false
      open = false
      fs.unwatch(id)
      return true
    }
  }

  static rootsEvent(message: { added?: unknown; removed?: unknown }): FsRootsEvent {
    const ids = (value: unknown) => (Array.isArray(value) ? value.filter((id): id is string => typeof id === 'string') : [])
    const added = ids(message.added)
    return { removed: ids(message.removed), ...(added.length > 0 && { added }) }
  }
}
