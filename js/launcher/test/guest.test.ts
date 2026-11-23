import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { GuestWindows, type GuestRow } from '../src/dev/guest.ts'
import { Inspect, type InspectHost, type PlayEvent } from '../src/dev/inspect.ts'

const row = (id: number, pid: number, extra: Partial<GuestRow> = {}): GuestRow => ({
  id, app_id: `app${id}`, pid, title: `T${id}`, x: 0, y: 0, width: 100, height: 80, activated: false, hidden: false,
  desktop: false, lock: false, confirm: false, ...extra,
})

function base(local: number, focused: number[]): InspectHost {
  return {
    shell: { focus: (w) => void focused.push(w), windows: () => [], inject: () => {}, play: async () => {} },
    web: () => null,
    record: (w) => (w === local ? { kind: 'web', app: 'files', title: 'Files' } : null),
    panel: () => false,
    close: async () => undefined,
    hide: () => {},
    open: () => Promise.reject(new Error('no open')),
    accessibility: async () => null,
    records: () => [local],
  }
}

describe('the window table the session manager sends along', () => {
  test('without one the host table is used; with one the winman ids name the windows', () => {
    const focused: number[] = []
    const guest = new GuestWindows(42, 'app')
    const inspect = new Inspect(guest.host(base(1, focused), () => 1))
    assert.deepEqual(inspect.windows(), [])
    const desktop = { id: 0, desktop: true, app_id: null, pid: 10, title: 'desktop', activated: false, hidden: false }
    guest.place([row(7, 42, { activated: true }), { ...row(9, 50), desk: 2, desktop: undefined }, desktop, { bogus: true }])
    const windows = inspect.windows().map((w) => [w.ref, w.kind, w.app, w.title, w.focus])
    assert.deepEqual(windows, [
      ['w7', 'web', 'files', 'Files', true],
      ['w9', 'native', 'app9', 'T9', false],
      ['w0', 'desktop', null, null, false],
    ])
    assert.equal(guest.mine(), 7)
  })

  test('the own window maps back to the local one; the desktop process owns w0', () => {
    const focused: number[] = []
    const app = new GuestWindows(42, 'app')
    const host = app.host(base(1, focused), () => 1)
    app.place([row(7, 42), row(9, 50)])
    host.shell.focus(7)
    host.shell.focus(9)
    assert.deepEqual(focused, [1, 9])
    assert.deepEqual(host.records(), [7])
    const desktop = new GuestWindows(10, 'desktop')
    desktop.place([row(7, 42), { id: 0, desktop: true, pid: 10 }])
    assert.equal(desktop.mine(), 0)
  })
})

describe('pointer and keys in the guest', () => {
  test('go to the manager once a window table came, before that to the local shell', () => {
    const local: object[] = []
    const sent: object[] = []
    const guest = new GuestWindows(42, 'app')
    const plain = base(1, [])
    const host = guest.host({ ...plain, shell: { ...plain.shell, inject: (e) => void local.push(e) } }, () => 1, { inject: (e) => void sent.push(e) })
    host.shell.inject({ type: 'move', x: 1, y: 2 })
    guest.place([row(7, 42)])
    host.shell.inject({ type: 'down', x: 1, y: 2, button: 0x110 })
    assert.deepEqual(local, [{ type: 'move', x: 1, y: 2 }])
    assert.deepEqual(sent, [{ type: 'down', x: 1, y: 2, button: 0x110 }])
  })
})

describe('the lock page in the guest', () => {
  test('is its own window on top, found by its own row, and not the desktop', () => {
    const lock = new GuestWindows(77, 'lock')
    assert.equal(lock.global(0), 0)
    lock.place([row(7, 42), { id: 0, desktop: true, pid: 10 }, { id: 900, lock: true, pid: 77, title: 'lock' }])
    assert.equal(lock.mine(), 900)
    assert.equal(lock.global(0), 900)
    const inspect = new Inspect(lock.host(base(0, []), () => 0))
    assert.deepEqual(inspect.windows().map((w) => [w.ref, w.kind]), [['w900', 'web'], ['w7', 'native'], ['w0', 'desktop']])
  })
})

describe('the system confirm page in the guest', () => {
  test('is its own window on top, marked confirm, and refuses what only the session manager does', async () => {
    const page = new GuestWindows(78, GuestWindows.roleOf({ kind: 'desktop', lock: true, confirm: true }))
    page.place([row(7, 42), { id: 0, desktop: true, pid: 10 }, { id: 901, confirm: true, pid: 78, title: 'confirm' }])
    assert.equal(page.mine(), 901)
    const inspect = new Inspect(page.host(base(0, []), () => 0))
    const windows = inspect.windows()
    assert.deepEqual(windows.map((w) => [w.ref, w.mode]), [['w901', 'confirm'], ['w7', 'window'], ['w0', 'desktop']])
    for (const action of ['move', 'resize', 'raise', 'close', 'minimize', 'fill'] as const) {
      const said = await inspect.act('w901', action, action === 'move' ? '1,1' : action === 'resize' ? '9x9' : null)
      assert.equal(said.ok, false, action)
      assert.match(said.text, /^w901 is the system confirm page, which the session manager shows and puts away/)
    }
  })

  test('the lock page is marked lock and is not moved either', async () => {
    const page = new GuestWindows(77, 'lock')
    page.place([{ id: 900, lock: true, pid: 77, title: 'lock' }])
    const inspect = new Inspect(page.host(base(0, []), () => 0))
    assert.equal(inspect.windows()[0]!.mode, 'lock')
    const said = await inspect.act('w900', 'move', '1,1')
    assert.deepEqual(said, { ok: false, text: 'w900 is the lock screen, which the session manager shows and puts away' })
  })
})

describe('the desktop panels in the guest', () => {
  test('the window table leaves them out, so the desktop adds its own on top', () => {
    const panelRow = { id: 5, canvas: 0, mode: 'panel', x: 0, y: 0, width: 800, height: 30, z: 1, focus: false }
    const plain = base(0, [])
    const host = new GuestWindows(10, 'desktop')
    const wrapped = host.host({ ...plain, panel: (id) => id === 5 }, () => 0, { panels: () => [panelRow] })
    host.place([row(7, 42), { id: 0, desktop: true, pid: 10 }])
    const rows = wrapped.shell.windows()
    assert.deepEqual(rows.map((r) => [r.id, r.z]), [[7, 2], [0, -1], [5, 3]])
    assert.equal(wrapped.panel(5), true)
  })
})

describe('a drag to another process window in the guest', () => {
  test('drops at the screen point the driver worked out', async () => {
    const sent: PlayEvent[] = []
    const plain = base(1, [])
    const rpc = { call: async (_service: string, method: string) => (method === 'tree' ? { nodes: [] } : { x: 10, y: 20 }) } as never
    const guest = new GuestWindows(42, 'app')
    const host = guest.host({ ...plain, web: (w) => (w === 1 ? rpc : null) }, () => 1, { play: async (events) => void sent.push(...events) })
    guest.place([{ ...row(7, 42), x: 100, y: 50 }, row(9, 50)])
    const result = await new Inspect(host).act('w7.3', 'drag', '640,360')
    assert.equal(result.ok, true)
    assert.deepEqual(sent[0], { type: 'move', x: 110, y: 70 })
    assert.deepEqual(sent.at(-1), { type: 'up', x: 640, y: 360, button: 'left' })
  })
})

describe('a drag is one sequence, paced where it is played', () => {
  test('move, press, the steps and the release go together, with no pauses of the launcher', async () => {
    const played: (readonly PlayEvent[])[] = []
    const plain = base(1, [])
    const rpc = { call: async (_service: string, method: string) => (method === 'tree' ? { nodes: [] } : { x: 10, y: 20 }) } as never
    const guest = new GuestWindows(42, 'app')
    const host = guest.host({ ...plain, web: (w) => (w === 1 ? rpc : null) }, () => 1, { play: async (events) => void played.push(events) })
    guest.place([{ ...row(7, 42), x: 100, y: 50 }])
    assert.equal((await new Inspect(host).act('w7.3', 'drag', '400,300')).ok, true)
    assert.equal(played.length, 1)
    const kinds = played[0]!.map((e) => e.type)
    assert.deepEqual(kinds, ['move', 'down', ...Array(Inspect.DRAG_STEPS).fill('move'), 'up'])
  })
})

describe('input to a page', () => {
  const row7 = { id: 7, window: 7, canvas: 0, mode: 'free', x: 30, y: 40, width: 300, height: 200, z: 1, focus: true }

  test('goes to the shell as one sequence in screen points, text in between through the page', async () => {
    const played: (readonly PlayEvent[])[] = []
    const typed: unknown[] = []
    const call = async (_service: string, method: string, args: unknown[]) => {
      if (method === 'text') typed.push(args[0])
      return method === 'offset' ? [0, 0] : null
    }
    const plain = base(7, [])
    const shell = { ...plain.shell, windows: () => [row7], play: async (events: readonly PlayEvent[]) => void played.push(events) }
    const host: InspectHost = { ...plain, web: () => ({ call }) as never, shell }
    const events = [
      { type: 'down', x: 5, y: 5 }, { type: 'move', x: 50, y: 5 }, { type: 'up', x: 50, y: 5, button: 'right' },
      { type: 'text', text: 'hi' }, { type: 'keydown', key: 'a' }, { type: 'wait', ms: 40 },
    ] as const
    assert.equal((await new Inspect(host).input(7, events)).ok, true)
    assert.deepEqual(played[0], [
      { type: 'down', x: 35, y: 45, button: 'left' }, { type: 'move', x: 80, y: 45 }, { type: 'up', x: 80, y: 45, button: 'right' },
    ])
    assert.deepEqual(typed, ['hi'])
    assert.equal(played[1]![0]!.type, 'keydown')
    assert.deepEqual(played[1]![1], { type: 'wait', ms: 40 })
  })

  test('a bad event sends nothing of its sequence', async () => {
    const played: unknown[] = []
    const plain = base(7, [])
    const shell = { ...plain.shell, windows: () => [row7], play: async (events: readonly PlayEvent[]) => void played.push(events) }
    const call = async () => [0, 0]
    const host: InspectHost = { ...plain, web: () => ({ call }) as never, shell }
    const events = [{ type: 'down', x: 5, y: 5 }, { type: 'up', x: 5, y: 5, button: 'thumb' }] as never
    await assert.rejects(new Inspect(host).input(7, events), /button is left, right or middle/)
    assert.deepEqual(played, [])
  })
})

describe('window changes in the guest', () => {
  test('go to winman by window number through the manager, and so does the focus of the desktop', async () => {
    const changes: object[] = []
    const focused: number[] = []
    const plain = base(1, focused)
    const guest = new GuestWindows(42, 'app')
    const shell = { ...plain.shell, setState: () => assert.fail('no local state') }
    const host = guest.host({ ...plain, shell }, () => 1, { window: (c) => void changes.push(c) })
    guest.place([row(7, 42), row(9, 50), { id: 0, desktop: true, pid: 10 }])
    host.shell.setState!(7, 'minimized')
    host.shell.focus(9)
    host.hide(9, false)
    await host.close(9)
    host.shell.focus(0)
    assert.deepEqual(changes, [
      { window: 7, state: 'minimized' }, { window: 9, focus: true }, { window: 9, state: 'normal' }, { window: 9, close: true }, { window: 0, focus: true },
    ])
    assert.deepEqual(focused, [])
  })
})

describe('a guard on act and run (what only the user may act on)', () => {
  function guarded(answer: string | null) {
    const sent: object[] = []
    const asked: unknown[][] = []
    const plain = base(1, [])
    const rpc = {
      call: async (_service: string, method: string, args: unknown[]) => {
        if (method === 'guarded') asked.push(args)
        return method === 'guarded' ? answer : method === 'tree' ? { nodes: [] } : { x: 10, y: 20 }
      },
    } as never
    const guest = new GuestWindows(42, 'app')
    const host = guest.host({ ...plain, web: (w) => (w === 1 ? rpc : null) }, () => 1, {
      inject: (e) => void sent.push(e),
      play: async (events) => void sent.push(...events),
    })
    guest.place([{ ...row(7, 42), x: 100, y: 50 }, row(9, 50)])
    return { inspect: new Inspect(host), sent, asked }
  }

  test('a step on a guarded element is refused and injects nothing', async () => {
    const { inspect, sent, asked } = guarded('this element')
    const result = await inspect.act('w7.3', 'click', null, '[data-user-only]')
    assert.equal(result.ok, false)
    assert.match(result.text, /click on w7\.3: only the user may act on this element/)
    assert.deepEqual(sent, [])
    assert.deepEqual(asked, [['[data-user-only]', 3, 'pointer']])
  })

  test('the page is asked how the step reaches it: pointer, keyboard, or not at all', async () => {
    const { inspect, asked } = guarded(null)
    const steps = [
      { ref: 'w7.3', action: 'type', text: 'x' }, { ref: 'w7', action: 'key', text: 'Enter' }, { ref: 'w7', action: 'minimize', text: null },
    ] as const
    await inspect.run(steps, '[x]')
    assert.deepEqual(asked, [['[x]', 3, 'keyboard'], ['[x]', -1, 'keyboard'], ['[x]', -1, 'none']])
  })

  test('without a guard nothing is asked', async () => {
    const { inspect, sent, asked } = guarded('this element')
    const result = await inspect.act('w7.3', 'click', null)
    assert.equal(result.ok, true)
    assert.deepEqual(asked, [])
    assert.ok(sent.length > 0)
  })
})

describe('move, resize and raise on a window ref', () => {
  const rows = (state: 'normal' | 'filled' = 'normal') => [
    { id: 3, window: 3, canvas: 0, mode: 'free', x: 10, y: 20, width: 300, height: 200, z: 2, focus: false, state },
    { id: 0, window: 0, canvas: 0, mode: 'desktop', x: 0, y: 0, width: 1280, height: 800, z: -1, focus: false },
  ]
  function host(state: 'normal' | 'filled' = 'normal', placed: object[] = [], focused: number[] = []): InspectHost {
    const table = rows(state)
    return {
      ...base(3, focused),
      shell: {
        focus: (w) => void focused.push(w),
        windows: () => table,
        inject: () => {},
        play: async () => {},
        place: async (w, change) => {
          placed.push({ w, ...change })
          const row = table.find((r) => r.id === w)!
          Object.assign(row, { x: change.x, y: change.y, ...(change.width !== undefined && { width: change.width, height: change.height }) })
          return [row.x, row.y, row.width, row.height] as const
        },
      },
      record: (w) => (w === 3 ? { kind: 'web', app: 'notes', title: 'Notes' } : null),
    }
  }

  test('move takes screen x,y and keeps the size; resize takes WxH and keeps the place', async () => {
    const placed: object[] = []
    const inspect = new Inspect(host('normal', placed))
    const moved = await inspect.act('w1', 'move', '100,80')
    assert.equal(moved.ok, true)
    assert.match(moved.text, /^w1 is at 100,80 300x200/)
    const resized = await inspect.act('w1', 'resize', '800x600')
    assert.match(resized.text, /^w1 is at 100,80 800x600/)
    assert.deepEqual(placed, [{ w: 3, x: 100, y: 80 }, { w: 3, x: 100, y: 80, width: 800, height: 600 }])
  })

  test('a bad argument, the desktop and a window that is not normal are refused with the reason', async () => {
    const inspect = new Inspect(host())
    assert.match((await inspect.act('w1', 'move', '100x80')).text, /move needs where to: screen x,y like "100,80"/)
    assert.match((await inspect.act('w1', 'resize', '0x10')).text, /resize needs a size: WxH like "800x600"/)
    assert.match((await inspect.act('w0', 'move', '1,2')).text, /the desktop cannot move/)
    const filled = new Inspect(host('filled'))
    const refused = await filled.act('w1', 'resize', '800x600')
    assert.equal(refused.ok, false)
    assert.match(refused.text, /w1 is filled; only a normal window can resize: act w1 restore first/)
  })

  test('raise brings the window to the front and refuses the desktop', async () => {
    const focused: number[] = []
    const inspect = new Inspect(host('normal', [], focused))
    assert.match((await inspect.act('w1', 'raise', null)).text, /^w1 is in front/)
    assert.deepEqual(focused, [3])
    assert.match((await inspect.act('w0', 'raise', null)).text, /the desktop stays behind every window/)
  })
})

describe('the keyboard follows the driver to the desktop', () => {
  const desktopRow = { id: 0, window: 0, canvas: 0, mode: 'desktop', x: 0, y: 0, width: 800, height: 600, z: -1, focus: false }
  const appRow = { id: 7, window: 7, canvas: 0, mode: 'free', x: 50, y: 50, width: 300, height: 200, z: 1, focus: true }

  function desktop() {
    const log: string[] = []
    const answer = (method: string) => (method === 'point' ? { x: 20, y: 30 } : method === 'tree' ? { nodes: [] } : null)
    const rpc = { call: async (_service: string, method: string) => answer(method) } as never
    const plain = base(0, [])
    const shell = {
      ...plain.shell,
      windows: () => [desktopRow, appRow],
      focus: (w: number) => void log.push(`focus ${w}`),
      inject: (e: { type: string; code?: number }) => void log.push(`${e.type} ${e.code ?? ''}`.trim()),
      play: async (events: readonly PlayEvent[]) => void log.push(`play ${events.map((e) => e.type).join(' ')}`),
    }
    const record = (w: number) => (w === 7 ? { kind: 'web' as const, app: 'files', title: 'F' } : null)
    const host: InspectHost = { ...plain, shell, web: (w) => (w === 0 ? rpc : null), record }
    return { inspect: new Inspect(host), log }
  }

  test('a key on a desktop element asks for the keyboard first, then presses', async () => {
    const { inspect, log } = desktop()
    assert.equal((await inspect.act('w0.4', 'key', 'Enter')).ok, true)
    assert.equal(log[0], 'focus 0', 'the desktop is focused before the key goes')
    assert.deepEqual(log.slice(1, 3), ['keydown 28', 'keyup 28'])
  })

  test('a click on a desktop element is one real pointer sequence', async () => {
    const { inspect, log } = desktop()
    assert.equal((await inspect.act('w0.4', 'click', null)).ok, true)
    assert.deepEqual(log.filter((line) => line.startsWith('play')), ['play move down up'])
  })
})
