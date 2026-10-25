import { Download, MoreHorizontal } from 'lucide-react'

import { IconButton, Menu } from '../ui'
import type { FileNodeView } from '../worker/types'
import { fileMenuItems, type FileActionHandlers } from './file-actions'

export interface FileRowActionsProps extends FileActionHandlers {
  item: FileNodeView
  onDownload: (item: FileNodeView) => void
  className?: string
}

export function FileRowActions({
  item,
  busy,
  onDownload,
  className,
  onRename,
  onMove,
  onVersions,
  onNewVersion,
  onDelete,
  onDownloadFolder,
}: FileRowActionsProps) {
  const handlersForMenu: FileActionHandlers = {
    busy,
    onRename,
    onMove,
    onVersions,
    onNewVersion,
    onDelete,
    onDownloadFolder,
  }

  return (
    <div className={className} data-row-action>
      <div className="flex items-center justify-end gap-0.5">
        {item.kind === 'file' && (
          <div className="hidden sm:block">
            <IconButton
              size="sm"
              label={`下载 ${item.name}`}
              data-testid={`download-${item.id}`}
              disabled={busy || !item.currentVersionId}
              onClick={() => onDownload(item)}
            >
              <Download size={16} />
            </IconButton>
          </div>
        )}
        <Menu
          label={`更多操作 ${item.name}`}
          items={fileMenuItems(item, handlersForMenu)}
          trigger={
            <IconButton
              size="sm"
              label={`更多 ${item.name}`}
              data-testid={`more-${item.id}`}
            >
              <MoreHorizontal size={17} />
            </IconButton>
          }
        />
      </div>
    </div>
  )
}
