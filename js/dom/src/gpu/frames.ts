const MAX_IN_FLIGHT = 2
const counters = new Set<(atLeast: number) => number>()

export const gpuFrames = {
  counted(counter: (atLeast: number) => number): void {
    counters.add(counter)
  },
  behind(): boolean {
    for (const count of counters) if (count(MAX_IN_FLIGHT) >= MAX_IN_FLIGHT) return true
    return false
  },
}
