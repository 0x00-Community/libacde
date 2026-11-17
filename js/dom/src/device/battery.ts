import { defineHandlers, Event, EventTarget, fire } from '../core/event'
import { getters, KEY, tag } from '../core/host'

export interface BatteryStatus {
  readonly charging: boolean
  readonly level: number
  readonly chargingTime: number | null
  readonly dischargingTime: number | null
}

export interface BatteryNative {
  battery(): Promise<BatteryStatus>
  watchBattery(callback: (status: BatteryStatus) => void): void
}

const PLUGGED: BatteryStatus = { charging: true, level: 1, chargingTime: 0, dischargingTime: null }
const status = new WeakMap<BatteryManager, BatteryStatus>()
const seconds = (value: number | null): number => value ?? Infinity

export class BatteryManager extends EventTarget {
  constructor(key: symbol) {
    if (key !== KEY) throw new TypeError('Illegal constructor')
    super()
    status.set(this, PLUGGED)
  }

  static update(manager: BatteryManager, next: BatteryStatus): void {
    const before = status.get(manager) ?? PLUGGED
    status.set(manager, next)
    const changed: [keyof BatteryStatus, string][] = [
      ['charging', 'chargingchange'],
      ['chargingTime', 'chargingtimechange'],
      ['dischargingTime', 'dischargingtimechange'],
      ['level', 'levelchange'],
    ]
    for (const [field, type] of changed) {
      if (before[field] !== next[field]) fire(manager, new Event(type))
    }
  }
}
getters(BatteryManager.prototype, {
  charging(this: BatteryManager) {
    return (status.get(this) ?? PLUGGED).charging
  },
  level(this: BatteryManager) {
    return (status.get(this) ?? PLUGGED).level
  },
  chargingTime(this: BatteryManager) {
    return seconds((status.get(this) ?? PLUGGED).chargingTime)
  },
  dischargingTime(this: BatteryManager) {
    return seconds((status.get(this) ?? PLUGGED).dischargingTime)
  },
})
defineHandlers(BatteryManager.prototype, ['chargingchange', 'chargingtimechange', 'dischargingtimechange', 'levelchange'])
tag(BatteryManager)

export class Battery {
  readonly #native: BatteryNative
  #manager: Promise<BatteryManager> | null = null

  constructor(native: BatteryNative) {
    this.#native = native
  }

  get(): Promise<BatteryManager> {
    this.#manager ??= this.#native.battery().then((first) => {
      const manager = new BatteryManager(KEY)
      BatteryManager.update(manager, first)
      this.#native.watchBattery((next) => BatteryManager.update(manager, next))
      return manager
    })
    return this.#manager
  }
}
