import type { ScreenshotEvent, WindowGrabEvent, WindowGrabLostEvent } from '@dopame/acde-api'
import type { DomNative, WebInternals } from '@dopame/acde-dom'
import { DragDrop, DragSource } from './dnd'
import type { HandOver } from './pagecalls'
import { InputMethod, type ImeEvent } from './ime'
import { ShellInput } from './keys'
import { WINDOW_EVENTS, type Native, type NativeSurface, type RawShellEvent, type ShellEvent, type WindowEvent } from './native'
import type { WindowList } from './remote'
import { mkdirSync } from './sys/fs'
import { userDir } from './sys/os'
import { join } from './sys/path'
import { TextEntry } from './text'
import { StartupTimeline } from './timeline'
import { Level, Log } from './log'

export class Host {
  static label = ''
  static readonly NOTICES = new Map<string, readonly string[]>([
    ['appschanged', ['apps:changed']],
    ['download', ['desktop:download']],
    ['notification', ['desktop:notification']],
    ['screenedge', ['desktop:edge']],
    ['shortcut', ['desktop:shortcut', 'window:shortcut']],
    ['permissionprompt', ['permissions:prompt']],
    ['permissionanswered', ['permissions:answered']],
  ])
  static readonly #WINDOW_EVENTS = new Set<string>(WINDOW_EVENTS)
  readonly #page: WebInternals
  readonly #dom: DomNative
  readonly #shellInput: ShellInput
  readonly #text: TextEntry
  readonly #dnd: DragDrop
  readonly #drag: DragSource
  readonly #ime: InputMethod | null
  readonly #windows: WindowList
  #surface: NativeSurface | null = null
  #closed = false
  #buttons = 0
  #move: RawShellEvent | null = null
  onActivation: (active: boolean) => void = () => {}
  onPanelMove: (id: number, extent: readonly [number, number, number, number]) => void = () => {}
  onGrab: (event: WindowGrabEvent | WindowGrabLostEvent) => void = () => {}
  onFrameButton: (key: string) => void = () => {}
  onState: (state: string, previous: string) => void = () => {}
  onFirstFrame: () => void = () => {}

  set handOver(handOver: HandOver) {
    this.#dnd.handOver = handOver
  }
  #framed = false

  constructor(page: WebInternals, dom: DomNative, scale: number, Ime: Native['Ime'], windows: WindowList) {
    this.#page = page
    this.#dom = dom
    this.#windows = windows
    this.#shellInput = new ShellInput((x, y) => {
      const id = dom.hitTest(x, y)
      return id === -1 ? null : id
    }, scale)
    this.#text = new TextEntry(page, dom)
    this.#dnd = new DragDrop(page)
    this.#drag = new DragSource(page, (data, icon) => this.#surface?.startDrag(data, icon), (element) => {
      const id = page.idOf(element)
      try {
        return id < 0 ? null : dom.capture(id, 1)
      } catch {
        return null
      }
    })
    this.#ime = Ime ? new InputMethod(() => new Ime((event) => this.#onIme(event))) : null
    if (this.#ime) this.#text.attachIme(this.#ime)
    page.input.onKeyDefault = (target, key, modifiers) => {
      const ctrl = !!(modifiers.ctrl || modifiers.meta)
      const lower = key.toLowerCase()
      if (ctrl && (lower === 'c' || lower === 'x' || lower === 'v')) {
        return void this.#text.clipboard(target as unknown as Element, lower).catch((error) => Host.report(error))
      }
      page.editing.editKey(target as never, key, ctrl, !!modifiers.shift)
    }
  }

  insertText(text: string): void {
    this.#text.commit(text)
    this.#text.report()
  }

  deliver(raw: readonly RawShellEvent[]): void {
    for (const event of raw) {
      try {
        if (Host.NOTICES.has(event.type)) for (const type of Host.NOTICES.get(event.type)!) this.#windows.notices.emit(type, event)
        else if (Host.#WINDOW_EVENTS.has(event.type)) this.#windows.deliver(event as unknown as WindowEvent)
      } catch (error) {
        Host.report(error)
      }
    }
  }

  #activation(raw: RawShellEvent): boolean {
    if (raw.type !== 'activate' && raw.type !== 'deactivate') return false
    const active = raw.type === 'activate'
    this.#text.surfaceActive = active
    this.#page.setActive(active)
    this.onActivation(active)
    return true
  }

  #handle(events: readonly ShellEvent[]): void {
    for (const event of events) {
      try {
        this.#one(event)
      } catch (error) {
        Host.report(error)
      }
    }
  }

  #one(event: ShellEvent): void {
    const page = this.#page
    switch (event.type) {
      case 'frame':
        return this.#frame(event.time)
      case 'panelmove':
        return this.onPanelMove(event.id, [event.x, event.y, event.width, event.height])
      case 'close':
      case 'exit':
        this.#closed = true
        return
      case 'dndenter':
      case 'dndmove':
      case 'dndleave':
      case 'dnddrop':
        return this.#dnd.handle(event)
      case 'dragend':
        if (this.#drag.active) {
          this.#drag.end(event.dropped)
          this.#buttons = 0
        }
        return
      case 'resize':
        this.#shellInput.scale = event.scale
        this.#dom.resize?.(event.width, event.height, event.scale)
        return page.resize()
      case 'keydown':
        if (event.key === 'PrintScreen') return void this.#screenshot()
    }
    this.#input(event)
  }

  #input(event: ShellEvent): void {
    const input = this.#page.input
    switch (event.type) {
      case 'pointermove': {
        const over = () => input.elementAt(event.target, event.x, event.y, true) as unknown as Element
        if ((this.#buttons & 1 || this.#drag.active) && this.#drag.move(over(), event.x, event.y, (source) => this.#cancelPointer(source))) return
        return input.pointer('pointermove', event.x, event.y, { target: event.target, modifiers: event.modifiers })
      }
      case 'pointerdown': {
        const button = event.button ?? 0
        this.#buttons |= 1 << button
        input.pointer('pointerdown', event.x, event.y, { target: event.target, button, modifiers: event.modifiers })
        if (button === 0) this.#drag.press(input.elementAt(event.target, event.x, event.y, true) as unknown as Element, event.x, event.y)
        return
      }
      case 'pointerup': {
        const button = event.button ?? 0
        this.#buttons &= ~(1 << button)
        if (button === 0 && this.#drag.release(input.elementAt(event.target, event.x, event.y, true) as unknown as Element, event.x, event.y)) return
        return input.pointer('pointerup', event.x, event.y, { target: event.target, button, modifiers: event.modifiers })
      }
      case 'pointerleave':
        if (this.#buttons || this.#drag.active) return
        return input.leave({ modifiers: event.modifiers })
      case 'wheel':
        input.wheel(event.x, event.y, event.deltaX, event.deltaY, { target: event.target, modifiers: event.modifiers })
        return
      case 'keydown':
      case 'keyup':
        if (!event.fromIme && this.#ime?.key(event.key, event.keycode ?? 0, event.modifiers, event.type === 'keyup')) return
        input.key(event.type, event.key, event.code, { modifiers: event.modifiers, repeat: event.repeat })
        return
    }
  }

  #cancelPointer(source: Element): void {
    const input = this.#page.input
    const captured = input.capture as unknown as Element | null
    input.capture = null
    const cancel = new PointerEvent('pointercancel', { bubbles: true, composed: true, pointerId: 1, pointerType: 'mouse', isPrimary: true })
    ;(captured ?? source).dispatchEvent(cancel)
    if (captured) captured.dispatchEvent(new PointerEvent('lostpointercapture', { pointerId: 1, pointerType: 'mouse', isPrimary: true }))
  }

  async #screenshot(): Promise<void> {
    const dir = userDir('PICTURES', 'Pictures')
    const stamp = new Date().toLocaleString('sv-SE').replace(/:/g, '.')
    const path = join(dir, `截图 ${stamp}.png`)
    let detail: ScreenshotEvent
    try {
      mkdirSync(dir, { recursive: true })
      const { width, height } = await this.#windows.screenshot(path)
      detail = { type: 'screenshot', path, width, height }
    } catch (error) {
      detail = { type: 'screenshot', path, error: (error as Error).message }
    }
    this.#windows.notices.emit('desktop:screenshot', detail)
  }

  #onIme(event: ImeEvent): void {
    try {
      if (event.type === 'preedit') this.#text.preedit(event.text)
      else if (event.type === 'panel') this.#text.panel(event)
      else if (event.type === 'commit') this.#text.commit(event.text)
      else if (event.type === 'deleteSurrounding') this.#text.deleteSurrounding(event.offset, event.length)
      else {
        const modifiers = InputMethod.modifiers(event.state)
        this.#input({ type: event.isRelease ? 'keyup' : 'keydown', key: InputMethod.key(event.keysym), code: '', modifiers, fromIme: true })
      }
    } catch (error) {
      Host.report(error)
    }
  }

  #frame(time: number): void {
    if (!this.#framed) {
      this.#framed = true
      StartupTimeline.mark('first frame')
      this.onFirstFrame()
    }
    this.#page.frame(time)
    this.#text.report()
  }

  async run(surface: NativeSurface): Promise<void> {
    StartupTimeline.mark('shell running')
    this.#surface = surface
    this.#text.attach(surface)
    surface.requestFrame()
    if (surface.waitThen) {
      const waitThen = surface.waitThen.bind(surface)
      return new Promise<void>((done) => {
        const turn = () => {
          try {
            this.#turn(surface)
          } finally {
            if (this.#closed) done()
            else waitThen(turn)
          }
        }
        turn()
      })
    }
    while (!this.#closed) {
      this.#turn(surface)
      await surface.wait()
    }
  }

  #turn(surface: NativeSurface): void {
    {
      const events: ShellEvent[] = []
      for (const raw of surface.pump()) {
        if (this.#activation(raw)) continue
        if (raw.type === 'grab') {
          this.onGrab({ kind: raw.key === 'keyboard' ? 'keyboard' : 'pointer', granted: raw.code === 1 })
          continue
        }
        if (raw.type === 'grablost') {
          this.onGrab({ kind: 'lost', reason: Host.lost(raw.title) })
          continue
        }
        if (raw.type === 'framebutton') {
          this.onFrameButton(raw.key ?? '')
          continue
        }
        if (raw.type === 'state') {
          this.onState(raw.state ?? 'normal', raw.previous ?? 'normal')
          continue
        }
        if (raw.type === 'imecommit' || raw.type === 'imepreedit') {
          this.#flushMove(events)
          this.#handle(events.splice(0))
          if (raw.type === 'imecommit') this.#text.commit(raw.key ?? '')
          else this.#text.preedit(raw.key ?? '')
          continue
        }
        if (raw.type === 'pointermove') {
          if (!this.#move) surface.requestFrame()
          this.#move = raw
          continue
        }
        this.#flushMove(events)
        const event = this.#shellInput.normalize(raw)
        if (event) events.push(event)
      }
      this.#handle(events)
    }
  }

  #flushMove(events: ShellEvent[]): void {
    const move = this.#move
    if (!move) return
    this.#move = null
    const event = this.#shellInput.normalize(move)
    if (event) events.push(event)
  }

  close(): void {
    this.#ime?.close()
  }

  static lost(reason: string | undefined): WindowGrabLostEvent['reason'] {
    return reason === 'esc' || reason === 'other_window' || reason === 'focus_lost' ? reason : 'denied'
  }

  static report(error: unknown): void {
    const text = error instanceof Error ? (error.stack ?? error.message) : String(error)
    Log.write(Level.Error, 'acde_host', `${Host.label ? `[${Host.label}] ` : ''}page error: ${text.split('\n').slice(0, 6).join('\n  ')}`)
  }
}
