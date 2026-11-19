import { Event, fire, type EventTarget } from '../core/event'
import { DOMException } from '../core/exception'
import { host, queueTask } from '../core/host'
import { activatedSince } from './activation'
import type { Element } from './element'
import { env } from './env'
import { focus } from './focus'
import { CONNECTED, documentId, lifecycle, wrap, type Wrapper } from './realm'

interface Pending {
  readonly element: Element
  readonly resolve: () => void
  readonly reject: (error: unknown) => void
}

export interface PointerLockOptions {
  readonly unadjustedMovement?: boolean
}

let locked: Element | null = null
let pending: Pending | null = null
let releasedByPage = false
let lostAt = -Infinity

function documentEvent(type: 'pointerlockchange' | 'pointerlockerror', then?: () => void): void {
  queueTask(() => {
    const document = wrap<EventTarget>(documentId)
    if (document) fire(document, new Event(type, { bubbles: true, composed: true }))
    then?.()
  })
}

function failed(reject: (error: unknown) => void, message: string, name: string): void {
  const error = new DOMException(message, name)
  documentEvent('pointerlockerror', () => reject(error))
}

export function pointerLockElement(): Element | null {
  return locked?.[CONNECTED] ? locked : null
}

export function requestPointerLock(element: Element, options?: PointerLockOptions): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    if (!element[CONNECTED]) return failed(reject, 'The element is not in the active document.', 'WrongDocumentError')
    if (options?.unadjustedMovement) return failed(reject, 'unadjustedMovement is not supported on this platform.', 'NotSupportedError')
    const access = env.pointerLock
    if (!access) return failed(reject, 'Pointer lock is not supported in this window.', 'NotSupportedError')
    if (!focus.windowActive) return failed(reject, 'The document is not focused.', 'WrongDocumentError')
    if (pending) return failed(reject, 'A pointer lock request is already pending.', 'InvalidStateError')
    if (!locked && !releasedByPage && !activatedSince(lostAt)) return failed(reject, 'Pointer lock needs a user gesture.', 'NotAllowedError')
    if (locked) {
      if (locked === element) return resolve()
      locked = element
      return documentEvent('pointerlockchange', resolve)
    }
    releasedByPage = false
    pending = { element, resolve, reject }
    access.lock()
  })
}

export function exitPointerLock(): void {
  if (!locked) return
  locked = null
  releasedByPage = true
  env.pointerLock?.unlock()
  documentEvent('pointerlockchange')
}

export function pointerLockSettled(granted: boolean): void {
  const request = pending
  pending = null
  if (!request) {
    if (granted) env.pointerLock?.unlock()
    return
  }
  if (!granted) return failed(request.reject, 'The window did not grant pointer lock.', 'NotAllowedError')
  if (!request.element[CONNECTED]) {
    env.pointerLock?.unlock()
    return failed(request.reject, 'The element left the document before pointer lock was granted.', 'WrongDocumentError')
  }
  locked = request.element
  documentEvent('pointerlockchange', request.resolve)
}

export function pointerLockLost(): void {
  if (!locked) return
  locked = null
  releasedByPage = false
  lostAt = host.now()
  documentEvent('pointerlockchange')
}

lifecycle.disconnected.push((node: Wrapper) => {
  if (node === (locked as unknown as Wrapper)) exitPointerLock()
})
