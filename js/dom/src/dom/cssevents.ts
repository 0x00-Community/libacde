import { fire, hooks } from '../core/event'
import { AnimationEvent, TransitionEvent } from './events'
import type { AnimationEvents } from '../native'
import { dom, existing, wrap } from './realm'

const TYPES = new Set([
  'animationstart', 'animationiteration', 'animationend', 'animationcancel',
  'transitionrun', 'transitionstart', 'transitionend', 'transitioncancel',
  'webkitanimationstart', 'webkitanimationiteration', 'webkitanimationend', 'webkittransitionend',
])
const OWN = 'acde-animation-'

let listened = false
let wake = (): void => {}

hooks.listen = (type) => {
  if (listened || !TYPES.has(type.toLowerCase())) return
  listened = true
  dom.wantAnimationEvents()
  wake()
}

export function cssEventsWanted(): boolean {
  return listened
}

export function onCssEventsWanted(callback: () => void): void {
  wake = callback
}

export function deliverCssEvents(taken: AnimationEvents | null): boolean {
  if (!taken) return false
  const { events } = taken
  for (const record of events) {
    if (!record.transition && record.name.startsWith(OWN)) continue
    const target = existing(record.target) ?? wrap(record.target)
    if (!target) continue
    const init = { bubbles: true, elapsedTime: record.elapsedTime, pseudoElement: record.pseudoElement }
    const event = record.transition
      ? new TransitionEvent(record.type, { ...init, propertyName: record.name })
      : new AnimationEvent(record.type, { ...init, animationName: record.name })
    fire(target as never, event)
  }
  return events.length > 0
}
