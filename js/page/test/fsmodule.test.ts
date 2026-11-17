import assert from 'node:assert/strict'
import { test } from 'node:test'
import { FsModule } from '../src/fsmodule.ts'
import { FileTypes, MimeDb } from '../src/mimedb.ts'

const db = MimeDb.parse([{ globs2: '50:text/markdown:*.md\n50:image/png:*.png\n50:application/pdf:*.pdf\n', subclasses: null }])!
const shared = new FileTypes({ registered: null, database: () => db })
const windows = new FileTypes({ registered: (ext) => (ext === '.png' ? 'image/png' : null), database: () => null })

const native = () => {
  const calls: string[] = []
  const fs = {
    download: async (root: string, path: string) => (calls.push(`${root}:${path}`), { name: path.split('/').pop()!, data: new Uint8Array([1, 2, 3]) }),
    list: async (root: string) => ({ root, entries: [] }),
  }
  return { fs, calls }
}

test('download answers the type the system knows for the name, as File.type does', async () => {
  const { fs, calls } = native()
  const typed = FsModule.typed(fs, () => shared)
  assert.deepEqual(await typed.download('downloads', 'notes/note.md'), { name: 'note.md', contentType: 'text/markdown', data: new Uint8Array([1, 2, 3]) })
  assert.equal((await typed.download('downloads', 'shot.PNG')).contentType, 'image/png')
  assert.equal((await typed.download('private', 'report.pdf')).contentType, 'application/pdf')
  assert.deepEqual(calls, ['downloads:notes/note.md', 'downloads:shot.PNG', 'private:report.pdf'])
  assert.equal((await FsModule.typed(fs, () => windows).download('desktop', 'shot.png')).contentType, 'image/png')
})

test('a name the system knows no type for is application/octet-stream', async () => {
  const { fs } = native()
  const typed = FsModule.typed(fs, () => shared)
  assert.equal((await typed.download('private', 'data.xyz')).contentType, 'application/octet-stream')
  assert.equal((await typed.download('private', 'LICENSE')).contentType, 'application/octet-stream')
  assert.equal((await FsModule.typed(fs, () => windows).download('private', 'note.md')).contentType, 'application/octet-stream')
})

test('the other calls are the native ones, and a failed download stays the native error', async () => {
  const { fs } = native()
  const typed = FsModule.typed(fs, () => shared)
  assert.equal(typed.list, fs.list)
  const denied = Object.assign(new Error('no such file'), { status: 404, code: 'ENOENT' })
  const failing = FsModule.typed({ download: async (_root: string, _path: string) => Promise.reject(denied) }, () => shared)
  await assert.rejects(failing.download('private', 'gone.md'), (error) => error === denied)
})
