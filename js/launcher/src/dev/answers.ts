import type { InspectAction, InspectArg, InspectMatch, InspectNode, InspectWindow } from '@acde/api'

export interface SnapshotOptions {
  readonly window?: string
  readonly ref?: string
  readonly all?: boolean
  readonly diff?: boolean
  readonly limit?: number
  readonly find?: string
  readonly tree?: boolean
  readonly offscreen?: boolean
}

export interface QueryOptions {
  readonly selector: string
  readonly window?: string
  readonly ref?: string
  readonly limit?: number
  readonly attributes?: readonly string[]
}

export interface Query {
  readonly window: string
  readonly count: number
  readonly nodes: readonly InspectMatch[]
}

export interface Snapshot {
  readonly text: string
  readonly windows: readonly InspectWindow[]
  readonly trees: Readonly<Record<string, readonly InspectNode[]>>
}

export interface InspectStep {
  readonly ref: string
  readonly action: InspectAction
  readonly text?: InspectArg
}

export interface ActResult {
  readonly ok: boolean
  readonly text: string
  readonly windows?: readonly InspectWindow[]
  readonly expanded?: readonly string[]
}

export function looked(after: Snapshot): Pick<ActResult, 'windows' | 'expanded'> {
  return { windows: after.windows, expanded: Object.keys(after.trees) }
}
