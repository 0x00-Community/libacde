export class LaunchArgs {
  static readonly USAGE = 'the single-app launcher: app <id|dir> | desktop [id] | lock <desktop id|dir> | service <name> '
    + '[--size WxH] [--scale N] [--seconds N] [--shot FILE] [--shot-after MS]'

  constructor(
    readonly kind: 'app' | 'desktop' | 'lock' | 'service',
    readonly app: string,
    readonly width: number,
    readonly height: number,
    readonly scale: number,
    readonly seconds: number | null,
    readonly shot: string | null,
    readonly shotAfterMs: number,
  ) {}

  static parse(argv: readonly string[]): LaunchArgs {
    const [command, ...given] = argv
    if (command !== 'app' && command !== 'desktop' && command !== 'lock' && command !== 'service') {
      throw new Error(`${command ?? 'nothing'} is not supported; ${LaunchArgs.USAGE}`)
    }
    const named = given[0] !== undefined && !given[0].startsWith('--')
    const app = named ? given[0]! : ''
    const rest = named ? given.slice(1) : given
    if (command === 'app' && !app) throw new Error(`app needs an id or a folder; ${LaunchArgs.USAGE}`)
    if (command === 'service' && !app) throw new Error(`service needs its name; ${LaunchArgs.USAGE}`)
    if (command === 'lock' && !app) throw new Error(`lock needs a desktop id or a folder; ${LaunchArgs.USAGE}`)
    const values = new Map<string, string>()
    for (let i = 0; i < rest.length; i += 2) {
      const flag = rest[i]!
      const value = rest[i + 1]
      if (!['--size', '--scale', '--seconds', '--shot', '--shot-after'].includes(flag) || value === undefined) throw new Error(`${flag}: ${LaunchArgs.USAGE}`)
      values.set(flag, value)
    }
    const size = /^(\d+)x(\d+)$/.exec(values.get('--size') ?? '800x600')
    if (!size) throw new Error(`--size ${values.get('--size')}: WxH`)
    const number = (flag: string, fallback: number) => {
      const value = Number(values.get(flag) ?? fallback)
      if (!(value > 0)) throw new Error(`${flag} ${values.get(flag)}: a positive number`)
      return value
    }
    const seconds = values.has('--seconds') ? number('--seconds', 1) : null
    const shot = values.get('--shot') ?? null
    return new LaunchArgs(command, app, Number(size[1]), Number(size[2]), number('--scale', 1), seconds, shot, number('--shot-after', 2000))
  }
}
