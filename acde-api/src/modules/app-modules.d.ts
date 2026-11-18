declare module 'acde:fs' {
  const fs: import('../types/page').AcdeFs
  export { fs, fs as default }
}
declare module 'acde:office' {
  const office: import('../types/page').AcdeOffice
  export { office, office as default }
}
declare module 'acde:terminal' {
  const terminal: import('../types/page').AcdeTerminals
  export { terminal, terminal as default }
}
declare module 'acde:power' {
  const power: import('../types/page').AcdePower
  export { power, power as default }
}
declare module 'acde:input' {
  const input: import('../types/page').AcdeInput
  export { input, input as default }
}
declare module 'acde:display' {
  const display: import('../types/page').AcdeDisplay
  export { display, display as default }
}

declare module 'acde:devices' {
  const devices: import('../types/page').AcdeDevices
  export { devices, devices as default }
}

declare module 'acde:audio' {
  const audio: import('../types/page').AcdeAudio
  export { audio, audio as default }
}

declare module 'acde:bluetooth' {
  const bluetooth: import('../types/page').AcdeBluetooth
  export { bluetooth, bluetooth as default }
}

declare module 'acde:system' {
  const system: import('../types/page').AcdeSystem
  export { system, system as default }
}

declare module 'acde:network' {
  const network: import('../types/page').AcdeNetwork
  export { network, network as default }
}
declare module 'acde:launch' {
  const launch: import('../types/page').AcdeLaunch
  export { launch, launch as default }
}
declare module 'acde:apps' {
  const apps: import('../types/page').AcdeAppList
  export { apps, apps as default }
}
declare module 'acde:open' {
  const open: import('../types/page').AcdeOpen
  export { open, open as default }
}
declare module 'acde:openPath' {
  const openPath: import('../types/page').AcdeOpenFiles['openPath']
  export { openPath, openPath as default }
}
declare module 'acde:handlers' {
  const handlers: import('../types/page').AcdeOpenFiles['handlers']
  export { handlers, handlers as default }
}
declare module 'acde:window' {
  const self: import('../types/page').AcdeWindow
  export { self as window, self as default }
}
declare module 'acde:capabilities' {
  const capabilities: import('../types/capabilities').AcdeCapabilities
  export { capabilities, capabilities as default }
}
declare module 'acde:*' {
  const service: unknown
  export default service
}
