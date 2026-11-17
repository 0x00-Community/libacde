import { DOMException } from '../core/exception'
import { illegal, tag } from '../core/host'
import { env } from '../dom/env'
import { storageRootHandle, type FileSystemDirectoryHandle } from '../dom/picker'
import type { StorageNative } from '../native'

export class StorageManager {
  readonly #native: StorageNative | null

  constructor(...key: unknown[]) {
    illegal(key[0])
    this.#native = (key[1] as StorageNative | undefined) ?? null
  }

  async estimate(): Promise<{ usage: number; quota: number }> {
    const native = this.#native
    if (!native?.estimate) return { usage: 0, quota: 0 }
    const { usage, quota } = await native.estimate()
    return { usage, quota }
  }

  async persist(): Promise<boolean> {
    return true
  }

  async persisted(): Promise<boolean> {
    return true
  }

  async getDirectory(): Promise<FileSystemDirectoryHandle> {
    let root: { path: string } | null
    try {
      root = env.files?.privateDirectory() ?? null
    } catch (error) {
      throw new DOMException(String((error as Error)?.message ?? error).replace(/^UnknownError: /, ''), 'UnknownError')
    }
    if (!root) throw new DOMException('The origin private file system is not available here.', 'SecurityError')
    return storageRootHandle(root.path)
  }
}
tag(StorageManager)
