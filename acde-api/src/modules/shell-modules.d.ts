declare module 'acde:windows' {
  const windows: import('../types/page').AcdeWindows
  export { windows, windows as default }
}
declare module 'acde:packages' {
  const packages: import('../types/page').AcdePackages
  export { packages, packages as default }
}
declare module 'acde:session' {
  const session: import('../types/page').AcdeSession
  export { session, session as default }
}
declare module 'acde:desktops' {
  const desktops: import('../types/page').AcdeDesktops
  export { desktops, desktops as default }
}
declare module 'acde:inspect' {
  const inspect: import('../types/page').AcdeInspect
  export { inspect, inspect as default }
}
declare module 'acde:desktop' {
  const desktop: import('../types/page').AcdeDesktop
  export { desktop, desktop as default }
}
declare module 'acde:permissions' {
  const permissions: import('../types/permissions').AcdePermissions
  export { permissions, permissions as default }
}
declare module 'acde:setDefault' {
  const setDefault: import('../types/page').AcdeOpenFiles['setDefault']
  export { setDefault, setDefault as default }
}
