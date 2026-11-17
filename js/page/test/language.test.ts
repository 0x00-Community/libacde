import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { HostStrings, SessionLanguage } from '../src/language.ts'

describe('SessionLanguage.of', () => {
  test('the locale variables decide when one is set', () => {
    assert.deepEqual(SessionLanguage.of({ LANG: 'zh_CN.UTF-8' }, ['en-US']), { language: 'zh-CN', languages: ['zh-CN', 'zh'] })
    assert.deepEqual(SessionLanguage.of({ LANGUAGE: 'de_DE:fr', LANG: 'C' }, ['ja-JP']), { language: 'de-DE', languages: ['de-DE', 'de', 'fr', 'en-US', 'en'] })
  })

  test("without them the operating system's preferred languages are used", () => {
    assert.deepEqual(SessionLanguage.of({}, ['zh-CN', 'en-US']), { language: 'zh-CN', languages: ['zh-CN', 'zh', 'en-US', 'en'] })
    assert.deepEqual(SessionLanguage.of({ LANG: '' }, ['zh-Hans-CN', 'en']), { language: 'zh-CN', languages: ['zh-CN', 'zh', 'en'] })
    assert.deepEqual(SessionLanguage.of({}, ['zh-Hant-TW', 'zh-Hans']), { language: 'zh-TW', languages: ['zh-TW', 'zh', 'zh-Hans'] })
    assert.deepEqual(SessionLanguage.of({}, []), { language: 'en-US', languages: ['en-US', 'en'] })
  })
})

const roots = [
  { id: 'private', label: 'App data', source: 'private' },
  { id: 'documents', label: 'Documents', source: 'place', place: 'documents' },
  { id: 'folder-0123456789abcdef', label: 'Photos 2026', source: 'granted' },
  { id: '/home/u/Desktop', label: 'Desktop', source: 'desktop' },
  { id: '/home/u/agentos-work', label: '~/agentos-work', source: 'default' },
]

test('root labels follow the page language, English otherwise; picked folders keep their names', () => {
  const zh = HostStrings.roots(roots, 'zh-CN').map((root) => root.label)
  assert.deepEqual(zh, ['应用数据', '文档', 'Photos 2026', '桌面', '~/agentos-work'])
  for (const language of ['en-US', 'de-DE', undefined, 'zhx']) {
    assert.deepEqual(HostStrings.roots(roots, language).map((root) => root.label), roots.map((root) => root.label), String(language))
  }
  assert.equal(HostStrings.roots(null, 'zh'), null)
})

test('chooser titles follow the language', () => {
  assert.equal(HostStrings.of('zh-Hans-CN').readFolder, '选择要读取的文件夹')
  assert.equal(HostStrings.of('en-GB').save, 'Save')
  assert.equal(HostStrings.of(null).desktop, 'Desktop')
})
