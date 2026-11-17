import { globals } from '../core/host'
import { Navigator } from '../dom/window'
import { KEY } from '../core/host'
import { Battery, BatteryManager, type BatteryNative } from './battery'
import { IdleDetector, type IdleNative } from './idle'
import { WakeLock, WakeLockSentinel, type WakeLockNative } from './wakelock'

export type DeviceNative = BatteryNative & WakeLockNative & IdleNative

export function installDevice(global: object, native: DeviceNative | undefined): void {
  if (!native) return
  const battery = new Battery(native)
  Object.defineProperty(Navigator.prototype, 'getBattery', {
    value: function getBattery(this: Navigator) {
      return battery.get()
    },
    writable: true,
    configurable: true,
  })
  globals(global, { BatteryManager })
  if (native.idle) {
    IdleDetector.install(native)
    globals(global, { IdleDetector })
  }
  if (!native.wakeLock) return
  const wakeLock = new WakeLock(KEY, native, () => (global as { document?: never }).document)
  Object.defineProperty(Navigator.prototype, 'wakeLock', {
    get: function wakeLock_(this: Navigator) {
      return wakeLock
    },
    enumerable: true,
    configurable: true,
  })
  globals(global, { WakeLock, WakeLockSentinel })
}
