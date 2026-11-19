import { host, illegal, KEY, tag } from '../core/host'

const TRANSIENT_MS = 5000

let last = -Infinity
let ever = false

export function activate(): void {
  last = host.now()
  ever = true
}

export function transientActivation(): boolean {
  return host.now() - last <= TRANSIENT_MS
}

export function activatedSince(time: number): boolean {
  return last > time && transientActivation()
}

export class UserActivation {
  constructor(token?: unknown) {
    illegal(token)
  }

  get hasBeenActive(): boolean {
    return ever
  }

  get isActive(): boolean {
    return transientActivation()
  }
}
tag(UserActivation)

export const userActivation = new UserActivation(KEY)
