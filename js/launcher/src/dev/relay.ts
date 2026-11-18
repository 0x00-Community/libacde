export interface DevRelay {
  respond(id: number, ok: boolean, payload: string): void
  devEvent(json: string): number
  announce(port: number): void
  inject(event: object): void
}

export function managerRelay(manager: { send?(frame: object): void } | null): DevRelay | null {
  const send = manager?.send?.bind(manager)
  if (!send) return null
  return {
    respond: (id, ok, payload) => send({ devresponse: { id, ok, payload } }),
    devEvent: (json) => {
      send({ devevent: json })
      return 1
    },
    announce: (port) => send({ devtools: { port } }),
    inject: (event) => send({ devinject: event }),
  }
}
