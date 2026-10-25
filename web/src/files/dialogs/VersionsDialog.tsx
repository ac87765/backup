import { Download, RotateCcw } from 'lucide-react'

import { Button, Dialog, FormError } from '../../ui'
import { formatBytes, formatTimestamp } from '../format'
import type { FileNodeView, VersionView } from '../../worker/types'

export interface VersionsDialogProps {
  node: FileNodeView | null
  versions: VersionView[]
  error: string | null
  busy: boolean
  onClose: () => void
  onDownload: (version: VersionView) => void
  onRestore: (version: VersionView) => void
}

export function VersionsDialog({
  node,
  versions,
  error,
  busy,
  onClose,
  onDownload,
  onRestore,
}: VersionsDialogProps) {
  if (!node) {
    return null
  }
  return (
    <Dialog
      open
      onClose={onClose}
      title={`版本历史 · ${node.name}`}
      size="lg"
    >
      <div className="space-y-3">
        <FormError message={error} />
        {versions.length === 0 ? (
          <p className="py-4 text-sm text-ink-600">没有可用版本。</p>
        ) : (
          <ul className="divide-y divide-line rounded-card border border-line">
            {versions.map((version, index) => (
              <li
                key={version.id}
                className="flex flex-wrap items-center justify-between gap-3 px-4 py-3 text-sm"
              >
                <div className="min-w-0 space-y-1">
                  <p className="flex flex-wrap items-center gap-2 font-medium text-ink-900">
                    {formatTimestamp(version.createdAt)}
                    {version.isCurrent && (
                      <span className="rounded-full bg-success-50 px-2 py-0.5 text-xs font-medium text-success-600">
                        当前版本
                      </span>
                    )}
                  </p>
                  <p className="text-xs leading-5 text-ink-600">
                    {formatBytes(version.size)} · {version.mime || '未知类型'}
                  </p>
                </div>
                <div className="flex gap-2">
                  <Button
                    size="sm"
                    icon={<Download size={14} />}
                    data-testid={`download-version-${index}`}
                    aria-label={`下载版本 ${version.id}`}
                    disabled={busy}
                    onClick={() => onDownload(version)}
                  >
                    下载
                  </Button>
                  {!version.isCurrent && (
                    <Button
                      size="sm"
                      icon={<RotateCcw size={14} />}
                      data-testid={`restore-version-${index}`}
                      aria-label={`恢复版本 ${version.id}`}
                      disabled={busy}
                      onClick={() => onRestore(version)}
                    >
                      恢复此版本
                    </Button>
                  )}
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>
    </Dialog>
  )
}
