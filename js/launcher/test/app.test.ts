import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { route, wallpaperOf } from '../src/app/calls.ts'
import { AppMenus } from '../src/app/menus.ts'
import { PageCalls } from '../src/app/page.ts'

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

  test('the file pickers hand the host the app label and sanitized filters, and pass back the lent files', async () => {
    const seen: unknown[] = []
    const book = { name: 'file', label: 'a.epub', writable: false }
    const out = { name: 'file-2', label: 'out.md', writable: true }
    const pages = new PageCalls({
      label: 'Reader',
      language: () => 'en',
      openFiles: async (title, multiple, filters) => {
        seen.push({ title, multiple, filters })
        return [book]
      },
      saveFile: async (title, name, filters) => {
        seen.push({ title, name, filters })
        return out
      },
      folder: async (title, writable) => {
        seen.push({ title, writable })
        return { name: 'work', label: 'Notes' }
      },
      unlend: async () => undefined,
    })
    assert.deepEqual(await pages.call('open', [{ multiple: true, filters: [{ name: 'Books', globs: ['*.epub'] }] }]), [book])
    assert.deepEqual(await pages.call('save', [{ name: 'notes.md' }]), out)
    assert.deepEqual(await pages.call('folder', [{ title: 'Reader: Pick', writable: true }]), { name: 'work', label: 'Notes' })
    assert.deepEqual(seen, [
      { title: 'Reader: Open', multiple: true, filters: [{ name: 'Books', globs: ['*.epub'], mimes: [] }] },
      { title: 'Reader: Save', name: 'notes.md', filters: [] },
      { title: 'Reader: Pick', writable: true },
    ])
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
