export interface LockUser {
  readonly name: string
  readonly fullName: string
  readonly avatar: string | null
  readonly confirm?: LockConfirm
  readonly status?: LockStatus
  readonly render?: 'gpu' | 'cpu'
}

export type LockRemember = 'once' | 'task' | 'scope' | 'until'

export interface LockConfirm {
  readonly id: string
  readonly agent: boolean
  readonly waiting: LockWaiting
}

export interface LockWaiting {
  readonly title: string
  readonly detail: string
  readonly expires: string
  readonly password: boolean
  readonly judge: 'supervisor' | 'user'
  readonly remember?: { readonly may: readonly LockRemember[]; readonly default: LockRemember; readonly max_days?: number }
  readonly tries_left?: number
}

export type LockFailure =
  | { readonly code: 'auth'; readonly status: 403 }
  | { readonly code: 'busy'; readonly status: 409 }
  | { readonly code: 'throttled'; readonly status: 429; readonly retryAfter: number }

export type LockError = Error & LockFailure

export interface AcdeLock {
  user(): Promise<LockUser>
  unlock(password: string, options?: LockUnlockOptions): Promise<void>
  cancel?(): Promise<void>
  status?(): Promise<LockStatus>
}

export interface LockUnlockOptions {
  readonly remember?: LockRemember
}

export interface LockStatus {
  readonly battery: LockBattery | null
  readonly network: LockNetwork | null
  readonly pending: number | null
}

export interface LockBattery {
  readonly percent: number
  readonly charging: boolean
}

export interface LockNetwork {
  readonly online: boolean
  readonly kind: 'wifi' | 'ethernet' | 'mobile' | 'other' | null
}
