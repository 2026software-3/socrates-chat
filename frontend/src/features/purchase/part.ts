export type Part = { id: string; name_zh_tw: string; name_en: string; name_es: string; price: number; enabled: boolean }
export type PartView = Part & { owned: boolean }
export function partName(part: Part, lang: string) { return lang === 'en' ? part.name_en : lang === 'es' ? part.name_es : part.name_zh_tw }
