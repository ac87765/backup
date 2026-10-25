import type { NodeKind } from '../worker/types'
import { cx } from './cx'
import { FILE_VISUAL_META, fileVisualKind } from './file-visual'

const ICON_SIZES = { sm: 16, md: 20, lg: 28, xl: 40 } as const

export interface FileIconProps {
  name: string
  kind: NodeKind
  size?: keyof typeof ICON_SIZES
  className?: string
}

export function FileIcon({
  name,
  kind,
  size = 'md',
  className,
}: FileIconProps) {
  const visual = fileVisualKind(name, kind)
  const meta = FILE_VISUAL_META[visual]
  const Icon = meta.Icon
  return (
    <span
      aria-hidden="true"
      data-file-kind={visual}
      className={cx(
        'inline-flex shrink-0 items-center justify-center',
        meta.className,
        className,
      )}
    >
      <Icon size={ICON_SIZES[size]} strokeWidth={1.75} focusable="false" />
    </span>
  )
}
