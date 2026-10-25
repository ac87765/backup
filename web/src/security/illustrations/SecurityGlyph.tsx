import type { SVGProps } from 'react'

export type SecurityGlyphKind =
  | 'eye'
  | 'lock'
  | 'code'
  | 'device'
  | 'desktop'
  | 'paper'
  | 'copies'
  | 'vault'
  | 'no-share'
  | 'rotate'
  | 'checklist'

export interface SecurityGlyphProps {
  kind: SecurityGlyphKind
  size?: number
  /** 提供时图标作为有含义的图形暴露给读屏；否则视为装饰。 */
  title?: string
  className?: string
}

/**
 * “安全与隐私”页的小插图（design D1.2 / D2）。
 *
 * 全部为内联 SVG、线宽 1.75、`currentColor` 描边，颜色由调用方的
 * D2 令牌文本类决定；不加载任何外部资源，也不使用 SMIL。
 */
export function SecurityGlyph({
  kind,
  size = 20,
  title,
  className,
}: SecurityGlyphProps) {
  const accessibility: Pick<SVGProps<SVGSVGElement>, 'aria-hidden' | 'role'> =
    title
      ? { role: 'img' }
      : { 'aria-hidden': true }

  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.75}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      {...accessibility}
    >
      {title && <title>{title}</title>}
      {kind === 'eye' && (
        <>
          <path d="M2.75 12s3.4-5.75 9.25-5.75S21.25 12 21.25 12 17.85 17.75 12 17.75 2.75 12 2.75 12Z" />
          <circle cx="12" cy="12" r="2.75" />
        </>
      )}
      {kind === 'lock' && (
        <>
          <rect x="4.75" y="10.25" width="14.5" height="9.5" rx="2.25" />
          <path d="M8.25 10.25V7.5a3.75 3.75 0 0 1 7.5 0v2.75" />
          <circle cx="12" cy="14.75" r="1.15" />
          <path d="M12 15.9v1.6" />
        </>
      )}
      {kind === 'code' && (
        <>
          <rect x="3.25" y="4.25" width="17.5" height="15.5" rx="2.5" />
          <path d="m8.5 9.5-2.4 2.5 2.4 2.5" />
          <path d="m15.5 9.5 2.4 2.5-2.4 2.5" />
          <path d="m13.1 8.5-2.2 7" />
        </>
      )}
      {kind === 'device' && (
        <>
          <rect x="3.25" y="4.25" width="17.5" height="12.5" rx="2.25" />
          <path d="M8.5 20.25h7" />
          <path d="M12 16.75v3.5" />
          <path d="M12 7.6 15.5 9v2.55c0 2.05-1.4 3.5-3.5 4.1-2.1-.6-3.5-2.05-3.5-4.1V9L12 7.6Z" />
        </>
      )}
      {kind === 'desktop' && (
        <>
          <rect x="3.25" y="4.25" width="17.5" height="12.5" rx="2.25" />
          <path d="M8.5 20.25h7" />
          <path d="M12 16.75v3.5" />
          <circle cx="16.1" cy="9.2" r="2.6" />
          <path d="m15 9.2 0.8 0.8 1.4-1.5" />
        </>
      )}
      {kind === 'paper' && (
        <>
          <path d="M6.25 3.75h7.5l4 4v12.5h-11.5Z" />
          <path d="M13.75 3.75v4h4" />
          <path d="M9.25 12.25h5.5" />
          <path d="M9.25 15.75h4" />
        </>
      )}
      {kind === 'copies' && (
        <>
          <rect x="8.25" y="3.75" width="10.5" height="13.5" rx="1.75" />
          <path d="M6.25 6.75H5.5a1.75 1.75 0 0 0-1.75 1.75v11a1.75 1.75 0 0 0 1.75 1.75h7.5a1.75 1.75 0 0 0 1.75-1.75v-.75" />
          <path d="M11.25 8.25h4.5" />
          <path d="M11.25 11.25h4.5" />
        </>
      )}
      {kind === 'vault' && (
        <>
          <rect x="3.25" y="4.75" width="17.5" height="14.5" rx="2.5" />
          <circle cx="12" cy="12" r="3.6" />
          <path d="M12 8.4V6.9" />
          <path d="M12 17.1v-1.5" />
          <path d="M15.6 12h1.5" />
          <path d="M6.9 12h1.5" />
          <path d="m14.55 9.45 1.05-1.05" />
          <path d="m8.4 15.6 1.05-1.05" />
          <path d="m14.55 14.55 1.05 1.05" />
          <path d="m8.4 8.4 1.05 1.05" />
        </>
      )}
      {kind === 'no-share' && (
        <>
          <path d="M20.75 3.25 3.25 20.75" />
          <path d="m8.2 13.1-2.95 1.3a1.2 1.2 0 0 1-1.6-1.25l.45-4.1a1.2 1.2 0 0 1 .9-1.02L18.9 4.5a1.2 1.2 0 0 1 1.5 1.5l-3.1 12.05a1.2 1.2 0 0 1-1.55.82L8.2 16.2" />
          <path d="m8.2 13.1 5.55-5.55" />
        </>
      )}
      {kind === 'rotate' && (
        <>
          <path d="M20 11.25a8 8 0 1 0-2.35 5.65" />
          <path d="M20.25 5.25v6h-6" />
          <path d="M9.25 12.35 11 14.1l3.75-3.75" />
        </>
      )}
      {kind === 'checklist' && (
        <>
          <rect x="5.25" y="4.25" width="13.5" height="16" rx="2" />
          <path d="M9 3.25h6v2.5H9Z" />
          <path d="m8.75 10.25 1.25 1.25 2.25-2.5" />
          <path d="m8.75 15.25 1.25 1.25 2.25-2.5" />
          <path d="M14.75 11h1.5" />
          <path d="M14.75 16h1.5" />
        </>
      )}
    </svg>
  )
}
