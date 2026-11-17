import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from './sys/fs'
import { sys } from './sys/natives'
import { xdgHome } from './sys/os'
import { dirname, join } from './sys/path'
import { process, type Env } from './sys/process'
import { Level, Log } from './log'

export interface Languages {
  readonly language?: string
  readonly languages?: string[]
}

export class SessionLanguage {
  static readonly TAG = /^([a-z]{2,3})-([A-Z]{2})$/

  static of(env: Env, system: readonly string[] = []): Languages {
    const tag = (locale: string) => {
      const base = locale.split(/[.@]/)[0]!.trim()
      if (!base) return null
      if (base === 'C' || base === 'POSIX') return 'en-US'
      return base.replace(/_/g, '-')
    }
    const locale = env['LC_ALL'] || env['LC_MESSAGES'] || env['LANG'] || ''
    const set = [...(env['LANGUAGE'] ? env['LANGUAGE'].split(':') : []), locale].map(tag).filter((t): t is string => !!t)
    const list = set.length ? set : system.map((t) => t.trim().replace(/^([a-z]{2,3})-[A-Z][a-z]{3}-([A-Z]{2}|\d{3})$/, '$1-$2')).filter(Boolean)
    const languages = [...new Set(list.flatMap((t) => (t.includes('-') ? [t, t.split('-')[0]!] : [t])))]
    return languages.length ? { language: languages[0]!, languages } : { language: 'en-US', languages: ['en-US', 'en'] }
  }

  static #locale(tag: string): string {
    const match = SessionLanguage.TAG.exec(tag)
    if (!match) throw Object.assign(new Error(`acde.session.setLanguage: ${JSON.stringify(tag)} is not a language-REGION tag such as zh-CN`), { status: 400 })
    return `${match[1]}_${match[2]}.UTF-8`
  }

  static #persist(locale: string, file: string): void {
    const lines = existsSync(file) ? readFileSync(file, 'utf8').split('\n').filter((l) => l.trim() && !/^\s*(LANG|LANGUAGE)=/.test(l)) : []
    mkdirSync(dirname(file), { recursive: true })
    writeFileSync(`${file}.tmp`, [`LANG=${locale}`, ...lines, ''].join('\n'))
    renameSync(`${file}.tmp`, file)
  }

  #current: Languages
  readonly #onChange: (languages: Languages) => void

  constructor(onChange: (languages: Languages) => void) {
    this.#current = SessionLanguage.of(process.env, sys().languages())
    this.#onChange = onChange
  }

  get current(): Languages {
    return this.#current
  }

  set(tag: string): void {
    const locale = SessionLanguage.#locale(tag)
    const next = SessionLanguage.of({ LANG: locale })
    const file = join(xdgHome('config'), 'locale.conf')
    try {
      SessionLanguage.#persist(locale, file)
    } catch (error) {
      Log.write(Level.Warn, 'acde_host', `${file} not written: ${(error as Error).message}`)
    }
    process.env['LANG'] = locale
    delete process.env['LANGUAGE']
    delete process.env['LC_ALL']
    delete process.env['LC_MESSAGES']
    if (this.#current.language === next.language) return
    Log.write(Level.Info, 'acde_host', `session language: ${next.language}`)
    this.#current = next
    this.#onChange(next)
  }
}

export interface Strings {
  readonly desktop: string
  readonly open: string
  readonly save: string
  readonly readFolder: string
  readonly writeFolder: string
  readonly places: Readonly<Record<string, string>>
}

interface RootLike {
  readonly source?: unknown
  readonly place?: unknown
  readonly label?: unknown
}

export class HostStrings {
  static readonly #EN: Strings = {
    desktop: 'Desktop',
    open: 'Open',
    save: 'Save',
    readFolder: 'Choose a folder to read',
    writeFolder: 'Choose a folder to read and change',
    places: { private: 'App data', desktop: 'Desktop', documents: 'Documents', downloads: 'Downloads', music: 'Music', pictures: 'Pictures', videos: 'Videos' },
  }

  static readonly #ZH: Strings = {
    desktop: '桌面',
    open: '打开',
    save: '保存',
    readFolder: '选择要读取的文件夹',
    writeFolder: '选择要授权读写的文件夹',
    places: { private: '应用数据', desktop: '桌面', documents: '文档', downloads: '下载', music: '音乐', pictures: '图片', videos: '视频' },
  }

  static of(language: string | null | undefined): Strings {
    return /^zh(?:-|$)/i.test(language ?? '') ? HostStrings.#ZH : HostStrings.#EN
  }

  static rootLabel(root: RootLike, language: string | null | undefined): string | null {
    const places = HostStrings.of(language).places
    const own = root.source === 'private' || root.source === 'desktop' || root.source === 'downloads'
    const key = root.source === 'place' ? root.place : own ? root.source : null
    return typeof key === 'string' ? (places[key] ?? null) : null
  }

  static roots<T>(roots: T, language: string | null | undefined): T {
    if (!Array.isArray(roots)) return roots
    return roots.map((root: RootLike) => {
      const label = root && typeof root === 'object' ? HostStrings.rootLabel(root, language) : null
      return label ? { ...root, label } : root
    }) as T
  }
}
