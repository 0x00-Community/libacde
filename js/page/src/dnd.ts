import type { WebInternals } from '@dopame/acde-dom'
import type { HandOver } from './pagecalls'
import { readFileSync, stat } from './sys/fs'
import { basename } from './sys/path'

type PageWindow = Window & typeof globalThis

export type DragInput =
  | { type: 'dndenter'; target: number | null; x: number; y: number; mime: readonly string[] }
  | { type: 'dndmove'; target: number | null; x: number; y: number }
  | { type: 'dnddrop'; target: number | null; x: number; y: number; data: Readonly<Record<string, string>>; files?: readonly string[] }
  | { type: 'dndleave' }

export class DragDrop {
  static readonly FILE_LIMIT = 64 * 1024 * 1024

  readonly #window: PageWindow
  readonly #page: WebInternals
  #mime: readonly string[] = []
  #target: Element | null = null
  #taken = false
  handOver: HandOver | null = null

  constructor(page: WebInternals) {
    this.#window = page.window as unknown as PageWindow
    this.#page = page
  }

  handle(event: DragInput): void {
    if (event.type === 'dndleave') return this.#leave()
    if (event.type === 'dndenter') this.#mime = event.mime
    const target = this.#element(event.target, event.x, event.y)
    this.#over(target, event.x, event.y)
    if (event.type === 'dnddrop') this.#drop(target, event.x, event.y, event.data, event.files ?? [])
  }

  #element(target: number | null, x: number, y: number): Element {
    return this.#page.input.elementAt(target, x, y, true) as unknown as Element
  }

  #transfer(data: Readonly<Record<string, string>> | null, files: readonly string[] = []): DataTransfer {
    const transfer = new this.#window.DataTransfer() as unknown as DataTransfer
    for (const [mime, text] of Object.entries(data ?? {})) transfer.setData(mime, text)
    for (const path of files) this.#add(transfer, path)
    const types = [...this.#mime, ...(this.#mime.includes('text/uri-list') ? ['Files'] : [])]
    Object.defineProperty(transfer, 'types', { value: Object.freeze(types), configurable: true })
    transfer.effectAllowed = 'all'
    return transfer
  }

  static fire(window: PageWindow, target: Element, type: string, x: number, y: number, transfer: DataTransfer, related: Element | null = null): boolean {
    const cancelable = !/^drag(leave|end)$/.test(type)
    const init = { bubbles: true, cancelable, composed: true, clientX: x, clientY: y, screenX: x, screenY: y, relatedTarget: related }
    const event = new window.MouseEvent(type, init as never)
    Object.defineProperty(event, 'dataTransfer', { value: transfer })
    return target.dispatchEvent(event as never)
  }

  static over(window: PageWindow, from: Element | null, target: Element, x: number, y: number, transfer: DataTransfer): boolean {
    if (target !== from) {
      DragDrop.fire(window, target, 'dragenter', x, y, transfer, from)
      if (from?.isConnected) DragDrop.fire(window, from, 'dragleave', x, y, transfer, target)
    }
    transfer.dropEffect = 'none'
    return !DragDrop.fire(window, target, 'dragover', x, y, transfer)
  }

  #over(target: Element, x: number, y: number): void {
    this.#taken = DragDrop.over(this.#window, this.#target, target, x, y, this.#transfer(null))
    this.#target = target
  }

  #leave(): void {
    if (this.#target?.isConnected) DragDrop.fire(this.#window, this.#target, 'dragleave', 0, 0, this.#transfer(null))
    this.#reset()
  }

  #reset(): void {
    this.#target = null
    this.#taken = false
    this.#mime = []
  }

  #drop(target: Element, x: number, y: number, data: Readonly<Record<string, string>>, files: readonly string[]): void {
    if (this.#taken) DragDrop.fire(this.#window, target, 'drop', x, y, this.#transfer(data, files))
    else if (target.isConnected) DragDrop.fire(this.#window, target, 'dragleave', x, y, this.#transfer(null))
    this.#reset()
  }

  #add(transfer: DataTransfer, path: string): void {
    try {
      const entry = stat(path)
      if (!entry || entry.isSymbolicLink || !(entry.isFile || entry.isDirectory)) return
      const handle = this.handOver?.handle(path, entry.isDirectory ? 'directory' : 'file', false) ?? null
      const small = entry.isFile && entry.size <= DragDrop.FILE_LIMIT
      const bytes = small ? (readFileSync(path) as Uint8Array<ArrayBuffer>) : null
      const name = basename(path)
      const type = entry.isFile ? this.#page.env.fileType(name) : ''
      const file = bytes ? (new this.#window.File([bytes], name, { type, lastModified: entry.mtimeMs }) as unknown as File) : null
      if (handle) this.#page.handles.transfer(transfer, file, handle)
      else if (file) transfer.items.add(file)
    } catch {
    }
  }
}

export interface DragIcon {
  readonly width: number
  readonly height: number
  readonly rgba?: Uint8Array
  readonly picture?: number
  readonly hotX: number
  readonly hotY: number
}

interface ElementPicture {
  readonly x: number
  readonly y: number
  readonly pixelWidth: number
  readonly pixelHeight: number
  readonly scale: number
  readonly picture: number
}

export class DragSource {
  static readonly SLOP = 4

  readonly #window: PageWindow
  readonly #start: (data: Record<string, string>, icon: DragIcon) => void
  readonly #picture: (element: Element) => ElementPicture | null
  #pressed: { source: Element; x: number; y: number } | null = null
  #drag: { source: Element; transfer: DataTransfer; target: Element | null; taken: boolean } | null = null

  constructor(page: WebInternals, start: (data: Record<string, string>, icon: DragIcon) => void, picture: (element: Element) => ElementPicture | null) {
    this.#window = page.window as unknown as PageWindow
    this.#start = start
    this.#picture = picture
  }

  static #file: DragIcon | null = null

  static get fileIcon(): DragIcon {
    if (DragSource.#file) return DragSource.#file
    const width = 40, height = 48, fold = 12, rgba = new Uint8Array(width * height * 4)
    const set = (x: number, y: number, [r, g, b, a]: readonly number[]) => rgba.set([r!, g!, b!, a!], (y * width + x) * 4)
    for (let y = 0; y < height; y++) {
      for (let x = 0; x < width; x++) {
        const inside = !(x >= width - fold && y < fold - (width - 1 - x))
        if (!inside) continue
        const edge = x === 0 || y === height - 1 || x === width - 1 || y === 0 || (x >= width - fold && y === fold - (width - 1 - x))
        const line = y >= 20 && y <= 38 && (y - 20) % 6 === 0 && x >= 8 && x <= 31
        const flap = x >= width - fold && y < fold
        set(x, y, edge ? [149, 92, 254, 255] : flap ? [221, 205, 255, 255] : line ? [196, 166, 255, 255] : [255, 255, 255, 235])
      }
    }
    return (DragSource.#file = { width, height, rgba, hotX: width / 2, hotY: height / 2 })
  }

  get active(): boolean {
    return this.#drag !== null
  }

  press(target: Element, x: number, y: number): void {
    const source = target.closest('[draggable="true"]')
    this.#pressed = source ? { source, x, y } : null
  }

  move(target: Element, x: number, y: number, onStart: (source: Element) => void): boolean {
    if (!this.#drag) {
      const pressed = this.#pressed
      if (!pressed || Math.hypot(x - pressed.x, y - pressed.y) < DragSource.SLOP) return false
      this.#pressed = null
      const transfer = new this.#window.DataTransfer() as unknown as DataTransfer
      transfer.effectAllowed = 'all'
      let image: { element: Element; x: number; y: number } | null = null
      const setDragImage = (element: Element, x: number, y: number) => void (image = { element, x, y })
      Object.defineProperty(transfer, 'setDragImage', { value: setDragImage, configurable: true })
      if (!DragDrop.fire(this.#window, pressed.source, 'dragstart', pressed.x, pressed.y, transfer)) return false
      this.#drag = { source: pressed.source, transfer, target: null, taken: false }
      onStart(pressed.source)
      const data: Record<string, string> = {}
      for (const type of transfer.types) if (type !== 'Files') data[type] = transfer.getData(type)
      if (Object.keys(data).length) this.#start(data, this.#icon(image, pressed))
    }
    this.#over(target, x, y)
    return true
  }

  #icon(image: { element: Element; x: number; y: number } | null, pressed: { source: Element; x: number; y: number }): DragIcon {
    const picture = this.#picture(image?.element ?? pressed.source)
    if (!picture) return DragSource.fileIcon
    const clamp = (v: number, max: number) => Math.max(0, Math.min(max - 1, Math.round(v)))
    return {
      width: picture.pixelWidth,
      height: picture.pixelHeight,
      picture: picture.picture,
      hotX: clamp((image ? image.x : pressed.x - picture.x) * picture.scale, picture.pixelWidth),
      hotY: clamp((image ? image.y : pressed.y - picture.y) * picture.scale, picture.pixelHeight),
    }
  }

  #over(target: Element, x: number, y: number): void {
    const drag = this.#drag!
    DragDrop.fire(this.#window, drag.source, 'drag', x, y, drag.transfer)
    drag.taken = DragDrop.over(this.#window, drag.target, target, x, y, drag.transfer)
    drag.target = target
  }

  release(target: Element, x: number, y: number): boolean {
    this.#pressed = null
    const drag = this.#drag
    if (!drag) return false
    this.#over(target, x, y)
    if (drag.taken) {
      if (drag.transfer.dropEffect === 'none') drag.transfer.dropEffect = 'copy'
      if (DragDrop.fire(this.#window, target, 'drop', x, y, drag.transfer)) drag.transfer.dropEffect = 'none'
    } else {
      DragDrop.fire(this.#window, target, 'dragleave', x, y, drag.transfer)
    }
    DragDrop.fire(this.#window, drag.source, 'dragend', x, y, drag.transfer)
    this.#drag = null
    return true
  }

  end(dropped: boolean): void {
    this.#pressed = null
    const drag = this.#drag
    if (!drag) return
    if (drag.target?.isConnected) DragDrop.fire(this.#window, drag.target, 'dragleave', 0, 0, drag.transfer)
    drag.transfer.dropEffect = dropped ? 'copy' : 'none'
    DragDrop.fire(this.#window, drag.source, 'dragend', 0, 0, drag.transfer)
    this.#drag = null
  }
}
