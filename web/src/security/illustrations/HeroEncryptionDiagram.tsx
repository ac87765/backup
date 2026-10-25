import { useMediaQuery, cx } from '../../ui'
import './hero.css'

export interface HeroEncryptionDiagramProps {
  className?: string
}

const HERO_LABEL =
  '原理图：文件在你的浏览器里用钥匙加密并上锁，只有上锁的盒子沿虚线送到服务器；服务器只存放锁着的盒子，钥匙始终留在你这边。'

/**
 * 首屏原理图（design D1.1 / D2）。
 *
 * 纯内联 SVG + CSS keyframes：文件出现 → 锁扣合上 → 上锁的盒子沿虚线
 * 移动到服务器。`prefers-reduced-motion: reduce` 时直接渲染终态静帧，
 * 不挂任何动画 class。
 */
export function HeroEncryptionDiagram({
  className,
}: HeroEncryptionDiagramProps) {
  const reducedMotion = useMediaQuery('(prefers-reduced-motion: reduce)')
  const animated = !reducedMotion

  return (
    <svg
      data-testid="security-hero"
      data-motion={animated ? 'animated' : 'static'}
      role="img"
      aria-label={HERO_LABEL}
      viewBox="0 0 720 300"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.75}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={cx(
        'security-hero block h-auto w-full',
        animated && 'security-hero-animated',
        className,
      )}
    >
      {/* 柔软的底与装饰点，颜色全部来自 D2 令牌。 */}
      <rect x="1" y="1" width="718" height="298" rx="20" className="fill-canvas" />
      <circle cx="292" cy="52" r="5" className="fill-brand-100" />
      <circle cx="372" cy="250" r="4" className="fill-brand-100" />
      <circle cx="454" cy="86" r="3" className="fill-brand-100" />
      <path
        d="M300 232c18-12 36-12 54 0"
        className="stroke-line"
        opacity="0.8"
      />

      {/* 浏览器 → 服务器 的虚线路径。 */}
      <g className="text-line-strong">
        <path
          className="hero-dash"
          d="M256 160h220"
          strokeDasharray="4 8"
        />
        <path d="m470 154 9 6-9 6" />
      </g>

      {/* 左侧：你的浏览器。 */}
      <g className="text-ink-500">
        <rect
          x="40"
          y="64"
          width="200"
          height="150"
          rx="14"
          className="fill-white"
        />
        <path d="M40 92h200" />
        <circle cx="56" cy="78" r="2.4" className="fill-ink-400" stroke="none" />
        <circle cx="66" cy="78" r="2.4" className="fill-ink-400" stroke="none" />
        <circle cx="76" cy="78" r="2.4" className="fill-ink-400" stroke="none" />
        <path d="M58 116h26" opacity="0.55" />
        <path d="M58 130h18" opacity="0.55" />
        <path d="M58 144h22" opacity="0.55" />
        <path d="M58 184h74" opacity="0.35" />

        {/* 文件进入浏览器。 */}
        <g transform="translate(112 108)">
          <g className="hero-file text-ink-500">
            <path d="M0 0h26l10 10v36H0Z" className="fill-white" />
            <path d="M26 0v10h10" />
            <path d="M8 22h18" />
            <path d="M8 30h14" />
            <path d="M8 38h10" opacity="0.6" />
          </g>

          {/* 锁扣合上。 */}
          <g transform="translate(7 24)">
            <g className="hero-lock text-brand-600">
              <rect
                x="0"
                y="7"
                width="22"
                height="17"
                rx="4"
                className="fill-white"
              />
              <path d="M5 7V4.5a6 6 0 0 1 12 0V7" />
              <circle cx="11" cy="15" r="1.5" className="fill-brand-600" stroke="none" />
              <path d="M11 16.5V20" />
            </g>
          </g>
        </g>
      </g>

      {/* 钥匙留在浏览器这一侧。 */}
      <g transform="translate(54 238)" className="text-brand-600">
        <circle cx="9" cy="12" r="7.25" />
        <circle cx="9" cy="12" r="2.2" />
        <path d="M16.25 12h21" />
        <path d="M31 12v6" />
        <path d="M36.75 12v4.25" />
        <path
          d="M10 -2c2-10 8-16 16-20"
          className="stroke-brand-100"
          strokeDasharray="2 4"
        />
      </g>

      {/* 右侧：云与机柜。 */}
      <g className="text-ink-500">
        <path
          d="M604 108c-11 0-19-7-19-17 0-8 6-14 14-15 2-12 12-20 24-20 11 0 20 6 24 16 10-1 18 6 18 16 0 9-8 17-18 17Z"
          className="fill-white"
        />
        <path d="M608 116c8 4 16 6 25 6 8 0 16-2 23-6" opacity="0.45" />

        <rect x="606" y="124" width="84" height="130" rx="10" className="fill-white" />
        <path d="M606 158h84" />
        <path d="M606 192h84" />
        <path d="M606 226h84" />
        <circle cx="619" cy="141" r="2.5" className="fill-brand-600" stroke="none" />
        <path d="M631 141h38" opacity="0.6" />
        <circle cx="619" cy="175" r="2.5" className="fill-success-600" stroke="none" />
        <path d="M631 175h30" opacity="0.6" />
        <path d="M631 209h38" opacity="0.6" />
        <path d="M631 241h22" opacity="0.6" />
      </g>

      {/* 服务器脚边的锁箱堆。 */}
      <LockedBox x={500} y={190} className="text-ink-500" />
      <LockedBox x={548} y={190} className="text-ink-500" />
      <LockedBox x={524} y={236} className="text-ink-500" />

      {/* 新落下的锁箱（动画的终态）。 */}
      <g transform="translate(500 140)">
        <g className="hero-pile-new">
          <LockedBoxShapes />
        </g>
      </g>

      {/* 沿虚线移动的锁箱；终态与上方新锁箱重合。 */}
      <g transform="translate(120 140)">
        <g className="hero-travel text-brand-600">
          <LockedBoxShapes />
        </g>
      </g>
    </svg>
  )
}

function LockedBox({
  x,
  y,
  className,
}: {
  x: number
  y: number
  className?: string
}) {
  return (
    <g transform={`translate(${x} ${y})`} className={className}>
      <LockedBoxShapes />
    </g>
  )
}

function LockedBoxShapes() {
  return (
    <>
      <rect
        x="0"
        y="0"
        width="52"
        height="42"
        rx="6"
        className="fill-white"
      />
      <path d="M0 12h52" opacity="0.55" />
      <rect
        x="20"
        y="16"
        width="12"
        height="10"
        rx="2.5"
        className="fill-white"
      />
      <path d="M23 16v-3a3 3 0 0 1 6 0v3" />
      <circle cx="26" cy="21" r="1" className="fill-current" stroke="none" />
      <path d="M8 31h20" opacity="0.45" />
    </>
  )
}
