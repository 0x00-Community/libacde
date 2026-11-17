export {}
const scope = globalThis as unknown as { gc?: () => void; Bun?: { gc(force: boolean): void } }
scope.gc ??= () => scope.Bun?.gc(true)
