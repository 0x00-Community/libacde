export interface LockUser {
  readonly name: string
  readonly fullName: string
  readonly avatar: string | null
}

export class FakeLock {
  static readonly USER: LockUser = { name: 'dev', fullName: 'acdev developer', avatar: null }
  static readonly PASSWORD = 'right-password'
  static readonly RETRY_AFTER = 3

  answer(method: string, args: readonly unknown[]): unknown {
    if (method === 'user') return { ...FakeLock.USER }
    if (method === 'unlock') return FakeLock.#unlock(args[0])
    throw Object.assign(new Error(`acdev: lock.${method} does not exist (user, unlock)`), { status: 404, code: 'not_found' })
  }

  static #unlock(password: unknown): undefined {
    if (password === FakeLock.PASSWORD) return undefined
    if (password === 'busy-password') throw Object.assign(new Error('acdev: another unlock is still being checked'), { status: 409, code: 'busy' })
    if (password === 'throttled-password') {
      const message = `acdev: too many wrong passwords; try again in ${FakeLock.RETRY_AFTER} s`
      throw Object.assign(new Error(message), { status: 429, code: 'throttled', retryAfter: FakeLock.RETRY_AFTER })
    }
    throw Object.assign(new Error('acdev: wrong password'), { status: 403, code: 'auth' })
  }
}
