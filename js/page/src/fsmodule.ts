import type { FileBytes } from '@dopame/acde-api'
import { FileTypes } from './mimedb'

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
}
