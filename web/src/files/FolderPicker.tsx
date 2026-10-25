import { useEffect, useState } from 'react'

import { errorMessage } from '../auth/errors'
import { FormError } from '../components/PageShell'
import type { FileNodeView, WorkerApi } from '../worker/types'
import { sortByName } from './format'

export interface FolderPickerProps {
  api: WorkerApi | null
  startId: string
  startName: string
  title: string
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
  title,
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
    <div
      role="dialog"
      aria-modal="true"
      aria-label={title}
      data-testid="folder-picker"
      className="fixed inset-0 z-30 flex items-center justify-center bg-slate-900/40 p-4"
    >
      <div className="w-full max-w-lg space-y-3 rounded-2xl bg-white p-4 shadow-xl">
        <h2 className="text-lg font-semibold text-slate-900">{title}</h2>
        <nav
          data-testid="picker-breadcrumb"
          className="flex flex-wrap items-center gap-1 text-sm text-slate-600"
        >
          {path.map((entry, index) => (
            <span key={entry.id} className="flex items-center gap-1">
              {index > 0 && <span>/</span>}
              <button
                type="button"
                className="underline disabled:no-underline"
                disabled={index === path.length - 1}
                onClick={() => {
                  setLoading(true)
                  setPath(path.slice(0, index + 1))
                }}
              >
                {entry.name}
              </button>
            </span>
          ))}
        </nav>
        <FormError message={error} />
        <div className="max-h-64 space-y-1 overflow-y-auto">
          {loading && <p className="text-sm text-slate-500">加载中…</p>}
          {!loading && items.length === 0 && (
            <p className="text-sm text-slate-500">当前文件夹没有子文件夹。</p>
          )}
          {items.map((item) => (
            <button
              key={item.id}
              type="button"
              data-testid="picker-folder"
              data-name={item.name}
              className="flex w-full items-center gap-2 rounded-lg border border-slate-200 px-3 py-2 text-left text-sm text-slate-800"
              onClick={() => {
                setLoading(true)
                setPath([...path, { id: item.id, name: item.name }])
              }}
            >
              <span aria-hidden="true">📁</span>
              <span className="truncate">{item.name}</span>
            </button>
          ))}
        </div>
        <div className="flex items-center justify-between gap-2">
          <p className="text-xs text-slate-500">
            目标：{path.map((entry) => entry.name).join(' / ')}
          </p>
          <div className="flex gap-2">
            <button
              type="button"
              className="rounded-lg border border-slate-300 px-3 py-2 text-sm"
              onClick={onCancel}
            >
              取消
            </button>
            <button
              type="button"
              data-testid="picker-select-current"
              disabled={current.id === excludeId}
              className="rounded-lg bg-slate-900 px-3 py-2 text-sm font-semibold text-white disabled:opacity-50"
              onClick={() => onSelect(current)}
            >
              选择此文件夹
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
