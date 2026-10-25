import { useState } from 'react'
import { ChevronDown, ChevronUp, X } from 'lucide-react'

import {
  Button,
  cx,
  FileIcon,
  IconButton,
  prefersReducedMotion,
  ProgressBar,
} from '../ui'
import type {
  TransferDirection,
  TransferStatus,
} from '../worker/types'
import { formatBytes } from './format'

/** 主线程可见的队列项（不含密钥、明文或密文）。 */
export interface TransferItemView {
  localId: string
  direction: TransferDirection
  fileName: string
  status: TransferStatus
  completedChunks: number
  totalChunks: number
  completedBytes: number
  totalBytes: number
  errorCode?: string
  errorMessage?: string
  sessionId?: string
  downloadId?: string
  /** 上传目标父目录（新建文件重试用）。 */
  parentId?: string
  /** 下载重试所需的节点/版本信息；新版本上传时为目标节点。 */
  nodeId?: string
  versionId?: string
  mime?: string
  size?: number
  /** 仅上传重试时保留；刷新后为 undefined（需重新选择文件）。 */
  file?: File
  pendingResume?: boolean
}

const STATUS_LABELS: Record<TransferStatus, string> = {
  pending: '等待中',
  uploading: '上传中',
  downloading: '下载中',
  paused: '已暂停',
  retrying: '重试中',
  completed: '已完成',
  failed: '失败',
  cancelled: '已取消',
}

const TERMINAL_STATUSES: TransferStatus[] = [
  'completed',
  'failed',
  'cancelled',
]

function isTerminal(status: TransferStatus): boolean {
  return TERMINAL_STATUSES.includes(status)
}

function itemPercent(item: TransferItemView): number {
  if (item.totalChunks > 0) {
    return Math.min(
      100,
      Math.round((item.completedChunks / item.totalChunks) * 100),
    )
  }
  if (item.totalBytes > 0) {
    return Math.min(
      100,
      Math.round((item.completedBytes / item.totalBytes) * 100),
    )
  }
  return item.status === 'completed' ? 100 : 0
}

function overallPercent(items: TransferItemView[]): number {
  const totalBytes = items.reduce((sum, item) => sum + item.totalBytes, 0)
  if (totalBytes > 0) {
    const completed = items.reduce(
      (sum, item) => sum + Math.min(item.completedBytes, item.totalBytes),
      0,
    )
    return Math.min(100, Math.round((completed / totalBytes) * 100))
  }
  if (items.length === 0) {
    return 0
  }
  return Math.round(
    items.reduce((sum, item) => sum + itemPercent(item), 0) / items.length,
  )
}

function progressTone(
  status: TransferStatus,
): 'brand' | 'success' | 'danger' {
  if (status === 'completed') {
    return 'success'
  }
  if (status === 'failed' || status === 'cancelled') {
    return 'danger'
  }
  return 'brand'
}

export interface TransferPanelProps {
  items: TransferItemView[]
  onPause: (item: TransferItemView) => void
  onResume: (item: TransferItemView) => void
  onCancel: (item: TransferItemView) => void
  onRetry: (item: TransferItemView) => void
  onResumeFile: (item: TransferItemView, file: File) => void
  onDismiss: (item: TransferItemView) => void
  onDismissCompleted: () => void
}

export function TransferPanel({
  items,
  onPause,
  onResume,
  onCancel,
  onRetry,
  onResumeFile,
  onDismiss,
  onDismissCompleted,
}: TransferPanelProps) {
  const [collapsed, setCollapsed] = useState(false)
  const [closed, setClosed] = useState(false)
  const [closing, setClosing] = useState(false)

  const closePanel = () => {
    setClosing(true)
    const delay = prefersReducedMotion() ? 0 : 180
    window.setTimeout(() => {
      setClosed(true)
      setClosing(false)
    }, delay)
  }

  if (items.length === 0) {
    return null
  }

  const activeCount = items.filter(
    (item) => !isTerminal(item.status),
  ).length
  const allTerminal = items.every((item) => isTerminal(item.status))
  const allCompleted = items.every((item) => item.status === 'completed')
  const progress = allCompleted ? 100 : overallPercent(items)
  const title =
    activeCount > 0
      ? `正在传输 ${activeCount} 项 · ${progress}%`
      : allCompleted
        ? '传输完成'
        : '传输结束'

  if (closed) {
    return (
      <button
        type="button"
        data-testid="transfer-panel-reopen"
        aria-label="打开传输面板"
        onClick={() => setClosed(false)}
        className="fixed bottom-4 right-4 z-40 flex h-11 items-center gap-2 rounded-full bg-brand-600 px-4 text-sm font-medium text-white shadow-pop transition-colors motion-fast hover:bg-brand-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600 focus-visible:ring-offset-2 sm:bottom-6 sm:right-6"
      >
        {title}
      </button>
    )
  }

  return (
    <section
      data-testid="transfer-panel"
      data-collapsed={collapsed ? 'true' : 'false'}
      aria-label="传输面板"
      className={cx(
        'fixed inset-x-0 bottom-0 z-40 overflow-hidden border-t border-line bg-white shadow-pop sm:inset-x-auto sm:bottom-6 sm:right-6 sm:w-[360px] sm:rounded-card sm:border',
        closing
          ? 'animate-panel-out pointer-events-none'
          : 'animate-panel-in',
      )}
    >
      <header className="flex items-center gap-1.5 px-4 py-3">
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-semibold text-ink-900">
            {title}
          </p>
          <div className="mt-1.5">
            <ProgressBar
              value={progress}
              tone={allTerminal && !allCompleted ? 'danger' : 'brand'}
              label="总体传输进度"
            />
          </div>
        </div>
        {allTerminal && (
          <Button
            size="sm"
            variant="ghost"
            data-testid="transfer-clear-completed"
            onClick={onDismissCompleted}
          >
            清除已完成
          </Button>
        )}
        <IconButton
          size="sm"
          label={collapsed ? '展开传输面板' : '折叠传输面板'}
          data-testid="transfer-collapse"
          onClick={() => setCollapsed((value) => !value)}
        >
          {collapsed ? <ChevronUp size={17} /> : <ChevronDown size={17} />}
        </IconButton>
        <IconButton
          size="sm"
          label="关闭传输面板"
          data-testid="transfer-close"
          onClick={closePanel}
        >
          <X size={17} />
        </IconButton>
      </header>

      <div
        className={cx(
          'grid transition-[grid-template-rows] motion-base',
          collapsed ? 'grid-rows-[0fr]' : 'grid-rows-[1fr]',
        )}
      >
        <div className="overflow-hidden">
        <ul className="max-h-72 overflow-y-auto border-t border-line">
          {items.map((item) => {
            const active =
              item.status === 'uploading' ||
              item.status === 'downloading' ||
              item.status === 'pending'
            const paused = item.status === 'paused'
            const canPause = active && (item.sessionId || item.downloadId)
            const canCancel =
              (item.direction === 'upload'
                ? Boolean(item.sessionId)
                : Boolean(item.downloadId)) &&
              (active || paused)
            const percent = itemPercent(item)
            const detail =
              item.totalBytes > 0
                ? `${formatBytes(item.completedBytes)} / ${formatBytes(item.totalBytes)}`
                : item.totalChunks > 0
                  ? `${item.completedChunks}/${item.totalChunks} 块`
                  : ''
            return (
              <li
                key={item.localId}
                data-testid="transfer-item"
                data-direction={item.direction}
                data-status={item.status}
                data-name={item.fileName}
                className="space-y-2 border-b border-line px-4 py-3 last:border-b-0"
              >
                <div className="flex items-start gap-2.5">
                  <span className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-control bg-canvas">
                    <FileIcon
                      name={item.fileName}
                      kind="file"
                      size="md"
                    />
                  </span>
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-medium text-ink-900">
                      {item.fileName}
                    </p>
                    <p className="mt-0.5 text-xs leading-5 text-ink-600">
                      {STATUS_LABELS[item.status]}
                      {detail ? ` · ${detail}` : ''}
                    </p>
                  </div>
                </div>

                <ProgressBar
                  value={percent}
                  tone={progressTone(item.status)}
                  label={`${item.fileName} 传输进度`}
                  testId="transfer-progress"
                />

                <div className="flex flex-wrap items-center justify-end gap-1">
                  {item.direction === 'upload' ? (
                    <span className="mr-auto text-xs text-ink-500">
                      上传
                    </span>
                  ) : (
                    <span className="mr-auto text-xs text-ink-500">
                      下载
                    </span>
                  )}
                  {canPause && (
                    <Button
                      size="sm"
                      variant="ghost"
                      data-testid="transfer-pause"
                      onClick={() => onPause(item)}
                    >
                      暂停
                    </Button>
                  )}
                  {paused && !item.pendingResume && (
                    <Button
                      size="sm"
                      variant="ghost"
                      data-testid="transfer-resume"
                      onClick={() => onResume(item)}
                    >
                      继续
                    </Button>
                  )}
                  {item.status === 'failed' && !item.pendingResume && (
                    <Button
                      size="sm"
                      variant="ghost"
                      data-testid="transfer-retry"
                      onClick={() => onRetry(item)}
                    >
                      重试
                    </Button>
                  )}
                  {(active || paused) && canCancel && (
                    <Button
                      size="sm"
                      variant="dangerGhost"
                      data-testid="transfer-cancel"
                      onClick={() => onCancel(item)}
                    >
                      取消
                    </Button>
                  )}
                  {isTerminal(item.status) && (
                    <Button
                      size="sm"
                      variant="ghost"
                      data-testid="transfer-dismiss"
                      onClick={() => onDismiss(item)}
                    >
                      清除
                    </Button>
                  )}
                </div>

                {item.pendingResume && (
                  <label className="block text-xs leading-5 text-ink-600">
                    重新选择同一文件以继续
                    <input
                      type="file"
                      data-testid={`resume-upload-input-${item.sessionId ?? ''}`}
                      className="mt-1 block w-full text-xs text-ink-900"
                      onChange={(event) => {
                        const file = event.target.files?.[0]
                        if (file) {
                          onResumeFile(item, file)
                        }
                        event.target.value = ''
                      }}
                    />
                  </label>
                )}
                {item.errorMessage && (
                  <p role="alert" className="text-xs leading-5 text-danger-600">
                    {item.errorMessage}
                  </p>
                )}
              </li>
            )
          })}
        </ul>
        </div>
      </div>
    </section>
  )
}
