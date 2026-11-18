export type ApprovalCapability = 'camera' | 'microphone' | 'idleDetection'

export type FolderCapability = `fs.folder-${string}`

export interface PermissionPrompt {
  readonly id: number
  readonly app: string
  readonly label: string
  readonly capability: ApprovalCapability
  readonly reason: 'request'
}

export interface PermissionPromptEvent extends PermissionPrompt {
  readonly type: 'permissionprompt'
}

export interface PermissionAnsweredEvent {
  readonly type: 'permissionanswered'
  readonly id: number
  readonly app: string
  readonly capability: ApprovalCapability
  readonly allowed: boolean
}

export interface PermissionEventMap {
  readonly prompt: PermissionPromptEvent
  readonly answered: PermissionAnsweredEvent
}

export interface PermissionDecision {
  readonly app: string
  readonly label: string | null
  readonly capability: ApprovalCapability | FolderCapability
  readonly decision: 'granted' | 'denied'
  readonly folder?: { readonly label: string; readonly exists: boolean }
}

export interface AcdePermissions {
  pending(): Promise<PermissionPrompt[]>
  answer(id: number, allow: boolean): Promise<void>
  list(app?: string): Promise<PermissionDecision[]>
  revoke(app: string, capability: ApprovalCapability | FolderCapability): Promise<boolean>
  on<K extends keyof PermissionEventMap>(type: K, handler: (event: PermissionEventMap[K]) => void): () => boolean
}

export interface AcdePermissionError extends Error {
  readonly status: 403
  readonly code: 'denied' | 'not_asked' | 'asking'
  readonly capability: string
}
