import { cx } from '../../ui'

export type KeyState = 'present' | 'lost'

export interface KeyPairProps {
  password: KeyState
  recovery: KeyState
  className?: string
}

function keyLabel(state: KeyState): string {
  return state === 'lost' ? '已丢失' : '完好'
}

/**
 * “忘记密码怎么办”三格对照里的两把钥匙（design D1.4 / D2）。
 *
 * 丢失的钥匙画成虚线轮廓并加一道斜杠；颜色走 D2 令牌。
 * 图形整体通过 aria-label 描述，避免在卡片里重复可见文字。
 */
export function KeyPair({ password, recovery, className }: KeyPairProps) {
  return (
    <svg
      viewBox="0 0 128 52"
      width="128"
      height="52"
      fill="none"
      strokeWidth={1.75}
      strokeLinecap="round"
      strokeLinejoin="round"
      role="img"
      aria-label={`两把钥匙：密码钥匙${keyLabel(password)}，恢复密钥${keyLabel(recovery)}`}
      className={cx('h-12 w-32', className)}
    >
      <g
        data-state={password}
        className={password === 'lost' ? 'text-ink-400' : 'text-brand-600'}
      >
        <g strokeDasharray={password === 'lost' ? '3 3' : undefined}>
          <KeyMark />
        </g>
        {password === 'lost' && <LostSlash />}
      </g>
      <g
        data-state={recovery}
        className={recovery === 'lost' ? 'text-ink-400' : 'text-brand-600'}
        transform="translate(64 0)"
      >
        <g strokeDasharray={recovery === 'lost' ? '3 3' : undefined}>
          <KeyMark />
        </g>
        {recovery === 'lost' && <LostSlash />}
      </g>
    </svg>
  )
}

function KeyMark() {
  return (
    <>
      <circle cx="14" cy="26" r="8.25" stroke="currentColor" />
      <circle cx="14" cy="26" r="2.5" stroke="currentColor" />
      <path d="M22.25 26h24" stroke="currentColor" />
      <path d="M39.5 26v6.25" stroke="currentColor" />
      <path d="M45.25 26v4.5" stroke="currentColor" />
    </>
  )
}

function LostSlash() {
  return (
    <path
      d="M4 45.5 50 6.5"
      stroke="currentColor"
      className="text-danger-600"
      strokeWidth={1.75}
    />
  )
}
