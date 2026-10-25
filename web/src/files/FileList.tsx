import { Fragment, useState, type MouseEvent as ReactMouseEvent } from 'react'
import { ArrowDown, ArrowUp, ArrowUpDown } from 'lucide-react'

import { ContextMenu, cx } from '../ui'
import { FileIcon } from '../ui/FileIcon'
import type { FileNodeView } from '../worker/types'
import { formatBytes, formatUnixTime } from './format'
import { fileMenuItems, type FileActions } from './file-actions'
import { FileRowActions } from './FileRowActions'
import type { SortField, SortState } from './view-preferences'
import { VirtualList } from './VirtualList'

interface ContextMenuState {
  item: FileNodeView
  point: { x: number; y: number }
  restoreFocusTo: HTMLElement | null
}

export interface FileListProps {
  items: FileNodeView[]
  sort: SortState
  onSort: (field: SortField) => void
  selectedId: string | null
  onSelect: (id: string) => void
  actions: FileActions
  onEndReached: () => void
  /** 已从服务端删除、正在播放淡出收起动画的行。 */
  removingIds?: ReadonlySet<string>
  /** 是否还有下一页；有下一页时使用固定视口虚拟滚动。 */
  hasMore?: boolean
}

const ROW_HEIGHT = 60
const FLOW_LIMIT = 10

function SortIndicator({
  active,
  direction,
}: {
  active: boolean
  direction: SortState['direction']
}) {
  if (!active) {
    return (
      <ArrowUpDown size={12} className="text-ink-400" aria-hidden="true" />
    )
  }
  return direction === 'asc' ? (
    <ArrowUp size={12} aria-hidden="true" />
  ) : (
    <ArrowDown size={12} aria-hidden="true" />
  )
}

export function FileList({
  items,
  sort,
  onSort,
  selectedId,
  onSelect,
  actions,
  onEndReached,
  removingIds,
  hasMore = false,
}: FileListProps) {
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null)
  const flow = items.length <= FLOW_LIMIT && !hasMore

  const headerButton = (field: SortField, label: string) => (
    <button
      type="button"
      onClick={() => onSort(field)}
      className="motion-fast inline-flex items-center gap-1 rounded-control px-1 py-0.5 text-xs font-medium text-ink-600 transition-colors hover:text-ink-900 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600"
    >
      {label}
      <SortIndicator active={sort.field === field} direction={sort.direction} />
    </button>
  )

  const ariaSort = (field: SortField): 'ascending' | 'descending' | 'none' => {
    if (sort.field !== field) {
      return 'none'
    }
    return sort.direction === 'asc' ? 'ascending' : 'descending'
  }

  const renderRow = (item: FileNodeView) => {
    const selected = selectedId === item.id
    const removing = removingIds?.has(item.id) ?? false
    return (
      <div
        role="row"
        data-testid="file-row"
        data-name={item.name}
        data-kind={item.kind}
        data-selected={selected ? 'true' : 'false'}
        data-removing={removing ? 'true' : 'false'}
        aria-selected={selected}
        onClick={() => onSelect(item.id)}
        onContextMenu={(event: ReactMouseEvent<HTMLDivElement>) => {
          event.preventDefault()
          setContextMenu({
            item,
            point: { x: event.clientX, y: event.clientY },
            restoreFocusTo: event.currentTarget,
          })
        }}
        onDoubleClick={(event) => {
          if ((event.target as HTMLElement).closest('button')) {
            return
          }
          actions.onOpen(item)
        }}
        className={cx(
          'animate-row-in group relative grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 px-3 transition-[background-color,opacity,max-height] motion-base md:grid-cols-[minmax(0,1fr)_160px_100px_96px]',
          flow
            ? 'max-h-[60px] min-h-[60px] overflow-hidden border-b border-line'
            : 'h-full',
          selected ? 'bg-brand-50' : 'hover:bg-canvas',
          removing &&
            (flow
              ? 'pointer-events-none max-h-0 min-h-0 border-transparent opacity-0'
              : 'pointer-events-none opacity-0'),
        )}
      >
        <div role="cell" className="flex min-w-0 items-center gap-3">
          <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-control bg-canvas">
            <FileIcon name={item.name} kind={item.kind} size="md" />
          </span>
          <div className="min-w-0">
            <button
              type="button"
              data-row-name
              aria-label={`打开 ${item.name}`}
              onClick={(event) => {
                event.stopPropagation()
                onSelect(item.id)
                actions.onOpen(item)
              }}
              className="motion-fast block max-w-full truncate rounded-control text-left text-sm font-medium text-ink-900 transition-colors hover:text-brand-600 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600"
            >
              {item.name}
            </button>
            <p className="mt-0.5 truncate text-xs leading-4 text-ink-600 md:hidden">
              {item.kind === 'folder'
                ? '文件夹'
                : `${formatBytes(item.size)} · ${formatUnixTime(item.mtime)}`}
            </p>
          </div>
        </div>
        <div
          role="cell"
          className="hidden truncate text-sm text-ink-600 md:block"
        >
          {item.kind === 'folder' ? '—' : formatUnixTime(item.mtime)}
        </div>
        <div role="cell" className="hidden text-sm text-ink-600 md:block">
          {item.kind === 'folder' ? '—' : formatBytes(item.size)}
        </div>
        <div role="cell" className="flex justify-end">
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
              'transition-opacity',
              selected
                ? 'opacity-100'
                : 'opacity-0 group-hover:opacity-100 group-focus-within:opacity-100',
            )}
          />
        </div>
      </div>
    )
  }

  return (
    <div role="table" aria-label="文件列表" className="w-full">
      <div
        role="row"
        className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 border-y border-line bg-canvas px-3 py-2 md:grid-cols-[minmax(0,1fr)_160px_100px_96px]"
      >
        <div role="columnheader" aria-sort={ariaSort('name')}>
          {headerButton('name', '名称')}
        </div>
        <div
          role="columnheader"
          aria-sort={ariaSort('mtime')}
          className="hidden md:block"
        >
          {headerButton('mtime', '修改时间')}
        </div>
        <div
          role="columnheader"
          aria-sort={ariaSort('size')}
          className="hidden md:block"
        >
          {headerButton('size', '大小')}
        </div>
        <div role="columnheader" className="sr-only">
          操作
        </div>
      </div>

      {flow ? (
        <div data-testid="file-list" role="rowgroup">
          {items.map((item) => (
            <Fragment key={item.id}>{renderRow(item)}</Fragment>
          ))}
        </div>
      ) : (
        <div role="rowgroup">
          <VirtualList
            items={items}
            getKey={(item) => item.id}
            testId="file-list"
            height={520}
            rowHeight={ROW_HEIGHT}
            rowClassName="group border-b border-line"
            onEndReached={onEndReached}
            renderItem={renderRow}
          />
        </div>
      )}

      {contextMenu && (
        <ContextMenu
          label={`更多操作 ${contextMenu.item.name}`}
          items={fileMenuItems(contextMenu.item, actions)}
          point={contextMenu.point}
          restoreFocusTo={contextMenu.restoreFocusTo}
          onClose={() => setContextMenu(null)}
        />
      )}
    </div>
  )
}
