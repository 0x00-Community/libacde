export interface AcdeSession {
  own(): boolean
  lock(): Promise<void>
  logout(): Promise<void>
  reboot(): Promise<void>
  poweroff(): Promise<void>
  setLanguage(tag: string): Promise<void>
  others(): Promise<SessionOther[]>
  switchUser(): Promise<void>
}

export interface SessionOther {
  readonly user: string
  readonly uid: number
  readonly place: string | null
  readonly remote: boolean
}
