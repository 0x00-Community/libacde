import type { Platform, PlatformSupport, RunningMode } from '../acde-api/src/index'

const HOST_WINDOW_ONLY = ['slots']
const WINDOWS_MISSING = [
  'network', 'system', 'bluetooth', 'audio', 'devices', 'power', 'input', 'display', 'idleDetection', 'office', 'camera', 'microphone',
  ...HOST_WINDOW_ONLY,
]
const MACOS_MISSING = [
  'network', 'system', 'bluetooth', 'audio', 'devices', 'power', 'input', 'display', 'idleDetection', 'office', ...HOST_WINDOW_ONLY,
]
const FULL = { backdrop: 'full' } as const

export const RUNNING_MODES: readonly RunningMode[] = ['session', 'desktop', 'standalone']

export const PLATFORM_SUPPORT: Readonly<Record<Platform, Partial<Readonly<Record<RunningMode, PlatformSupport>>>>> = {
  linux: {
    session: { unsupported: [], window: FULL },
    desktop: { unsupported: HOST_WINDOW_ONLY, window: FULL },
    standalone: { unsupported: HOST_WINDOW_ONLY, window: FULL },
  },
  win32: {
    desktop: { unsupported: WINDOWS_MISSING, window: FULL },
    standalone: { unsupported: [...WINDOWS_MISSING, 'notifications'], window: FULL },
  },
  darwin: {
    desktop: { unsupported: MACOS_MISSING, window: FULL },
    standalone: { unsupported: MACOS_MISSING, window: FULL },
  },
}

export function platformSupport(platform: string, mode: RunningMode): PlatformSupport | null {
  return PLATFORM_SUPPORT[platform as Platform]?.[mode] ?? null
}
