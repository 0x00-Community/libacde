import type { CapabilityStatus } from '@dopame/acde-api'
import type { CaptureNative, DeviceNative } from '@dopame/acde-dom'
import { FileAccess } from '../files'
import { Permissions, type Approval } from './permissions'

export interface CaptureAccess {
  supported(capability: string): boolean
  request(capability: string): Promise<CapabilityStatus>
}

type CaptureName = 'camera' | 'microphone'

type CaptureDevice = Awaited<ReturnType<CaptureNative['devices']>>[number]

export class Permits {
  static readonly MODULES: Readonly<Record<string, readonly string[]>> = { launch: ['launch', 'slots'], terminal: ['terminal'], network: ['network'] }
  readonly #approvals: Map<string, Approval>
  readonly #label: string
  readonly #watchers = new Set<(capability: string) => void>()

  constructor(approvals: Readonly<Record<string, Approval>> | undefined, label: string) {
    this.#approvals = new Map(Object.entries(approvals ?? {}))
    this.#label = label
  }

  approval(capability: string): Approval {
    return this.#approvals.get(capability) ?? 'granted'
  }

  set(capability: string, approval: Approval): void {
    if (this.#approvals.get(capability) === approval) return
    this.#approvals.set(capability, approval)
    for (const watcher of this.#watchers) watcher(capability)
  }

  usable(module: string): boolean {
    const gates = this.#gates(module)
    return !gates.length || gates.some((name) => this.approval(name) === 'granted')
  }

  reason(module: string): string {
    const reasons = this.#gates(module).flatMap((name) => {
      const approval = this.approval(name)
      return approval === 'granted' ? [] : [Permissions.reason(name, approval)]
    })
    return `acde:${module}: ${reasons.join('; ')}`
  }

  guard<T extends object>(module: string, value: T): T {
    if (module === 'fs' || module === 'office') {
      return FileAccess.guard(value, (capability) => this.#approvals.get(capability) ?? null, this.#label, module === 'office')
    }
    if (!Permits.MODULES[module] || module === 'launch') return value
    return new Proxy(value, {
      get: (target, key, receiver) => {
        const member = Reflect.get(target, key, receiver) as unknown
        if (typeof member !== 'function') return member
        return (...args: unknown[]) => {
          if (!this.usable(module)) throw this.#error(this.#gates(module)[0] ?? module)
          return (member as (...a: unknown[]) => unknown).apply(target, args)
        }
      },
    })
  }

  device(native: DeviceNative, access: CaptureAccess): DeviceNative {
    const name = 'idleDetection'
    const state = (): 'granted' | 'denied' | 'prompt' => {
      const approval = this.approval(name)
      if (approval === 'granted') return 'granted'
      return approval === 'denied' || !access.supported(name) ? 'denied' : 'prompt'
    }
    return {
      ...native,
      idlePermission: state,
      requestIdlePermission: async () => {
        if (state() !== 'prompt') return state() === 'granted'
        const status = await access.request(name).catch(() => null)
        if (status?.state === 'available') this.set(name, 'granted')
        else if (status?.state === 'denied') this.set(name, 'denied')
        return state() === 'granted'
      },
    }
  }

  capture(native: CaptureNative, access: CaptureAccess): CaptureNative {
    const gate = (kind: string): CaptureName => (kind === 'audioinput' || kind === 'microphone' ? 'microphone' : 'camera')
    const state = (name: CaptureName): 'granted' | 'denied' | 'prompt' => {
      const approval = this.approval(name)
      if (approval === 'granted') return 'granted'
      return approval === 'denied' || !access.supported(name) ? 'denied' : 'prompt'
    }
    const allowed = (kind: string) => state(gate(kind)) === 'granted'
    return {
      devices: (full) => native.devices(full).then((devices) => (full ? devices : Permits.#listed(devices, allowed))),
      permission: (kind) => (allowed(kind) ? native.permission(kind) : state(kind)),
      request: async (kind) => {
        if (state(kind) !== 'prompt') return allowed(kind)
        const status = await access.request(kind).catch(() => null)
        if (status?.state === 'available') this.set(kind, 'granted')
        else if (status?.state === 'denied') this.set(kind, 'denied')
        return allowed(kind)
      },
      onPermission: (callback) => {
        this.#watchers.add((capability) => (capability === 'camera' || capability === 'microphone') && callback())
      },
      open: (kind, ...rest) => (allowed(kind) ? native.open(kind, ...rest) : Promise.reject(this.#notAllowed(gate(kind)))),
      close: (source) => native.close(source),
      onEnded: (callback) => native.onEnded(callback),
    }
  }

  static #listed(devices: CaptureDevice[], allowed: (kind: string) => boolean): CaptureDevice[] {
    const kinds = new Set<string>()
    return devices.flatMap((device) => {
      if (allowed(device.kind)) return [device]
      if (kinds.has(device.kind)) return []
      kinds.add(device.kind)
      return [{ ...device, deviceId: '', label: '', groupId: '', facingMode: null }]
    })
  }

  #notAllowed(capability: string): Error {
    const { message, ...fields } = this.#error(capability)
    return Object.assign(new Error(`NotAllowedError: ${message}`), fields)
  }

  #gates(module: string): string[] {
    return (Permits.MODULES[module] ?? []).filter((name) => this.#approvals.has(name))
  }

  #error(capability: string): Error {
    const approval = this.approval(capability)
    return Permissions.error(this.#label, capability, approval === 'granted' ? 'denied' : approval)
  }
}
