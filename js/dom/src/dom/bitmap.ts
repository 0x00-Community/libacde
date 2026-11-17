import { Blob } from '../core/blob'
import { DOMException } from '../core/exception'
import { tag } from '../core/host'
import { URL } from '../core/url'
import { canvasSources, ImageData } from './canvas'
import { createHtml } from './html'

const TOKEN = Symbol('token')

interface Canvas {
  width: number
  height: number
  readonly localName: string
  getContext(kind: '2d'): Context2d
}

interface Context2d {
  translate(x: number, y: number): void
  scale(x: number, y: number): void
  drawImage(image: unknown, ...a: number[]): void
  putImageData(image: ImageData, x: number, y: number): void
}

export const bitmaps = {} as {
  canvas(bitmap: ImageBitmap): Canvas | null
  premultiplied(bitmap: ImageBitmap): boolean
}

export class ImageBitmap {
  #canvas: Canvas | null
  #width: number
  #height: number
  readonly #premultiplied: boolean

  constructor(token: symbol, canvas: Canvas, premultiplied: boolean) {
    if (token !== TOKEN) throw new TypeError('Illegal constructor')
    this.#canvas = canvas
    this.#width = canvas.width
    this.#height = canvas.height
    this.#premultiplied = premultiplied
  }

  get width(): number {
    return this.#canvas ? this.#width : 0
  }

  get height(): number {
    return this.#canvas ? this.#height : 0
  }

  close(): void {
    this.#canvas = null
  }

  static {
    bitmaps.canvas = (b) => b.#canvas
    bitmaps.premultiplied = (b) => b.#premultiplied
  }
}
tag(ImageBitmap)

interface Options {
  imageOrientation?: string
  premultiplyAlpha?: string
  colorSpaceConversion?: string
  resizeWidth?: number
  resizeHeight?: number
  resizeQuality?: string
}

async function drawable(source: unknown): Promise<[unknown, number, number]> {
  if (source instanceof ImageBitmap) {
    const canvas = bitmaps.canvas(source)
    if (!canvas) throw new DOMException('The image bitmap is closed', 'InvalidStateError')
    return [canvas, canvas.width, canvas.height]
  }
  if (source instanceof ImageData) {
    const canvas = createHtml('canvas') as unknown as Canvas
    canvas.width = source.width
    canvas.height = source.height
    canvas.getContext('2d').putImageData(source, 0, 0)
    return [canvas, source.width, source.height]
  }
  if (source instanceof Blob) {
    const url = URL.createObjectURL(source)
    try {
      const img = createHtml('img') as unknown as { src: string; decode(): Promise<void>; naturalWidth: number; naturalHeight: number }
      img.src = url
      await img.decode().catch(() => {
        throw new DOMException('The source image could not be decoded', 'InvalidStateError')
      })
      return [img, img.naturalWidth, img.naturalHeight]
    } finally {
      URL.revokeObjectURL(url)
    }
  }
  const local = (source as { localName?: string } | null)?.localName
  if (local === 'img') {
    const img = source as { complete: boolean; decode(): Promise<void>; naturalWidth: number; naturalHeight: number }
    if (!img.complete) await img.decode().catch(() => {})
    if (!img.naturalWidth || !img.naturalHeight) throw new DOMException('The source image is not decoded', 'InvalidStateError')
    return [source, img.naturalWidth, img.naturalHeight]
  }
  if (local === 'canvas') {
    const canvas = source as Canvas
    if (!canvas.width || !canvas.height) throw new DOMException('The source canvas is empty', 'InvalidStateError')
    return [source, canvas.width, canvas.height]
  }
  if (local === 'video') throw new DOMException('createImageBitmap of a <video> is not supported', 'NotSupportedError')
  throw new TypeError('createImageBitmap: the source is not an image')
}

export async function createImageBitmap(source: unknown, ...args: unknown[]): Promise<ImageBitmap> {
  const cropped = args.length >= 4
  const options = ((cropped ? args[4] : args[0]) ?? {}) as Options
  const [sx, sy, sw, sh] = (cropped ? args.slice(0, 4) : []).map(Number) as number[]
  if (cropped && (!sw || !sh)) throw new RangeError('createImageBitmap: the crop width or height is 0')
  if (options.resizeWidth === 0 || options.resizeHeight === 0) throw new DOMException('createImageBitmap: a resize size is 0', 'InvalidStateError')
  const [image, width, height] = await drawable(source)
  const rect = cropped ? [sw! < 0 ? sx! + sw! : sx!, sh! < 0 ? sy! + sh! : sy!, Math.abs(sw!), Math.abs(sh!)] : [0, 0, width, height]
  const [x, y, w, h] = rect as [number, number, number, number]
  const outW = Math.round(options.resizeWidth ?? (options.resizeHeight ? (w * options.resizeHeight) / h : w))
  const outH = Math.round(options.resizeHeight ?? (options.resizeWidth ? (h * options.resizeWidth) / w : h))
  const canvas = createHtml('canvas') as unknown as Canvas
  canvas.width = Math.max(1, outW)
  canvas.height = Math.max(1, outH)
  const context = canvas.getContext('2d')
  if (options.imageOrientation === 'flipY') {
    context.translate(0, canvas.height)
    context.scale(1, -1)
  }
  context.drawImage(image, x, y, w, h, 0, 0, canvas.width, canvas.height)
  return new ImageBitmap(TOKEN, canvas, options.premultiplyAlpha === 'premultiply')
}

canvasSources.resolve = (image) => (image instanceof ImageBitmap ? (bitmaps.canvas(image) as unknown) : image)
