import { cn } from 'cn'
import type { BuildingId, ShellArea } from './areas'

type Point = [number, number]

function dotGrid(x: number, y: number, cols: number, rows: number, dx: number, dy: number): Point[] {
  const pts: Point[] = []
  for (let i = 0; i < cols; i++) {
    for (let j = 0; j < rows; j++) {
      pts.push([x + i * dx, y + j * dy])
    }
  }
  return pts
}

function dotRing(cx: number, cy: number, r: number, count: number): Point[] {
  const pts: Point[] = []
  for (let i = 0; i < count; i++) {
    const a = (i / count) * Math.PI * 2
    pts.push([Number((cx + r * Math.cos(a)).toFixed(1)), Number((cy + r * Math.sin(a)).toFixed(1))])
  }
  return pts
}

type BuildingGeometry = {
  id: BuildingId
  pin: Point
  columns: Point[]
  renderPoche: (className?: string) => React.ReactNode
}

/* Building footprints on a 220x150 Nolli plan (west / Capitoline at left), matching prototypes/c-interior/index.html. */
const BUILDING_GEOMETRY: BuildingGeometry[] = [
  {
    id: 'tabularium',
    pin: [20, 55],
    columns: dotGrid(12, 22, 3, 9, 8, 8),
    renderPoche: (c) => <rect x="8" y="16" width="24" height="78" className={c} />,
  },
  {
    id: 'saturn',
    pin: [51, 91],
    columns: dotGrid(46, 82, 2, 3, 9, 8),
    renderPoche: (c) => (
      <>
        <rect x="42" y="78" width="18" height="26" className={c} />
        <rect x="60" y="84" width="6" height="14" className={c} />
      </>
    ),
  },
  {
    id: 'rostra',
    pin: [53, 56],
    columns: [],
    renderPoche: (c) => (
      <>
        <path d="M40 52h26v9H40z" className={c} />
        <path
          d="M40 61q13 7 26 0"
          fill={c ? undefined : 'none'}
          stroke={c ? undefined : 'var(--color-ink)'}
          className={c}
        />
      </>
    ),
  },
  {
    id: 'curia',
    pin: [92, 24],
    columns: dotGrid(84, 15, 3, 3, 8, 8),
    renderPoche: (c) => <rect x="80" y="10" width="24" height="28" className={c} />,
  },
  {
    id: 'basilica',
    pin: [113, 120],
    columns: dotGrid(76, 110, 2, 10, 8.4, 20),
    renderPoche: (c) => <rect x="70" y="104" width="86" height="32" className={c} />,
  },
  {
    id: 'castor',
    pin: [175, 118],
    columns: dotGrid(168, 106, 2, 4, 14, 8),
    renderPoche: (c) => (
      <>
        <rect x="164" y="100" width="22" height="36" className={c} />
        <rect x="167" y="94" width="16" height="6" className={c} />
      </>
    ),
  },
  {
    id: 'vesta',
    pin: [196, 74],
    columns: dotRing(196, 74, 7.5, 10),
    renderPoche: (c) => <circle cx="196" cy="74" r="11" className={c} />,
  },
]

export type ForumPlanProps = {
  areas: ShellArea[]
  currentAreaId?: string
  hoveredBuilding?: BuildingId | null
  onHoverBuilding?: (building: BuildingId | null) => void
  onSelectArea?: (area: ShellArea) => void
  className?: string
}

/**
 * Nolli 1748-style figure-ground Forum plan as inline SVG (Figma Nav/Forum Plan 2267:710).
 * Hidden from assistive tech (`aria-hidden="true"`) so screen readers use the accessible legend list.
 */
export function ForumPlan({
  areas,
  currentAreaId,
  hoveredBuilding,
  onHoverBuilding,
  onSelectArea,
  className,
}: ForumPlanProps) {
  const byBuilding = new Map(areas.map((a) => [a.building, a]))
  const currentArea = areas.find((a) => a.id === currentAreaId)
  const currentGeo = currentArea ? BUILDING_GEOMETRY.find((b) => b.id === currentArea.building) : undefined

  return (
    <svg
      viewBox="0 0 220 150"
      aria-hidden="true"
      focusable="false"
      className={cn('block h-auto w-full select-none', className)}
    >
      {/* Pure white Forum plaza ground matching Figma Nav/Forum Plan 2267:710 */}
      <rect x="0.5" y="0.5" width="219" height="149" fill="var(--color-surface)" stroke="var(--color-ink-faint)" />

      {/* Surrounding urban fabric in warm deep paper matching Figma 2267:710 */}
      <g fill="var(--color-paper-deep)" stroke="var(--color-ink)" strokeWidth="0.5">
        <path d="M0 0h70v6H38v4H0z" />
        <path d="M112 0h108v40h-6V12h-60l-8-6h-34z" />
        <path d="M0 100h4v50H0z" />
        <path d="M60 140h96v10H60z" />
        <path d="M200 96h20v54h-30v-10h10z" />
        <path d="M118 44h30v20h-30z" />
      </g>

      {/* Via Sacra */}
      <path
        d="M32 66 C 70 70, 120 70, 150 68 S 200 62, 220 60"
        fill="none"
        stroke="var(--color-ink-soft)"
        strokeWidth="0.8"
        strokeDasharray="2.5 2"
      />
      <text
        x="124"
        y="80"
        fontFamily="var(--font-latin)"
        fontSize="6.5"
        letterSpacing="1.2"
        fill="var(--color-ink-soft)"
      >
        VIA SACRA
      </text>
      <text
        x="86"
        y="60"
        fontFamily="var(--font-latin)"
        fontSize="7"
        letterSpacing="1.6"
        fill="var(--color-ink-soft)"
      >
        FORVM
      </text>

      {/* Seven Forum buildings */}
      {BUILDING_GEOMETRY.map((geo) => {
        const area = byBuilding.get(geo.id)
        const isHere = area !== undefined && area.id === currentAreaId
        const isHover = area !== undefined && hoveredBuilding === geo.id
        const isInert = area === undefined

        return (
          <g
            key={geo.id}
            data-building={geo.id}
            data-here={isHere ? 'true' : undefined}
            className={cn(
              'vedute-plan-bld',
              isHere && 'is-here',
              isHover && 'is-hover',
              isInert && 'is-inert',
            )}
            onClick={area && onSelectArea ? () => onSelectArea(area) : undefined}
            onPointerEnter={area && onHoverBuilding ? () => onHoverBuilding(geo.id) : undefined}
            onPointerLeave={area && onHoverBuilding ? () => onHoverBuilding(null) : undefined}
          >
            <g className="vedute-plan-poche">{geo.renderPoche()}</g>
            {geo.columns.length > 0 && (
              <g className="vedute-plan-open" fill="var(--color-ink)">
                {geo.columns.map(([cx, cy], idx) => (
                  <circle key={idx} cx={cx} cy={cy} r="1" />
                ))}
              </g>
            )}
            {area && geo.renderPoche('vedute-plan-rim')}
          </g>
        )
      })}

      {/* Current building numeral pin */}
      {currentArea && currentGeo && (
        <g
          data-vedute-plan-pin=""
          transform={`translate(${currentGeo.pin[0]} ${currentGeo.pin[1]})`}
          className="pointer-events-none"
        >
          <circle r="7" fill="var(--color-paper)" stroke="var(--color-chalk)" strokeWidth="1.2" />
          <text
            fontFamily="var(--font-latin)"
            fontSize="8.5"
            fill="var(--color-chalk)"
            textAnchor="middle"
            dominantBaseline="central"
          >
            {currentArea.numeral}
          </text>
        </g>
      )}

      {/* Compass rose */}
      <g transform="translate(206 22)" fill="none" stroke="var(--color-ink)" strokeWidth="0.7">
        <path d="M0 -9 L3 3 L0 1 L-3 3 Z" fill="var(--color-ink)" />
        <text
          y="-11"
          fontFamily="var(--font-latin)"
          fontSize="6"
          textAnchor="middle"
          stroke="none"
          fill="var(--color-ink)"
        >
          N
        </text>
      </g>

      {/* Scale bar */}
      <g transform="translate(10 140)" stroke="var(--color-ink)" strokeWidth="0.7">
        <path d="M0 0h40M0 -2v4M20 -2v4M40 -2v4" />
        <text x="44" y="2" fontFamily="var(--font-latin)" fontSize="5.5" stroke="none" fill="var(--color-ink-soft)">
          PASSVS C
        </text>
      </g>
    </svg>
  )
}
