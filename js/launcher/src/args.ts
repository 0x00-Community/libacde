export class LaunchArgs {
  static readonly USAGE = 'the single-app launcher: app <id|dir> [--size WxH] [--scale N] [--seconds N] [--shot FILE] [--shot-after MS]'

  constructor(
    readonly app: string,
    readonly width: number,
    readonly height: number,
    readonly scale: number,
    readonly seconds: number | null,
    readonly shot: string | null,
    readonly shotAfterMs: number,
  ) {}

  static parse(argv: readonly string[]): LaunchArgs {
    const [command, app, ...rest] = argv
    if (command !== 'app' || !app) throw new Error(`${command ?? 'nothing'} is not supported; ${LaunchArgs.USAGE}`)
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
    return new LaunchArgs(app, Number(size[1]), Number(size[2]), number('--scale', 1), seconds, values.get('--shot') ?? null, number('--shot-after', 2000))
  }
}
