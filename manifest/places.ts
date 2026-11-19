import type { FsRootName, SharedPlace } from '../acde-api/src/index'

export const SHARED_PLACES: readonly SharedPlace[] = Object.freeze(['desktop', 'documents', 'downloads', 'music', 'pictures', 'videos'])

export const FS_ROOTS: readonly FsRootName[] = Object.freeze([...SHARED_PLACES, 'removable'])
