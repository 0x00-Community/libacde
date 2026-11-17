import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { FileTypes, MimeDb } from '../src/mimedb.ts'

const globs2 = `# comment
80:application/x-heavy:*.foo
50:text/x-light:*.foo
50:text/x-makefile:Makefile
50:text/x-makefile:makefile
10:text/x-makefile:*.mk
50:application/gzip:*.gz
50:application/x-compressed-tar:*.tar.gz
50:text/markdown:*.md
50:text/x-c++src:*.C:cs
50:text/x-csrc:*.c
50:text/vnd.trolltech.linguist:*.ts
50:video/mp2t:*.ts
50:text/troff:*.[1-9]
40:text/x-readme:README*
50:application/x-gone:*.gone
`
const subclasses = `text/vnd.trolltech.linguist application/xml
application/xml text/plain
`

const db = MimeDb.parse([{ globs2, subclasses }])!

describe('MimeDb', () => {
  test('no database at all', () => {
    assert.equal(MimeDb.parse([{ globs2: null, subclasses: null }]), null)
  })

  test('literal names beat patterns', () => {
    assert.deepEqual(db.candidates('Makefile'), ['text/x-makefile'])
    assert.deepEqual(db.candidates('MAKEFILE'), ['text/x-makefile'])
  })

  test('higher weight, then the longer pattern', () => {
    assert.deepEqual(db.candidates('a.foo'), ['application/x-heavy'])
    assert.deepEqual(db.candidates('a.tar.gz'), ['application/x-compressed-tar'])
    assert.deepEqual(db.candidates('a.gz'), ['application/gzip'])
  })

  test('case-insensitive unless marked cs, and cs wins a tie', () => {
    assert.deepEqual(db.candidates('NOTES.MD'), ['text/markdown'])
    assert.deepEqual(db.candidates('a.C'), ['text/x-c++src'])
    assert.deepEqual(db.candidates('a.c'), ['text/x-csrc'])
  })

  test('full globs and classes', () => {
    assert.deepEqual(db.candidates('ls.1'), ['text/troff'])
    assert.deepEqual(db.candidates('README.txt'), ['text/x-readme'])
  })

  test('ties stay ambiguous', () => {
    assert.deepEqual(db.candidates('main.ts'), ['text/vnd.trolltech.linguist', 'video/mp2t'])
  })

  test('nothing matches', () => {
    assert.deepEqual(db.candidates('archive.unknownext'), [])
  })

  test('text is text/* or a subclass of text/plain', () => {
    assert.equal(db.isText('text/markdown'), true)
    assert.equal(db.isText('application/xml'), true)
    assert.equal(db.isText('text/vnd.trolltech.linguist'), true)
    assert.equal(db.isText('video/mp2t'), false)
  })

  test('__NOGLOBS__ in a more important directory drops the globs below it', () => {
    const user = '50:application/x-gone:__NOGLOBS__\n50:text/x-mine:*.gone\n'
    const merged = MimeDb.parse([{ globs2: user, subclasses: null }, { globs2, subclasses }])!
    assert.deepEqual(merged.candidates('a.gone'), ['text/x-mine'])
    assert.deepEqual(merged.candidates('a.md'), ['text/markdown'])
  })
})

const REGISTRY: Record<string, string> = {
  '.png': 'image/png',
  '.pdf': 'application/pdf',
  '.csv': 'application/vnd.ms-excel',
  '.zip': 'application/x-zip-compressed',
  '.docx': 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  '.odd': 'not a type',
}

function windows(asked: string[] = []): FileTypes {
  const registered = (ext: string) => (asked.push(ext), REGISTRY[ext] ?? null)
  return new FileTypes({ registered, database: () => assert.fail('Windows hosts never read shared-mime-info') })
}

describe('FileTypes on Windows (file associations)', () => {
  test('gives the content type the system registers for the extension', () => {
    const asked: string[] = []
    const types = windows(asked)
    assert.equal(types.associations, true)
    assert.equal(types.byName('photo.PNG'), 'image/png')
    assert.equal(types.byName('C:\\Users\\u\\table.csv'), 'application/vnd.ms-excel')
    assert.equal(types.byName('backup.tar.zip'), 'application/x-zip-compressed')
    assert.deepEqual(asked, ['.png', '.csv', '.zip'])
  })

  test('a name the system knows no type for is unknown, not guessed', () => {
    const asked: string[] = []
    const types = windows(asked)
    for (const name of ['notes.md', 'main.py', 'lib.rs', 'README', 'trailing.', 'odd.odd']) assert.equal(types.byName(name), FileTypes.UNKNOWN, name)
    assert.deepEqual(asked, ['.md', '.py', '.rs', '.odd'])
  })
})

describe('FileTypes elsewhere (shared-mime-info)', () => {
  test('takes the best glob, and nothing without a database', () => {
    const db = MimeDb.parse([{ globs2: '50:text/markdown:*.md\n50:text/x-python:*.py', subclasses: null }])
    const types = new FileTypes({ registered: null, database: () => db })
    assert.equal(types.associations, false)
    assert.equal(types.byName('notes.md'), 'text/markdown')
    assert.equal(types.byName('photo.png'), FileTypes.UNKNOWN)
    assert.equal(new FileTypes({ registered: null, database: () => null }).byName('notes.md'), FileTypes.UNKNOWN)
  })
})

test('the extension is what Windows associates: the last one, lowercased', () => {
  assert.equal(FileTypes.extension('a.tar.GZ'), '.gz')
  assert.equal(FileTypes.extension('dir.d/noext'), '')
  assert.equal(FileTypes.extension('C:\\x.y\\file'), '')
  assert.equal(FileTypes.extension('.bashrc'), '.bashrc')
  assert.equal(FileTypes.extension('weird.a b'), '')
})
