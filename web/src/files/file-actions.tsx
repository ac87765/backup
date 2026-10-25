import {
  FolderDown,
  FolderInput,
  History,
  Pencil,
  Trash2,
  UploadCloud,
} from 'lucide-react'

import type { MenuItem } from '../ui'
import type { FileNodeView } from '../worker/types'

export interface FileActionHandlers {
  busy: boolean
  onRename: (item: FileNodeView) => void
  onMove: (item: FileNodeView) => void
  onVersions: (item: FileNodeView) => void
  onNewVersion: (item: FileNodeView) => void
  onDelete: (item: FileNodeView) => void
  /** 文件夹整体下载为 .zip（design D7）。 */
  onDownloadFolder?: (item: FileNodeView) => void
}

export interface FileActions extends FileActionHandlers {
  onOpen: (item: FileNodeView) => void
  onDownload: (item: FileNodeView) => void
}

/** 行内“更多”菜单与右键菜单共用的菜单项；可访问名称保持 `动作 {文件名}`。 */
export function fileMenuItems(
  item: FileNodeView,
  handlers: FileActionHandlers,
): MenuItem[] {
  const items: MenuItem[] = [
    {
      label: '重命名',
      ariaLabel: `重命名 ${item.name}`,
      icon: <Pencil size={16} />,
      disabled: handlers.busy,
      onSelect: () => handlers.onRename(item),
    },
    {
      label: '移动到…',
      ariaLabel: `移动 ${item.name}`,
      icon: <FolderInput size={16} />,
      disabled: handlers.busy,
      onSelect: () => handlers.onMove(item),
    },
  ]
  if (item.kind === 'file') {
    items.push(
      {
        label: '版本历史',
        ariaLabel: `版本历史 ${item.name}`,
        icon: <History size={16} />,
        disabled: handlers.busy,
        onSelect: () => handlers.onVersions(item),
      },
      {
        label: '上传新版本',
        ariaLabel: `上传新版本 ${item.name}`,
        icon: <UploadCloud size={16} />,
        disabled: handlers.busy,
        onSelect: () => handlers.onNewVersion(item),
      },
    )
  } else if (handlers.onDownloadFolder) {
    items.push({
      label: '下载为 .zip',
      ariaLabel: `下载为 .zip ${item.name}`,
      icon: <FolderDown size={16} />,
      disabled: handlers.busy,
      onSelect: () => handlers.onDownloadFolder?.(item),
    })
  }
  items.push({
    label: '删除',
    ariaLabel: `删除 ${item.name}`,
    icon: <Trash2 size={16} />,
    tone: 'danger',
    separatorBefore: true,
    disabled: handlers.busy,
    onSelect: () => handlers.onDelete(item),
  })
  return items
}
