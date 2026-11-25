import fc from 'fast-check'
import { describe, expect, it } from 'vitest'
import { ratioPercent, stackBarProportions, stageForRound, toRoman } from './scales'

const ROMAN_VALUES: Record<string, number> = {
  I: 1,
  V: 5,
  X: 10,
  L: 50,
  C: 100,
  D: 500,
  M: 1000,
}

function fromRoman(roman: string): number {
  let total = 0
  for (let i = 0; i < roman.length; i++) {
    const curr = ROMAN_VALUES[roman[i]!] ?? 0
    const next = ROMAN_VALUES[roman[i + 1] ?? ''] ?? 0
    total += curr < next ? -curr : curr
  }
  return total
}

describe('stageForRound', () => {
  it('maps boundary rounds according to DESIGN.md (<=0..5 clarify, 6..12 argue, >=13 challenge)', () => {
    expect(stageForRound(-5)).toBe(1)
    expect(stageForRound(0)).toBe(1)
    expect(stageForRound(1)).toBe(1)
    expect(stageForRound(5)).toBe(1)
    expect(stageForRound(6)).toBe(2)
    expect(stageForRound(12)).toBe(2)
    expect(stageForRound(13)).toBe(3)
    expect(stageForRound(25)).toBe(3)
    expect(stageForRound(Number.NaN)).toBe(1)
    expect(stageForRound(Number.NEGATIVE_INFINITY)).toBe(1)
    expect(stageForRound(Number.POSITIVE_INFINITY)).toBe(3)
  })

  it('always returns a valid stage and is monotonically non-decreasing', () => {
    fc.assert(
      fc.property(fc.integer({ min: -100, max: 200 }), fc.integer({ min: -100, max: 200 }), (a, b) => {
        const low = Math.min(a, b)
        const high = Math.max(a, b)
        const sLow = stageForRound(low)
        const sHigh = stageForRound(high)
        expect([1, 2, 3]).toContain(sLow)
        expect([1, 2, 3]).toContain(sHigh)
        expect(sLow).toBeLessThanOrEqual(sHigh)
      }),
    )
  })
})

describe('stackBarProportions', () => {
  it('returns all zeros for empty, non-positive, or non-finite inputs', () => {
    expect(stackBarProportions([])).toEqual([])
    expect(stackBarProportions([0, 0, 0])).toEqual([0, 0, 0])
    expect(stackBarProportions([-5, Number.NaN, Number.POSITIVE_INFINITY])).toEqual([0, 0, 0])
  })

  it('produces valid percentages summing to 100 whenever any segment is positive', () => {
    fc.assert(
      fc.property(
        fc.array(fc.double({ min: -1000, max: 1000, noNaN: true, noDefaultInfinity: true }), { maxLength: 12 }),
        (values) => {
          const result = stackBarProportions(values)
          expect(result).toHaveLength(values.length)
          for (let i = 0; i < values.length; i++) {
            const p = result[i]!
            expect(Number.isFinite(p)).toBe(true)
            expect(p).toBeGreaterThanOrEqual(0)
            expect(p).toBeLessThanOrEqual(100)
            if (values[i]! <= 0) expect(p).toBe(0)
          }

          const hasPositive = values.some((v) => v > 0)
          const sum = result.reduce((acc, p) => acc + p, 0)
          if (hasPositive) {
            expect(sum).toBeCloseTo(100, 6)
          } else {
            expect(sum).toBe(0)
          }
        },
      ),
    )
  })

  it('preserves ordering and is invariant under positive scaling', () => {
    fc.assert(
      fc.property(
        fc.array(fc.integer({ min: 0, max: 500 }), { minLength: 1, maxLength: 8 }),
        fc.integer({ min: 1, max: 100 }),
        (values, scale) => {
          const base = stackBarProportions(values)
          const scaled = stackBarProportions(values.map((v) => v * scale))
          for (let i = 0; i < values.length; i++) {
            expect(scaled[i]).toBeCloseTo(base[i]!, 6)
            for (let j = 0; j < values.length; j++) {
              if (values[i]! <= values[j]!) {
                expect(base[i]!).toBeLessThanOrEqual(base[j]! + 1e-9)
              }
            }
          }
        },
      ),
    )
  })
})

describe('ratioPercent', () => {
  it('stays within [0, 100] and is monotonic in part for positive totals', () => {
    expect(ratioPercent(22, 28)).toBeCloseTo((22 / 28) * 100, 6)
    expect(ratioPercent(0, 0)).toBe(0)
    expect(ratioPercent(5, 0)).toBe(0)
    expect(ratioPercent(Number.NaN, 10)).toBe(0)

    fc.assert(
      fc.property(
        fc.double({ min: -500, max: 500, noNaN: true, noDefaultInfinity: true }),
        fc.double({ min: -500, max: 500, noNaN: true, noDefaultInfinity: true }),
        fc.double({ min: -500, max: 500, noNaN: true, noDefaultInfinity: true }),
        (a, b, total) => {
          const low = Math.min(a, b)
          const high = Math.max(a, b)
          const pLow = ratioPercent(low, total)
          const pHigh = ratioPercent(high, total)
          expect(pLow).toBeGreaterThanOrEqual(0)
          expect(pLow).toBeLessThanOrEqual(100)
          expect(pHigh).toBeGreaterThanOrEqual(0)
          expect(pHigh).toBeLessThanOrEqual(100)
          if (total <= 0) {
            expect(pLow).toBe(0)
            expect(pHigh).toBe(0)
          } else {
            expect(pLow).toBeLessThanOrEqual(pHigh)
            if (low <= 0) expect(pLow).toBe(0)
            if (high >= total) expect(pHigh).toBe(100)
          }
        },
      ),
    )
  })
})

describe('toRoman', () => {
  it('formats standard numerals and round-trips across 1..3999', () => {
    expect(toRoman(1)).toBe('I')
    expect(toRoman(4)).toBe('IV')
    expect(toRoman(9)).toBe('IX')
    expect(toRoman(12)).toBe('XII')
    expect(toRoman(42)).toBe('XLII')
    expect(toRoman(2026)).toBe('MMXXVI')

    fc.assert(
      fc.property(fc.integer({ min: 1, max: 3999 }), (n) => {
        const roman = toRoman(n)
        expect(roman).toMatch(/^[IVXLCDM]+$/)
        expect(fromRoman(roman)).toBe(n)
      }),
    )
  })
})
