import { formatBytes } from './format'
import type {
  TransferDirection,
  TransferStatus,
} from '../worker/types'

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

interface TransferQueueProps {
  items: TransferItemView[]
  onPause: (item: TransferItemView) => void
  onResume: (item: TransferItemView) => void
  onCancel: (item: TransferItemView) => void
  onRetry: (item: TransferItemView) => void
  onResumeFile: (item: TransferItemView, file: File) => void
  onDismiss: (item: TransferItemView) => void
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

function percent(item: TransferItemView): number {
  if (item.totalChunks <= 0) {
    return 0
  }
  return Math.min(100, Math.round((item.completedChunks / item.totalChunks) * 100))
}

export function TransferQueue({
  items,
  onPause,
  onResume,
  onCancel,
  onRetry,
  onResumeFile,
  onDismiss,
}: TransferQueueProps) {
  if (items.length === 0) {
    return null
  }
  return (
    <section className="space-y-3 rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
      <h2 className="text-lg font-semibold text-slate-900">传输队列</h2>
      <ul className="space-y-2">
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
          return (
            <li
              key={item.localId}
              data-testid="transfer-item"
              data-direction={item.direction}
              data-status={item.status}
              data-name={item.fileName}
              className="space-y-2 rounded-lg border border-slate-200 p-3"
            >
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="min-w-0">
                  <p className="truncate font-medium text-slate-900">
                    {item.direction === 'upload' ? '↑ ' : '↓ '}
                    {item.fileName}
                  </p>
                  <p className="text-xs text-slate-500">
                    {STATUS_LABELS[item.status]}
                    {item.totalChunks > 0
                      ? ` · ${item.completedChunks}/${item.totalChunks} 块`
                      : ''}
                    {item.totalBytes > 0
                      ? ` · ${formatBytes(item.completedBytes)} / ${formatBytes(item.totalBytes)}`
                      : ''}
                  </p>
                </div>
                <div className="flex flex-wrap gap-1 text-xs">
                  {canPause && (
                    <button
                      type="button"
                      data-testid="transfer-pause"
                      className="rounded border border-slate-300 px-2 py-1"
                      onClick={() => onPause(item)}
                    >
                      暂停
                    </button>
                  )}
                  {paused && !item.pendingResume && (
                    <button
                      type="button"
                      data-testid="transfer-resume"
                      className="rounded border border-slate-300 px-2 py-1"
                      onClick={() => onResume(item)}
                    >
                      继续
                    </button>
                  )}
                  {item.status === 'failed' && !item.pendingResume && (
                    <button
                      type="button"
                      data-testid="transfer-retry"
                      className="rounded border border-slate-300 px-2 py-1"
                      onClick={() => onRetry(item)}
                    >
                      重试
                    </button>
                  )}
                  {(active || paused) && canCancel && (
                    <button
                      type="button"
                      data-testid="transfer-cancel"
                      className="rounded border border-red-200 px-2 py-1 text-red-700"
                      onClick={() => onCancel(item)}
                    >
                      取消
                    </button>
                  )}
                  {(item.status === 'completed' ||
                    item.status === 'cancelled' ||
                    item.status === 'failed') && (
                    <button
                      type="button"
                      data-testid="transfer-dismiss"
                      className="rounded border border-slate-300 px-2 py-1"
                      onClick={() => onDismiss(item)}
                    >
                      清除
                    </button>
                  )}
                </div>
              </div>
              <progress
                data-testid="transfer-progress"
                className="h-2 w-full overflow-hidden rounded bg-slate-100"
                max={100}
                value={percent(item)}
              />
              {item.pendingResume && (
                <label className="block text-xs text-slate-600">
                  重新选择同一文件以继续
                  <input
                    type="file"
                    data-testid={`resume-upload-input-${item.sessionId ?? ''}`}
                    className="mt-1 block w-full text-xs"
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
                <p role="alert" className="text-xs text-red-700">
                  {item.errorMessage}
                </p>
              )}
            </li>
          )
        })}
      </ul>
    </section>
  )
}
