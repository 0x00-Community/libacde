import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { route, wallpaperOf } from '../src/app/calls.ts'
import { AppMenus } from '../src/app/menus.ts'
import { PageCalls } from '../src/app/page.ts'
import { bindNatives } from '../../page/src/sys/natives.ts'

const alone = { window: 1, desktop: false, managed: false }

describe('the single-app launcher answers its window and page calls', () => {
  test('without a manager the title, the menus and the file pickers stay here; with one they go to it', () => {
    assert.equal(route('window', 'setTitle', ['x'], alone), 'title')
    assert.equal(route('window', 'setMenus', [[]], alone), 'menus')
    assert.equal(route('page', 'open', [{}], alone), 'page')
    assert.equal(route('page', 'save', [{}], alone), 'page')
    assert.equal(route('page', 'notify', [{}], alone), 'page')
    assert.equal(route('page', 'shown', [], alone), 'acknowledge')
    assert.equal(route('page', 'download', [], alone), 'unsupported')
    assert.equal(route('window', 'setTitle', ['x'], { ...alone, managed: true }), 'manager')
    assert.equal(route('page', 'devreload', [], alone), 'devreload')
    assert.equal(route('page', 'devreload', [], { ...alone, managed: true }), 'devreload')
  })

  test('menus go to the host window, and their shortcuts are bound and found', () => {
    const calls: unknown[][] = []
    const shell = {
      setMenus: (window: number, menus: string) => void calls.push(['setMenus', window, JSON.parse(menus).length]),
      shortcut: (combo: string, scope: string, owner: number) => void calls.push(['shortcut', combo, scope, owner]),
      unshortcut: (combo: string, scope: string, owner: number) => void calls.push(['unshortcut', combo, scope, owner]),
    }
    const menus = new AppMenus(shell, () => 1)
    const item = (id: string, shortcut?: string) => ({ id, label: id, action: true, ...(shortcut && { shortcut }) })
    menus.set([{ id: 'view', label: 'View', items: [item('notes', 'mod+1'), item('habits', 'mod+2'), { type: 'separator' as const }] }])
    assert.deepEqual(menus.pressed('mod+2'), { menu: 'view', item: 'habits' })
    menus.set([{ id: 'view', label: 'View', items: [item('notes', 'mod+1')] }])
    assert.equal(menus.pressed('mod+2'), null)
    assert.deepEqual(calls, [
      ['setMenus', 1, 1], ['shortcut', 'mod+1', 'menu', 1], ['shortcut', 'mod+2', 'menu', 1],
      ['setMenus', 1, 1], ['unshortcut', 'mod+2', 'menu', 1],
    ])
  })

  test('with a session manager the menu chords go to the page instead of the shell', () => {
    const sent: (readonly string[])[] = []
    const menus = new AppMenus({ shortcut: () => assert.fail('no shell shortcut in a session') }, () => 1, (keys) => void sent.push(keys))
    menus.remember([{ id: 'file', label: 'File', items: [{ id: 'new', label: 'New', action: true, shortcut: 'Alt+N' }] }])
    assert.deepEqual(sent, [['Alt+N']])
    assert.deepEqual(menus.pressed('Alt+N'), { menu: 'file', item: 'new' })
  })

  test('the file pickers use the host file chooser with the app label; without one they are cancelled', async () => {
    const seen: unknown[] = []
    const chooser = {
      openFile: async (options: unknown) => {
        seen.push(options)
        return ['/tmp/a.epub']
      },
      saveFile: async (options: unknown) => {
        seen.push(options)
        return '/tmp/out.md'
      },
    }
    const folder = async (title: string, writable: boolean) => {
      seen.push({ title, writable })
      return { name: 'work', label: 'Notes' }
    }
    const pages = new PageCalls({ label: 'Reader', language: () => 'en', chooser: () => Promise.resolve(chooser), folder, unlend: async () => undefined })
    assert.deepEqual(await pages.call('open', [{ multiple: true, filters: [{ name: 'Books', globs: ['*.epub'] }] }]), ['/tmp/a.epub'])
    assert.equal(await pages.call('save', [{ name: 'notes.md' }]), '/tmp/out.md')
    assert.deepEqual(await pages.call('folder', [{ title: 'Reader: Pick', writable: true }]), { name: 'work', label: 'Notes' })
    assert.deepEqual(seen, [
      { title: 'Reader: Open', multiple: true, directory: false, filters: [{ name: 'Books', globs: ['*.epub'], mimes: [] }] },
      { title: 'Reader: Save', name: 'notes.md', filters: [] },
      { title: 'Reader: Pick', writable: true },
    ])
    const logged: string[] = []
    const log = (_level: number, _target: string, text: string) => void logged.push(text)
    bindNatives({ host: { log, logEnabled: () => false }, sys: { platform: 'linux', argv: [], pid: 1 } } as never)
    const none = new PageCalls({ label: 'Reader', language: () => 'en', chooser: () => null, folder: async () => null, unlend: async () => undefined })
    assert.equal(await none.call('open', [{}]), null)
    assert.match(logged[0]!, /no file chooser/)
  })
})

test('a wallpaper is a #rrggbb or #rrggbbaa colour and an optional absolute image', () => {
  assert.deepEqual(wallpaperOf({ color: '#0E0F14' }), { color: '#0E0F14' })
  assert.deepEqual(wallpaperOf({ color: '#ffffff80', image: null }), { color: '#ffffff80', image: null })
  assert.deepEqual(wallpaperOf({ color: '#FFFFFF', image: '/usr/share/wall.png' }), { color: '#FFFFFF', image: '/usr/share/wall.png' })
  assert.throws(() => wallpaperOf({ color: 'white' }), (e: { code?: string }) => e.code === 'bad_color')
  assert.throws(() => wallpaperOf({ color: '#fff' }), (e: { code?: string }) => e.code === 'bad_color')
  assert.throws(() => wallpaperOf({ color: '#FFFFFF', image: 'wall.png' }), (e: { code?: string }) => e.code === 'bad_request')
})
