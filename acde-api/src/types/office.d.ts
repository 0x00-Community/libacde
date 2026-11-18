import type { FinderRoot } from './page'

export type OfficeProblem = 'unreachable' | 'unhealthy' | 'secret' | 'rejected' | 'bridge'

export type OfficeServer =
  | { readonly ok: true; readonly url: string; readonly version: string | null; readonly problem: null; readonly message: null }
  | { readonly ok: false; readonly url: string; readonly version: string | null; readonly problem: OfficeProblem; readonly message: string }

export type OfficeSaveState = 'open' | 'saving' | 'saved' | 'failed' | 'conflict' | 'closed'

export interface OfficeStatus {
  readonly key: string
  readonly state: OfficeSaveState
  readonly savedAt: number | null
  readonly expectedMs: number
  readonly message: string | null
  readonly conflictPath: string | null
  readonly editors: number
}

export type OfficeDocumentType = 'word' | 'cell' | 'slide' | 'pdf'

export interface OfficeFile {
  readonly root: string
  readonly rootLabel: string
  readonly path: string
  readonly name: string
  readonly ext: string
  readonly documentType: OfficeDocumentType
  readonly size: number
  readonly modifiedMs: number
  readonly editable: boolean
  readonly readOnlyReason: string | null
}

export interface OfficeEntry {
  readonly name: string
  readonly kind: 'directory' | 'document'
  readonly size: number
  readonly modifiedMs: number
  readonly ext: string
}

export interface OfficeEditorConfig {
  readonly type: 'desktop'
  readonly width: string
  readonly height: string
  readonly documentType: OfficeDocumentType
  readonly document: {
    readonly fileType: string
    readonly key: string
    readonly title: string
    readonly url: string
    readonly permissions: Readonly<Record<
      'edit' | 'comment' | 'review' | 'fillForms' | 'modifyFilter' | 'modifyContentControl' | 'download' | 'print' | 'copy' | 'chat' | 'protect',
      boolean
    >>
  }
  readonly editorConfig: {
    readonly mode: 'edit' | 'view'
    readonly lang: string
    readonly region: string
    readonly user: { readonly id: string; readonly name: string }
    readonly callbackUrl?: string
    readonly coEditing: { readonly mode: 'fast' | 'strict'; readonly change: boolean }
    readonly customization: {
      readonly autosave: boolean
      readonly forcesave: boolean
      readonly compactHeader: boolean
      readonly toolbarHideFileName: boolean
      readonly hideRightMenu: boolean
      readonly integrationMode: 'embed'
      readonly features: {
        readonly spellcheck: { readonly mode: boolean; readonly change: boolean }
        readonly featuresTips: boolean
        readonly tabBackground: { readonly mode: 'toolbar' | 'header'; readonly change: boolean }
      }
      readonly feedback: boolean
      readonly help: boolean
      readonly plugins: boolean
      readonly macros: boolean
      readonly mentionShare: boolean
      readonly close: { readonly visible: boolean; readonly text: string }
    }
  }
  readonly token: string
}

export interface OfficeEditor {
  readonly api: string
  readonly key: string
  readonly config: OfficeEditorConfig
}

export interface OfficeState {
  readonly base: string
  readonly apps: readonly []
  readonly roots: readonly FinderRoot[]
  readonly root: string | null
  readonly dir: string
  readonly entries: readonly OfficeEntry[]
  readonly truncated: boolean
  readonly file: OfficeFile | null
  readonly editor: OfficeEditor | null
  readonly status: OfficeStatus | null
  readonly server: OfficeServer
  readonly error: string | null
}

export interface AcdeOffice {
  server(): Promise<OfficeServer>
  state(root?: string, dir?: string, path?: string): Promise<OfficeState>
  state(handle: FileSystemFileHandle): Promise<OfficeState>
  status(key: string): Promise<OfficeStatus>
  save(body: { key: string }): Promise<{ readonly result: 'requested' | 'no_changes'; readonly status: OfficeStatus }>
  'new'(body: { root: string; kind: 'docx' | 'xlsx' | 'pptx'; dir?: string; name?: string }): Promise<{ readonly path: string }>
  editorPage(setup: Pick<OfficeEditor, 'api' | 'config'>, theme: 'light' | 'dark'): Promise<string>
}
