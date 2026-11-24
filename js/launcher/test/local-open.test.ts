import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { bindNatives } from '../../page/src/sys/natives.ts'
import { LocalOpener, withDefault, type OpenFiles } from '../src/local/open.ts'

bindNatives({ host: { logEnabled: () => false, log: () => {} }, sys: { platform: 'linux', argv: [], pid: 7, env: () => ({}), stderr: () => {} } } as never)

function files(given: Record<string, string>, dirs: readonly string[] = []): OpenFiles & { written: Record<string, string> } {
  const written: Record<string, string> = {}
  return {
    written,
    readText: (path) => written[path] ?? given[path] ?? null,
    readDir: (dir) => [...new Set(Object.keys(given).filter((p) => p.startsWith(`${dir}/`)).map((p) => p.slice(dir.length + 1).split('/')[0]!))],
    isDirectory: (path) => dirs.includes(path),
    exists: (path) => path in given || dirs.includes(path),
    writeText: (path, text) => {
      written[path] = text
    },
  }
}

const places = { data: ['/home/u/.local/share', '/usr/share'], config: '/home/u/.config', configDirs: ['/etc/xdg'], applications: ['/home/u/.local/share/applications', '/usr/share/applications'] }

const GLOBS = [
  '# comment', '50:text/markdown:*.md', '50:text/x-c++src:*.C:cs', '50:text/x-csrc:*.c', '50:text/x-makefile:Makefile', '60:application/x-compressed-tar:*.tar.gz',
  '50:application/gzip:*.gz',
].join('\n')

describe('what a file is and who opens it on the host (as acdes mime.rs / defaults.rs)', () => {
  test('types come from shared-mime-info globs2: weight, then the longer pattern, case only when cs; else the extension table; folders are inode/directory', () => {
    const opener = new LocalOpener(files({ '/usr/share/mime/globs2': GLOBS }, ['/docs']), places)
    assert.equal(opener.mimeOf('/x/Notes.MD'), 'text/markdown')
    assert.equal(opener.mimeOf('/x/a.C'), 'text/x-c++src')
    assert.equal(opener.mimeOf('/x/a.c'), 'text/x-csrc')
    assert.equal(opener.mimeOf('/x/makefile'), 'text/x-makefile')
    assert.equal(opener.mimeOf('/x/a.tar.gz'), 'application/x-compressed-tar')
    assert.equal(opener.mimeOf('/x/report.PDF'), 'application/pdf')
    assert.equal(opener.mimeOf('/x/what.unknownext'), 'application/octet-stream')
    assert.equal(opener.mimeOf('/docs'), 'inode/directory')
  })

  test('a remembered choice comes first, from acde-mimeapps.list, then mimeapps.list; remember writes the own list', () => {
    const given = files({ '/etc/xdg/mimeapps.list': '[Default Applications]\ntext/plain=gedit.desktop;\n' })
    const opener = new LocalOpener(given, places)
    assert.deepEqual(opener.remembered('text/plain'), ['gedit.desktop'])
    opener.remember('text/plain', 'acde-editor.desktop')
    assert.equal(given.written['/home/u/.config/acde-mimeapps.list'], '[Default Applications]\ntext/plain=acde-editor.desktop;\n')
    assert.deepEqual(opener.remembered('text/plain'), ['acde-editor.desktop', 'gedit.desktop'])
    assert.equal(withDefault('[Default Applications]\ntext/plain=a;\n[Added Associations]\nx=y\n', 'text/plain', 'b'), '[Default Applications]\ntext/plain=b;\n[Added Associations]\nx=y\n')
  })

  test('programs are shown .desktop entries listing the type; acde-*.desktop and hidden ones are not programs', () => {
    const entry = (name: string, extra = '') => `[Desktop Entry]\nType=Application\nName=${name}\nMimeType=text/plain;image/png;\nIcon=${name.toLowerCase()}\n${extra}`
    const opener = new LocalOpener(files({
      '/usr/share/applications/gedit.desktop': entry('Gedit'),
      '/usr/share/applications/hidden.desktop': entry('Hidden', 'NoDisplay=true\n'),
      '/usr/share/applications/acde-editor.desktop': entry('Editor'),
      '/home/u/.local/share/applications/gedit.desktop': entry('My Gedit'),
    }), places)
    assert.deepEqual(opener.natives('text/plain').map((n) => [n.id, n.label, n.file]), [['gedit.desktop', 'My Gedit', '/home/u/.local/share/applications/gedit.desktop']])
  })

  test('chosen: the remembered acde app or program, else the first app whose manifest lists the type, else the first program', () => {
    const given = files({ '/home/u/.config/acde-mimeapps.list': '[Default Applications]\ntext/plain=acde-notes.desktop;\n' })
    const opener = new LocalOpener(given, places)
    const native = { id: 'gedit.desktop', label: 'Gedit', file: '/a/gedit.desktop', icon: null }
    const isApp = (id: string) => ['notes', 'editor'].includes(id)
    assert.deepEqual(opener.chosen('text/plain', [native], isApp, () => 'editor'), { kind: 'acde', app: 'notes' })
    assert.deepEqual(opener.chosen('image/png', [native], isApp, () => 'editor'), { kind: 'acde', app: 'editor' })
    assert.deepEqual(opener.chosen('image/png', [native], isApp, () => null), { kind: 'native', native })
    assert.equal(opener.chosen('image/png', [], isApp, () => null), null)
  })
})
