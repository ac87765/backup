import type { MouseEvent as ReactMouseEvent } from 'react'

import { cx, FileIcon } from '../ui'
import type { FileNodeView } from '../worker/types'
import { formatBytes } from './format'
import type { FileActions } from './file-actions'
import { FileRowActions } from './FileRowActions'

export interface FileGridProps {
  items: FileNodeView[]
  selectedId: string | null
  onSelect: (id: string) => void
  actions: FileActions
}

export function FileGrid({
  items,
  selectedId,
  onSelect,
  actions,
}: FileGridProps) {
  return (
    <div
      role="list"
      data-testid="file-grid"
      className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5"
    >
      {items.map((item) => {
        const selected = selectedId === item.id
        return (
          <div
            key={item.id}
            role="listitem"
            data-testid="file-row"
            data-name={item.name}
            data-kind={item.kind}
            data-selected={selected ? 'true' : 'false'}
            onClick={() => onSelect(item.id)}
            onDoubleClick={(event: ReactMouseEvent<HTMLDivElement>) => {
              if ((event.target as HTMLElement).closest('button')) {
                return
              }
              actions.onOpen(item)
            }}
            className={cx(
              'group relative flex flex-col items-center gap-3 rounded-card border bg-white px-4 pb-4 pt-6 text-center transition-colors',
              selected
                ? 'border-brand-600 bg-brand-50'
                : 'border-line hover:border-line-strong hover:bg-canvas',
            )}
          >
            <FileRowActions
              item={item}
              busy={actions.busy}
              onDownload={actions.onDownload}
              onRename={actions.onRename}
              onMove={actions.onMove}
              onVersions={actions.onVersions}
              onNewVersion={actions.onNewVersion}
              onDelete={actions.onDelete}
              className={cx(
                'absolute right-2 top-2 rounded-control bg-white/90 transition-opacity',
                selected
                  ? 'opacity-100'
                  : 'opacity-0 group-hover:opacity-100 group-focus-within:opacity-100',
              )}
            />
            <button
              type="button"
              data-row-name
              aria-label={`打开 ${item.name}`}
              onClick={(event) => {
                event.stopPropagation()
                onSelect(item.id)
                actions.onOpen(item)
              }}
              className="flex w-full flex-col items-center gap-3 rounded-control focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600"
            >
              <span className="flex h-16 w-16 items-center justify-center rounded-card bg-canvas">
                <FileIcon name={item.name} kind={item.kind} size="xl" />
              </span>
              <span className="line-clamp-2 w-full break-all text-sm font-medium leading-5 text-ink-900">
                {item.name}
              </span>
            </button>
            <p className="text-xs leading-4 text-ink-600">
              {item.kind === 'folder'
                ? '文件夹'
                : formatBytes(item.size)}
            </p>
          </div>
        )
      })}
    </div>
  )
}
