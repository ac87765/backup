import { useEffect, useState } from 'react'
import { Folder } from 'lucide-react'

import { errorMessage } from '../auth/errors'
import { Button, cx, FormError } from '../ui'
import type { FileNodeView, WorkerApi } from '../worker/types'
import { sortByName } from './format'

export interface FolderPickerProps {
  api: WorkerApi | null
  startId: string
  startName: string
  /** 禁止选择该节点（通常是正在移动的文件夹自身）。 */
  excludeId?: string
  onCancel: () => void
  onSelect: (folder: { id: string; name: string }) => void
}

/** 在文件夹树中选择目标位置；只显示文件夹，不显示文件。 */
export function FolderPicker({
  api,
  startId,
  startName,
  excludeId,
  onCancel,
  onSelect,
}: FolderPickerProps) {
  const [path, setPath] = useState<{ id: string; name: string }[]>([
    { id: startId, name: startName },
  ])
  const [items, setItems] = useState<FileNodeView[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const current = path[path.length - 1]

  useEffect(() => {
    if (!api || !current) {
      return
    }
    let cancelled = false
    void (async () => {
      try {
        const folders: FileNodeView[] = []
        let cursor: string | undefined
        do {
          const page = await api.listFolder(current.id, cursor)
          folders.push(...page.items.filter((item) => item.kind === 'folder'))
          cursor = page.nextCursor ?? undefined
        } while (cursor && !cancelled)
        if (!cancelled) {
          setItems(sortByName(folders))
          setError(null)
        }
      } catch (caught) {
        if (!cancelled) {
          setError(errorMessage(caught))
        }
      } finally {
        if (!cancelled) {
          setLoading(false)
        }
      }
    })()
    return () => {
      cancelled = true
    }
  }, [api, current])

  if (!current) {
    return null
  }

  return (
    <div data-testid="folder-picker" className="space-y-3">
      <nav
        data-testid="picker-breadcrumb"
        aria-label="目标文件夹路径"
        className="flex flex-wrap items-center gap-0.5 text-sm text-ink-600"
      >
        {path.map((entry, index) => (
          <span key={entry.id} className="flex items-center gap-0.5">
            {index > 0 && <span className="text-ink-400">/</span>}
            <button
              type="button"
              disabled={index === path.length - 1}
              onClick={() => {
                setLoading(true)
                setPath(path.slice(0, index + 1))
              }}
              className={cx(
                'rounded-control px-1.5 py-1 transition-colors',
                index === path.length - 1
                  ? 'font-medium text-ink-900'
                  : 'text-ink-600 hover:bg-canvas hover:text-ink-900',
                'disabled:cursor-default focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600',
              )}
            >
              {entry.name}
            </button>
          </span>
        ))}
      </nav>
      <FormError message={error} />
      <div className="max-h-64 space-y-1 overflow-y-auto rounded-card border border-line p-1.5">
        {loading && (
          <p aria-live="polite" className="px-2 py-3 text-sm text-ink-600">
            加载中…
          </p>
        )}
        {!loading && items.length === 0 && (
          <p className="px-2 py-3 text-sm text-ink-600">
            当前文件夹没有子文件夹。
          </p>
        )}
        {items.map((item) => (
          <button
            key={item.id}
            type="button"
            data-testid="picker-folder"
            data-name={item.name}
            onClick={() => {
              setLoading(true)
              setPath([...path, { id: item.id, name: item.name }])
            }}
            className="flex w-full items-center gap-2 rounded-control px-2.5 py-2 text-left text-sm text-ink-900 transition-colors hover:bg-canvas focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600"
          >
            <Folder
              size={18}
              className="shrink-0 text-file-folder"
              aria-hidden="true"
            />
            <span className="truncate">{item.name}</span>
          </button>
        ))}
      </div>
      <div className="flex flex-wrap items-center justify-between gap-2 border-t border-line pt-3">
        <p className="min-w-0 truncate text-xs leading-5 text-ink-600">
          目标：{path.map((entry) => entry.name).join(' / ')}
        </p>
        <div className="flex gap-2">
          <Button onClick={onCancel}>取消</Button>
          <Button
            variant="primary"
            data-testid="picker-select-current"
            disabled={current.id === excludeId}
            onClick={() => onSelect(current)}
          >
            选择此文件夹
          </Button>
        </div>
      </div>
    </div>
  )
}
