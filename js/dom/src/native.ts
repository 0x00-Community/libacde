export const NONE = -1

export const enum NodeType {
  Element = 1,
  Text = 3,
  CDataSection = 4,
  ProcessingInstruction = 7,
  Comment = 8,
  Document = 9,
  DocumentType = 10,
  DocumentFragment = 11,
}

export const enum Ns {
  Html = 0,
  Svg = 1,
  MathMl = 2,
  Other = 3,
}

export interface Metrics {
  offsetLeft: number
  offsetTop: number
  offsetWidth: number
  offsetHeight: number
  offsetParent: number
  clientLeft: number
  clientTop: number
  clientWidth: number
  clientHeight: number
  scrollLeft: number
  scrollTop: number
  scrollWidth: number
  scrollHeight: number
}

export interface DomNative {
  document(): number
  parseDocument(html: string): void
  viewport(): { width: number; height: number; scale: number }

  nodeType(id: number): NodeType
  parent(id: number): number
  firstChild(id: number): number
  lastChild(id: number): number
  nextSibling(id: number): number
  previousSibling(id: number): number
  childNodes(id: number): Uint32Array
  children(id: number): Uint32Array
  childCount(id: number): number
  isConnected(id: number): boolean
  ancestors(id: number): Uint32Array
  describe(id: number): [NodeType, Ns, boolean, string]
  resize?(width: number, height: number, scale: number): void
  setDark?(on: boolean): void
  compareDocumentPosition(a: number, b: number): number

  createElement(ns: Ns, localName: string): number
  createElementNS(namespace: string, prefix: string, localName: string): number
  createCDATASection(data: string): number
  createProcessingInstruction(target: string, data: string): number
  processingInstructionTarget(id: number): string
  createDocument(kind: string, contentType: string, url: string): number
  documentInfo(id: number): string[]
  parseXml(text: string, contentType: string, url: string): number
  ownerDocument(id: number): number
  adopt(id: number, document: number): void
  inHtmlDocument(id: number): boolean
  serializeXml(id: number): string
  createText(data: string): number
  createComment(data: string): number
  createFragment(): number
  createDocumentType(name: string, publicId: string, systemId: string): number
  documentType(id: number): string[]
  clone(id: number, deep: boolean): number

  insertBefore(parent: number, child: number, reference: number): void
  remove(child: number): void
  replaceChildren(parent: number, ids: Uint32Array): void

  localName(id: number): string
  namespace(id: number): Ns
  namespaceURI(id: number): string
  prefix(id: number): string
  attributesNS(id: number): string[]
  getAttributeNS(id: number, namespace: string, localName: string): string | null
  setAttributeNS(id: number, namespace: string, prefix: string, localName: string, value: string): boolean
  removeAttributeNS(id: number, namespace: string, localName: string): boolean
  getAttribute(id: number, name: string): string | null
  setAttribute(id: number, name: string, value: string): boolean
  removeAttribute(id: number, name: string): boolean
  hasAttribute(id: number, name: string): boolean
  attributes(id: number): string[]
  id(id: number): string
  className(id: number): string

  data(id: number): string
  setData(id: number, data: string): void

  textContent(id: number): string
  setTextContent(id: number, text: string): void
  innerText(id: number): string
  innerHTML(id: number): string
  outerHTML(id: number): string
  parseFragment(context: number, html: string): number
  templateContent(id: number): number
  parseInert(html: string, url: string): number

  querySelector(root: number, selector: string): number
  querySelectorAll(root: number, selector: string): Uint32Array
  matches(id: number, selector: string): boolean
  closest(id: number, selector: string): number
  getElementById(id: string): number
  getElementsByClassName(root: number, classNames: string): Uint32Array
  getElementsByTagName(root: number, qualifiedName: string): Uint32Array
  getElementsByTagNameNS(root: number, namespace: string, localName: string): Uint32Array

  value(id: number): string
  setValue(id: number, value: string): void
  checked(id: number): boolean
  setChecked(id: number, checked: boolean): void
  selectedIndex(id: number): number
  setSelectedIndex(id: number, index: number): void
  options(select: number): Uint32Array
  optionSelected(option: number): boolean
  setOptionSelected(option: number, selected: boolean): void
  resetSelect(select: number): void
  listBoxOptionAt(select: number, x: number, y: number): number
  openPicker(select: number): boolean
  closePicker(): void
  pickerState(): Float64Array
  pickerOptionAt(x: number, y: number): number
  pickerOptionRect(option: number): Float64Array | null
  setPickerActive(option: number): void
  setSelectionRange(id: number, start: number, end: number): void

  setFocus(id: number, visible: boolean): void
  setActive(id: number): void
  setOpenState(id: number, modal: boolean, open: boolean): void
  setSelection(anchorNode: number, anchorOffset: number, focusNode: number, focusOffset: number): void

  rect(id: number): Float64Array
  clientRects(id: number): Float64Array
  metrics(id: number): Metrics | null
  hitTest(x: number, y: number): number
  hitTestAll(x: number, y: number): Uint32Array
  computedStyle(id: number, pseudo: string, property: string): string
  computedStyleAll(id: number, pseudo: string): string[]
  cssSupports(condition: string): boolean
  cssSupportsDeclaration(property: string, value: string): boolean
  caretRect(id: number, offset: number): Float64Array
  caretFromPoint(x: number, y: number): { node: number; offset: number } | null
  textRects(id: number, start: number, end: number): Float64Array
  matchMedia(query: string): boolean
  parseColor(color: string): Float64Array | null

  sheetRuleCount(id: number): number
  sheetRule(id: number, index: number): string
  sheetInsertRule(id: number, rule: string, index: number): number
  sheetDeleteRule(id: number, index: number): void

  scrollTo(id: number, left: number, top: number, smooth: boolean): void
  takeScrolled(): Uint32Array

  imageState(id: number): Float64Array
  takeLoaded(): Uint32Array
  takeAnimationEvents(): AnimationEvents
  sheetState(id: number): number
  sheetUrl(id: number): string | null
  sheetOpaque(id: number): boolean
  sheetSerial(id: number): number
  setLoader(load: ((id: number, url: string, destination: string, cors: string, served?: boolean, cached?: boolean) => void) | null): void
  loaded(id: number, bytes: Uint8Array | null, opaque: boolean): void
  canvas(id: number, commands: Uint8Array): void
  measureText(id: number, font: string, text: string): Float64Array
  toDataURL(id: number, type: string): string
  getImageData(id: number, x: number, y: number, width: number, height: number): Uint8Array
  capture(
    id: number, scale: number,
  ): { x: number; y: number; width: number; height: number; pixelWidth: number; pixelHeight: number; scale: number; picture: number } | null

  setPanel(id: number, x: number, y: number, width: number, height: number): void
  highlight?(id: number): void
  unpin(id: number): void
  pinnedIn(root: number): Uint32Array

  commit(): void

  semantics?(root: number, limit: number, offscreen?: boolean, invisible?: boolean): string

  stats(reset?: boolean): DomStats
}

export interface AnimationEventRecord {
  readonly type: string
  readonly transition: boolean
  readonly target: number
  readonly name: string
  readonly elapsedTime: number
  readonly pseudoElement: string
}

export interface AnimationEvents {
  readonly animating: boolean
  readonly events: readonly AnimationEventRecord[]
}

export interface PhaseTiming {
  readonly count: number
  readonly totalMs: number
  readonly maxMs: number
}

export interface DomStats {
  readonly resolve: PhaseTiming
  readonly frame: PhaseTiming
  readonly frameLocked: PhaseTiming
  readonly resolveMs: number
  readonly paintMs: number
  readonly damageMs: number
  readonly layersMs: number
  readonly fragmentsRecorded: number
  readonly fragmentsReused: number
  readonly animating: boolean
  readonly nodes: number
  readonly images: number
  readonly imageBytes: number
  readonly mallocBytes: number
}

export interface HostNative {
  now(): number
  timeOrigin(): number
  setTimer(ms: number, callback: () => void, refed?: boolean): number
  clearTimer(handle: number): void
  refTimer(handle: number, refed: boolean): void
  queueMicrotask(callback: () => void): void
  requestFrame(): void
  reportError(error: unknown, thrown?: unknown): void
  print(level: number, text: string): void
  log(level: number, target: string, text: string): void
  logEnabled(level: number, target: string): boolean
  clock(): number

  importModule(url: string): Promise<unknown>
  defineModule(url: string, exports: object): void
  refuseModule(url: string, reason: string): void
  setModuleSource?(source: ((url: string) => string | null) | null): void
  runModule(source: string, url: string): Promise<unknown>
  runScript(source: string, url: string): unknown

  readAsset(url: string): Uint8Array | null
  liveEdit?(url: string, source: string): string | undefined
  inspected?(): boolean
  devtoolsHandler?(handler: (session: number, text: string | null) => void): string | undefined
  devtoolsSend?(session: number, text: string): void
  addImportMap?(imports: [string, string][], scopes: [string, [string, string][]][]): void

  encodeUtf8(text: string): Uint8Array
  decode(bytes: Uint8Array, label: string, fatal: boolean, ignoreBom: boolean): string
  encodingName(label: string): string | null
  openDecoder(encoding: string, fatal: boolean, ignoreBom: boolean): number
  decodeChunk(id: number, bytes: Uint8Array, last: boolean): string
  closeDecoder(id: number): void
  parseUrl(input: string, base: string | null): {
    href: string; origin: string; protocol: string; username: string; password: string; host: string; hostname: string; port: string; pathname: string
    search: string; hash: string
  } | null
  serialize(value: unknown): Uint8Array
  isProxy?(value: unknown): boolean
  runMicrotasks?(): void
  cloneHooks?(brand: symbol, revive: (root: unknown, placeholders: readonly [object, unknown][]) => unknown): void
  deserialize(bytes: Uint8Array): unknown
  randomBytes(buffer: Uint8Array): void
  randomUUID(): string
}

export interface GpuNative {
  requestAdapter(): Promise<GpuAdapterInfo | null>
  createDevice(
    features: string[], limits: Record<string, number>, onError: (kind: string, message: string) => void, onLost: (reason: string, message: string) => void,
  ): number
  destroyDevice(device: number): void
  pushErrorScope(device: number, filter: string): void
  popErrorScope(device: number): GpuErrorInfo | null | undefined
  drop(id: number): void

  createBuffer(device: number, descriptor: string): number
  destroyBuffer(device: number, buffer: number): void
  mapAsync(device: number, buffer: number, mode: number, offset: number, size: number): Promise<string | null>
  getMappedRange(buffer: number, offset: number, size: number): ArrayBuffer
  unmap(device: number, buffer: number, written: (number | ArrayBuffer)[]): void
  createTexture(device: number, descriptor: string): number
  destroyTexture(device: number, texture: number): void
  createView(device: number, texture: number, descriptor: string): number
  createSampler(device: number, descriptor: string): number
  createBindGroupLayout(device: number, descriptor: string): number
  createPipelineLayout(device: number, descriptor: string): number
  createBindGroup(device: number, descriptor: string): number
  createShaderModule(device: number, code: string, label: string): number
  compilationInfo(module: number): GpuCompilationMessage[]
  createComputePipeline(device: number, descriptor: string, async: boolean): number | string
  createRenderPipeline(device: number, descriptor: string, async: boolean): number | string
  getBindGroupLayout(device: number, pipeline: number, index: number): number
  createQuerySet(device: number, descriptor: string): number
  destroyQuerySet(device: number, querySet: number): void

  createCommandEncoder(device: number, label: string): number
  copyBufferToBuffer(device: number, encoder: number, source: number, sourceOffset: number, destination: number, destinationOffset: number, size: number): void
  copyBufferToTexture(device: number, encoder: number, descriptor: string): void
  copyTextureToBuffer(device: number, encoder: number, descriptor: string): void
  copyTextureToTexture(device: number, encoder: number, descriptor: string): void
  clearBuffer(device: number, encoder: number, buffer: number, offset: number, size: number): void
  resolveQuerySet(device: number, encoder: number, querySet: number, first: number, count: number, destination: number, offset: number): void
  encoderDebug(device: number, encoder: number, op: number, label: string): void
  renderPass(device: number, encoder: number, descriptor: string, commands: Float64Array, length: number, strings: string[]): void
  computePass(device: number, encoder: number, descriptor: string, commands: Float64Array, length: number, strings: string[]): void
  finishEncoder(device: number, encoder: number, label: string): number
  createRenderBundle(device: number, descriptor: string, commands: Float64Array, length: number, strings: string[]): number

  submit(device: number, buffers: number[]): void
  writeBuffer(device: number, buffer: number, offset: number, data: ArrayBufferView | ArrayBuffer): void
  writeTexture(device: number, descriptor: string, data: ArrayBufferView | ArrayBuffer): void
  onSubmittedWorkDone(device: number): Promise<void>

  preferredFormat(): string
  configure(node: number, device: number, descriptor: string): void
  unconfigure(node: number): void
  currentTexture(node: number, width: number, height: number): number
  present(node: number): void
  pixels(node: number): { width: number; height: number; rgba: Uint8Array } | null
  png(node: number, width: number, height: number): Uint8Array
  pngAsync(node: number, width: number, height: number): Promise<Uint8Array>
}

export interface GpuAdapterInfo {
  vendor: string
  architecture: string
  device: string
  description: string
  isFallbackAdapter: boolean
  features: string[]
  limits: Record<string, number>
}

export interface GpuErrorInfo {
  kind: string
  message: string
}

export interface GpuCompilationMessage {
  message: string
  type: 'error' | 'warning' | 'info'
  lineNum: number
  linePos: number
  offset: number
  length: number
}

export interface StorageAreaNative {
  length(): number
  key(index: number): string | null
  get(key: string): string | null
  set(key: string, value: string, url: string): boolean
  remove(key: string, url: string): void
  clear(url: string): void
  keys(): string[]
}

export interface StorageNative {
  readonly local: StorageAreaNative
  readonly idb?: IdbNative
  readonly cookies?: CookieNative
  readonly caches?: CacheNative
  estimate?(): Promise<{ usage: number; quota: number }>
  privateDirectory?(): string
  onChange(callback: (key: string | null, oldValue: string | null, newValue: string | null, url: string) => void): void
}

export interface CacheQueryNative {
  readonly url: string
  readonly method: string
  readonly headers: [string, string][]
  readonly ignoreSearch: boolean
  readonly ignoreMethod: boolean
  readonly ignoreVary: boolean
}

export interface CacheEntryNative {
  readonly url: string
  readonly method: string
  readonly requestHeaders: [string, string][]
  readonly response: string
  readonly responseHeaders: [string, string][]
  readonly body: Uint8Array
}

export interface CacheNative {
  names(): Promise<string[]>
  has(name: string): Promise<boolean>
  open(name: string): Promise<number>
  delete(name: string): Promise<boolean>
  query(cache: number | null, query: CacheQueryNative | null, first: boolean): Promise<CacheEntryNative[]>
  batch(cache: number, operations: ({ type: 'put'; entry: CacheEntryNative } | { type: 'delete'; query: CacheQueryNative })[]): Promise<boolean[]>
}

export interface CookieNative {
  set(name: string, value: string, path: string, expires: number | null, httpOnly: boolean, fromScript: boolean): void
  matching(path: string, script: boolean): [string, string][]
}

export interface IdbIndexInfo {
  readonly id: number
  readonly name: string
  readonly keyPath: string
  readonly unique: boolean
  readonly multiEntry: boolean
}

export interface IdbStoreInfo {
  readonly id: number
  readonly name: string
  readonly keyPath: string | null
  readonly autoIncrement: boolean
  readonly generator: number
  readonly indexes: readonly IdbIndexInfo[]
}

export interface IdbSchema {
  readonly version: number
  readonly stores: readonly IdbStoreInfo[]
}

export interface IdbError {
  readonly name: string
  readonly message: string
}

export interface IdbRecord {
  readonly key: Uint8Array
  readonly primaryKey: Uint8Array
  readonly value: Uint8Array | null
}

export type IdbEvent =
  | { readonly type: 'opened'; readonly req: number; readonly db: number; readonly conn: number; readonly schema: IdbSchema }
  | {
    readonly type: 'upgrade'; readonly req: number; readonly db: number; readonly conn: number; readonly tx: number; readonly old: number
    readonly schema: IdbSchema
  }
  | { readonly type: 'openFailed'; readonly req: number; readonly error: IdbError }
  | { readonly type: 'blocked'; readonly req: number; readonly old: number; readonly new: number | null }
  | { readonly type: 'versionchange'; readonly conn: number; readonly old: number; readonly new: number | null }
  | { readonly type: 'deleted'; readonly req: number; readonly old: number }
  | { readonly type: 'started'; readonly tx: number; readonly generators: readonly [number, number][] }
  | {
    readonly type: 'result'; readonly tx: number; readonly op: number; readonly id?: number; readonly count?: number; readonly records?: readonly IdbRecord[]
    readonly error?: IdbError
  }
  | { readonly type: 'complete'; readonly tx: number }
  | { readonly type: 'aborted'; readonly tx: number; readonly error: IdbError | null }
  | { readonly type: 'forcedClose'; readonly conn: number }

export type IdbKeyBytes = Uint8Array | null

export interface IdbNative {
  onEvent(callback: (event: IdbEvent) => void): void
  open(name: string, req: number, version: number | undefined): void
  deleteDatabase(name: string, req: number): void
  databases(): [string, number][]
  begin(db: number, tx: number, conn: number, scope: number[], mode: 0 | 1): void
  commit(db: number, tx: number): void
  abort(db: number, tx: number): void
  close(db: number, conn: number): void
  acknowledge(db: number, conn: number): void
  put(
    db: number, tx: number, op: number, store: number, key: Uint8Array, value: Uint8Array, indexes: (number | Uint8Array)[], overwrite: boolean,
    generator: number | undefined,
  ): void
  remove(db: number, tx: number, op: number, store: number, lower: IdbKeyBytes, lowerOpen: boolean, upper: IdbKeyBytes, upperOpen: boolean): void
  clear(db: number, tx: number, op: number, store: number): void
  get(
    db: number, tx: number, op: number, index: boolean, source: number, lower: IdbKeyBytes, lowerOpen: boolean, upper: IdbKeyBytes, upperOpen: boolean,
    values: boolean, count: number, direction: number,
  ): void
  count(
    db: number, tx: number, op: number, index: boolean, source: number, lower: IdbKeyBytes, lowerOpen: boolean, upper: IdbKeyBytes, upperOpen: boolean,
  ): void
  cursor(
    db: number, tx: number, op: number, index: boolean, source: number, lower: IdbKeyBytes, lowerOpen: boolean, upper: IdbKeyBytes, upperOpen: boolean,
    direction: number, position: IdbKeyBytes, positionPrimary: IdbKeyBytes, key: IdbKeyBytes, primaryKey: IdbKeyBytes, advance: number, values: boolean,
  ): void
  createStore(db: number, tx: number, op: number, id: number, name: string, keyPath: string | null, autoIncrement: boolean): void
  deleteStore(db: number, tx: number, op: number, store: number): void
  renameStore(db: number, tx: number, op: number, store: number, name: string): void
  createIndex(db: number, tx: number, op: number, id: number, store: number, name: string, keyPath: string, unique: boolean, multiEntry: boolean): void
  indexEntries(db: number, tx: number, op: number, index: number, entries: Uint8Array[]): void
  deleteIndex(db: number, tx: number, op: number, index: number): void
  renameIndex(db: number, tx: number, op: number, index: number, name: string): void
}

export interface CaptureDevice {
  readonly deviceId: string
  readonly kind: 'audioinput' | 'videoinput'
  readonly label: string
  readonly groupId: string
  readonly facingMode: string | null
  readonly modes: readonly (readonly [number, number, number])[]
  readonly channelCount: number
}

export interface CaptureOpened extends CaptureDevice {
  readonly source: number
  readonly width: number
  readonly height: number
  readonly frameRate: number
}

export interface CaptureNative {
  devices(full: boolean): Promise<CaptureDevice[]>
  permission(kind: 'camera' | 'microphone'): 'granted' | 'denied' | 'prompt'
  request?(kind: 'camera' | 'microphone'): Promise<boolean>
  onPermission?(callback: () => void): void
  open(kind: 'audioinput' | 'videoinput', deviceId: string, width: number, height: number, frameRate: number): Promise<CaptureOpened>
  close(source: number): void
  onEnded(callback: (source: number, reason: string) => void): void
}

export interface AudioNative {
  readonly sampleRate: number
  onEvent(callback: (kind: number, id: number) => void): void
  createContext(context: number, destination: number): void
  setRunning(context: number, running: boolean): void
  closeContext(context: number): void
  time(context: number): number
  createNode(context: number, node: number, kind: number): void
  connect(from: number, to: number, param: number): void
  disconnect(from: number, to: number, param: number): void
  automate(node: number, param: number, op: number, value: number, time: number, extra: number): void
  automateCurve(node: number, param: number, values: Float32Array, time: number, duration: number): void
  paramValue(node: number, param: number): number
  setWave(node: number, type: number): void
  setLoop(node: number, loop: boolean, loopStart: number, loopEnd: number): void
  setPanner(node: number, hrtf: boolean, distanceModel: number, refDistance: number, maxDistance: number, rolloffFactor: number, coneInnerAngle: number, coneOuterAngle: number, coneOuterGain: number): void
  setBuffer(node: number, buffer: number): void
  start(node: number, when: number, offset: number, duration: number): void
  stop(node: number, when: number): void
  release(node: number): void
  decode(buffer: number, bytes: Uint8Array): Promise<{ length: number; sampleRate: number; numberOfChannels: number }>
  createBuffer(buffer: number, sampleRate: number, channels: Float32Array[]): void
  channelData(buffer: number): Float32Array[]
  releaseBuffer(buffer: number): void
  openMedia(media: number, bytes: Uint8Array): Promise<{ duration: number; sampleRate: number; numberOfChannels: number }>
  media(media: number, op: number, value: number): void
  mediaTime(media: number): number
  closeMedia(media: number): void
  openVideo(media: number, node: number, bytes: Uint8Array): Promise<{ duration: number; width: number; height: number; audio: number }>
  present(media: number): number
  videoSize(media: number): [number, number]
  clearVideo(node: number): void
  readonly videoTypes: readonly string[]
  openStream(media: number, node: number, videoSource: number, audioSource: number): { duration: number; width: number; height: number; audio: number }
  streamEnabled(media: number, enabled: boolean): void
  streamNode(node: number, source: number): void
}
