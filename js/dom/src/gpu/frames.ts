import { host } from '../core/host'

// Frames still on the GPU. WebGPU and WebGL canvases share one device, so they share this count: a frame that presented
// either kind registers the promise for its GPU work, and requestAnimationFrame callbacks wait while two frames are still
// in flight, as browsers keep the page at most a frame or two ahead of the GPU. Without it a page whose GPU work takes
// longer than a frame queues frames until the driver's command buffer budget blocks it mid-frame, half a second behind.
const inFlight = new Set<{ readonly at: number }>()
const MAX_IN_FLIGHT = 2
// A frame whose completion never arrives (a lost device) stops counting after this long, so frames are not held forever.
const STALE_MS = 1000

export const gpuFrames = {
  submitted(done: Promise<void>): void {
    const entry = { at: host.now() }
    inFlight.add(entry)
    const end = () => inFlight.delete(entry)
    done.then(end, end)
  },
  behind(): boolean {
    if (inFlight.size < MAX_IN_FLIGHT) return false
    const now = host.now()
    let count = 0
    for (const { at } of inFlight) if (now - at < STALE_MS) count++
    return count >= MAX_IN_FLIGHT
  },
}
