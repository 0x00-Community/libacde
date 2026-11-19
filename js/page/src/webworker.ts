import { installWorkerScope, type WorkerSideNative } from '@dopame/acde-dom/worker'
import type { BackendTransport, HostNative } from '@dopame/acde-dom'

interface WorkerNatives {
  readonly host: HostNative
  readonly worker: WorkerSideNative
  readonly backend?: Omit<BackendTransport, 'port'>
}

export default function boot(natives: WorkerNatives): void {
  installWorkerScope(globalThis, natives.host, natives.worker, natives.backend)
}
