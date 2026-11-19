export type SharedPlace = 'desktop' | 'documents' | 'downloads' | 'music' | 'pictures' | 'videos'

export type PlaceCapability = `fs.${SharedPlace}`

export type FsRootName = SharedPlace | 'removable'
