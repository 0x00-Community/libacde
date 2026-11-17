import { existsSync, mkdirSync, readdirSync, rmSync, stat, writeFileSync } from './sys/fs'
import { homedir, xdgHome } from './sys/os'
import { join } from './sys/path'
import { pathToFileURL } from './sys/url'
import { process } from './sys/process'
import { PageSource } from './page'

export class OfficePage {
  static dir(): string {
    const snap = join(homedir(), 'snap', 'firefox', 'common')
    return existsSync(snap) ? join(snap, 'acde-office') : join(process.env['XDG_RUNTIME_DIR'] || xdgHome('cache'), 'acde', 'office')
  }

  static write(setup: { api: string; config: unknown }, theme: 'light' | 'dark'): string {
    const config = structuredClone(setup.config) as { document?: { key?: string; title?: string }; editorConfig?: { customization?: Record<string, unknown> } }
    if (config.editorConfig?.customization) config.editorConfig.customization['uiTheme'] = theme === 'dark' ? 'theme-night' : 'theme-white'
    const key = String(config.document?.key ?? 'document').replace(/[^\w.-]/g, '_')
    const json = JSON.stringify(config).replace(/</g, '\\u003c')
    const title = PageSource.escape(String(config.document?.title ?? '文稿'))
    const html = `<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8"><title>${title}</title>
<style>html,body,#editor{margin:0;height:100%;overflow:hidden}</style>
<script src="${setup.api.replace(/"/g, '&quot;')}"></script></head>
<body><div id="editor"></div>
<script>
if (window.DocsAPI) new DocsAPI.DocEditor('editor', ${json});
else document.body.textContent = 'Document Server 的编辑器脚本没有加载 (${setup.api.replace(/['\\<]/g, '')})';
</script></body></html>
`
    const dir = OfficePage.dir()
    mkdirSync(dir, { recursive: true, mode: 0o700 })
    for (const name of readdirSync(dir)) {
      const old = join(dir, name)
      const mtime = stat(old)?.mtimeMs
      if (name.endsWith('.html') && mtime !== undefined && Date.now() - mtime > 3600_000) rmSync(old, { force: true })
    }
    const file = join(dir, `${key}.html`)
    writeFileSync(file, html, { mode: 0o600 })
    return pathToFileURL(file).href
  }
}
