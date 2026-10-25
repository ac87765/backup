import { useState } from 'react'
import { ChevronDown, ChevronUp, Folder, X } from 'lucide-react'

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
import type { FolderJobStatus, FolderJobView } from './folder-job'
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
  retrying: '服务器繁忙，稍后自动继续',
  completed: '已完成',
  failed: '失败',
  cancelled: '已取消',
}

const FOLDER_STATUS_LABELS: Record<FolderJobStatus, string> = {
  scanning: '正在扫描…',
  planning: '正在准备…',
  running: '备份中',
  done: '备份完成',
  cancelled: '已取消',
  failed: '任务结束',
}

const FOLDER_ACTIVE: FolderJobStatus[] = ['scanning', 'planning', 'running']

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
  /** 文件夹上传/下载任务分组（design D6）。 */
  folderJobs?: FolderJobView[]
  onPause: (item: TransferItemView) => void
  onResume: (item: TransferItemView) => void
  onCancel: (item: TransferItemView) => void
  onRetry: (item: TransferItemView) => void
  onResumeFile: (item: TransferItemView, file: File) => void
  onDismiss: (item: TransferItemView) => void
  onDismissCompleted: () => void
  onCancelFolder?: (job: FolderJobView) => void
  onRetryFolder?: (job: FolderJobView) => void
  onDismissFolder?: (job: FolderJobView) => void
}

function isFolderActive(status: FolderJobStatus): boolean {
  return FOLDER_ACTIVE.includes(status)
}

function folderPercent(job: FolderJobView): number {
  if (job.totalFiles > 0) {
    return Math.min(100, Math.round((job.doneFiles / job.totalFiles) * 100))
  }
  if (job.totalBytes > 0) {
    return Math.min(
      100,
      Math.round((job.completedBytes / job.totalBytes) * 100),
    )
  }
  return job.status === 'done' ? 100 : 0
}

export function TransferPanel({
  items,
  folderJobs = [],
  onPause,
  onResume,
  onCancel,
  onRetry,
  onResumeFile,
  onDismiss,
  onDismissCompleted,
  onCancelFolder = () => undefined,
  onRetryFolder = () => undefined,
  onDismissFolder = () => undefined,
}: TransferPanelProps) {
  const [collapsed, setCollapsed] = useState(false)
  const [closed, setClosed] = useState(false)
  const [closing, setClosing] = useState(false)
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set())

  const closePanel = () => {
    setClosing(true)
    const delay = prefersReducedMotion() ? 0 : 180
    window.setTimeout(() => {
      setClosed(true)
      setClosing(false)
    }, delay)
  }

  if (items.length === 0 && folderJobs.length === 0) {
    return null
  }

  const activeCount =
    items.filter((item) => !isTerminal(item.status)).length +
    folderJobs.filter((job) => isFolderActive(job.status)).length
  const allTerminal =
    items.every((item) => isTerminal(item.status)) &&
    folderJobs.every((job) => !isFolderActive(job.status))
  const allCompleted =
    items.every((item) => item.status === 'completed') &&
    folderJobs.every((job) => job.status === 'done')
  const totalBytes =
    items.reduce((sum, item) => sum + item.totalBytes, 0) +
    folderJobs.reduce((sum, job) => sum + job.totalBytes, 0)
  const completedBytes =
    items.reduce(
      (sum, item) => sum + Math.min(item.completedBytes, item.totalBytes),
      0,
    ) +
    folderJobs.reduce(
      (sum, job) => sum + Math.min(job.completedBytes, job.totalBytes),
      0,
    )
  const progress =
    totalBytes > 0
      ? Math.min(100, Math.round((completedBytes / totalBytes) * 100))
      : allCompleted
        ? 100
        : Math.round(
            [...items.map(itemPercent), ...folderJobs.map(folderPercent)].reduce(
              (sum, value) => sum + value,
              0,
            ) / Math.max(1, items.length + folderJobs.length),
          )
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
            onClick={() => {
              onDismissCompleted()
              for (const job of folderJobs) {
                if (!isFolderActive(job.status)) {
                  onDismissFolder(job)
                }
              }
            }}
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
          {folderJobs.map((job) => {
            const percent = folderPercent(job)
            const isExpanded = expanded.has(job.id)
            const visibleChildren = job.children.filter(
              (child) =>
                child.status === 'failed' || child.status === 'uploading',
            )
            const summary = `新增 ${job.counts.added} · 更新 ${job.counts.updated} · 未变化 ${job.counts.unchanged} · 失败 ${job.counts.failed}`
            return (
              <li
                key={job.id}
                data-testid="folder-job"
                data-status={job.status}
                data-direction={job.direction}
                data-done={job.doneFiles}
                data-total={job.totalFiles}
                data-name={job.name}
                className="space-y-2 border-b border-line px-4 py-3 last:border-b-0"
              >
                <div className="flex items-start gap-2.5">
                  <span className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-control bg-brand-50 text-brand-600">
                    <Folder size={17} aria-hidden="true" />
                  </span>
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-medium text-ink-900">
                      {job.name}
                    </p>
                    <p className="mt-0.5 text-xs leading-5 text-ink-600">
                      {FOLDER_STATUS_LABELS[job.status]}
                      {` · 已完成 ${job.doneFiles} / ${job.totalFiles} 个文件`}
                      {job.totalBytes > 0
                        ? ` · ${formatBytes(job.completedBytes)} / ${formatBytes(job.totalBytes)}`
                        : ''}
                    </p>
                  </div>
                </div>

                <ProgressBar
                  value={percent}
                  tone={
                    job.status === 'failed'
                      ? 'danger'
                      : job.status === 'done'
                        ? 'success'
                        : 'brand'
                  }
                  label={`${job.name} 文件夹进度`}
                />

                {job.direction === 'upload' && (
                  <p className="text-xs leading-5 text-ink-600">{summary}</p>
                )}
                {job.retrying && (
                  <p className="text-xs leading-5 text-brand-600" role="status">
                    服务器繁忙，稍后自动继续
                  </p>
                )}

                <div className="flex flex-wrap items-center justify-end gap-1">
                  {isFolderActive(job.status) && (
                    <Button
                      size="sm"
                      variant="dangerGhost"
                      data-testid="folder-job-cancel"
                      onClick={() => onCancelFolder(job)}
                    >
                      取消
                    </Button>
                  )}
                  {job.status === 'failed' && job.retryable && (
                    <Button
                      size="sm"
                      variant="ghost"
                      data-testid="folder-job-retry"
                      onClick={() => onRetryFolder(job)}
                    >
                      重试失败项
                    </Button>
                  )}
                  {!isFolderActive(job.status) && (
                    <Button
                      size="sm"
                      variant="ghost"
                      data-testid="folder-job-dismiss"
                      onClick={() => onDismissFolder(job)}
                    >
                      清除
                    </Button>
                  )}
                  {(visibleChildren.length > 0 || job.hiddenChildren > 0) && (
                    <Button
                      size="sm"
                      variant="ghost"
                      data-testid="folder-job-expand"
                      aria-expanded={isExpanded}
                      onClick={() =>
                        setExpanded((previous) => {
                          const next = new Set(previous)
                          if (next.has(job.id)) {
                            next.delete(job.id)
                          } else {
                            next.add(job.id)
                          }
                          return next
                        })
                      }
                    >
                      {isExpanded ? '收起文件' : '查看文件'}
                    </Button>
                  )}
                </div>

                {isExpanded && (
                  <ul className="max-h-56 space-y-1 overflow-y-auto rounded-control bg-canvas p-2">
                    {visibleChildren.map((child) => (
                      <li
                        key={child.id}
                        data-testid="folder-job-child"
                        data-status={child.status}
                        className="flex items-start justify-between gap-2 text-xs leading-5"
                      >
                        <span className="min-w-0 truncate text-ink-900">
                          {child.relativePath}
                        </span>
                        <span
                          className={cx(
                            'shrink-0',
                            child.status === 'failed'
                              ? 'text-danger-600'
                              : 'text-ink-600',
                          )}
                        >
                          {child.status === 'failed'
                            ? (child.error ?? '失败')
                            : '上传中'}
                        </span>
                      </li>
                    ))}
                    {job.hiddenChildren > 0 && (
                      <li className="text-xs text-ink-600">
                        还有 {job.hiddenChildren} 个文件未显示
                      </li>
                    )}
                  </ul>
                )}

                {job.errorMessage && (
                  <p role="alert" className="text-xs leading-5 text-danger-600">
                    {job.errorMessage}
                  </p>
                )}
              </li>
            )
          })}
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
