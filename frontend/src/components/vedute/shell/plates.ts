import plate1SmAvif from '@/assets/plate-1-sm.avif'
import plate1SmWebp from '@/assets/plate-1-sm.webp'
import plate1LgAvif from '@/assets/plate-1-lg.avif'
import plate1LgWebp from '@/assets/plate-1-lg.webp'
import plate2SmAvif from '@/assets/plate-2-sm.avif'
import plate2SmWebp from '@/assets/plate-2-sm.webp'
import plate2LgAvif from '@/assets/plate-2-lg.avif'
import plate2LgWebp from '@/assets/plate-2-lg.webp'
import plate3SmAvif from '@/assets/plate-3-sm.avif'
import plate3SmWebp from '@/assets/plate-3-sm.webp'
import plate3LgAvif from '@/assets/plate-3-lg.avif'
import plate3LgWebp from '@/assets/plate-3-lg.webp'
import plate4SmAvif from '@/assets/plate-4-sm.avif'
import plate4SmWebp from '@/assets/plate-4-sm.webp'
import plate4LgAvif from '@/assets/plate-4-lg.avif'
import plate4LgWebp from '@/assets/plate-4-lg.webp'

export type PlateId = 1 | 2 | 3 | 4

export type PlateAsset = {
  avifSrcSet: string
  webpSrcSet: string
  smAvif: string
  smWebp: string
  lgAvif: string
  lgWebp: string
  fallback: string
}

function makeAsset(smAvif: string, lgAvif: string, smWebp: string, lgWebp: string): PlateAsset {
  return {
    avifSrcSet: `${smAvif} 768w, ${lgAvif} 1440w`,
    webpSrcSet: `${smWebp} 768w, ${lgWebp} 1440w`,
    smAvif,
    smWebp,
    lgAvif,
    lgWebp,
    fallback: lgWebp,
  }
}

export const PLATES: Record<PlateId, PlateAsset> = {
  1: makeAsset(plate1SmAvif, plate1LgAvif, plate1SmWebp, plate1LgWebp),
  2: makeAsset(plate2SmAvif, plate2LgAvif, plate2SmWebp, plate2LgWebp),
  3: makeAsset(plate3SmAvif, plate3LgAvif, plate3SmWebp, plate3LgWebp),
  4: makeAsset(plate4SmAvif, plate4LgAvif, plate4SmWebp, plate4LgWebp),
}

export const DEFAULT_CARD_PLATE = PLATES[1]
