import { Dialog } from '../../ui'
import type { FileNodeView, WorkerApi } from '../../worker/types'
import { FolderPicker } from '../FolderPicker'

export interface MoveDialogProps {
  node: FileNodeView | null
  api: WorkerApi | null
  rootId: string
  onClose: () => void
  onSelect: (folder: { id: string; name: string }) => void
}

export function MoveDialog({
  node,
  api,
  rootId,
  onClose,
  onSelect,
}: MoveDialogProps) {
  if (!node) {
    return null
  }
  return (
    <Dialog
      open
      onClose={onClose}
      title={`移动“${node.name}”`}
      size="md"
    >
      <FolderPicker
        key={`move-${node.id}`}
        api={api}
        startId={rootId}
        startName="全部文件"
        excludeId={node.id}
        onCancel={onClose}
        onSelect={onSelect}
      />
    </Dialog>
  )
}
