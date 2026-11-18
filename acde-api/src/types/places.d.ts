export type SharedPlace = 'desktop' | 'documents' | 'downloads' | 'music' | 'pictures' | 'videos'

export type PlaceCapability = `fs.${SharedPlace}`
