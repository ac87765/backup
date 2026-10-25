import { LayoutGrid, List, RotateCw, Search } from 'lucide-react'

import { cx, IconButton, TextField } from '../ui'
import type { ViewMode } from './view-preferences'

export interface FileToolbarProps {
  filter: string
  onFilterChange: (value: string) => void
  viewMode: ViewMode
  onViewModeChange: (mode: ViewMode) => void
  onRefresh: () => void
}

export function FileToolbar({
  filter,
  onFilterChange,
  viewMode,
  onViewModeChange,
  onRefresh,
}: FileToolbarProps) {
  return (
    <div className="flex items-center gap-1.5 sm:gap-2">
      <div className="w-36 shrink-0 sm:w-56">
        <TextField
          aria-label="筛选当前文件夹"
          data-testid="file-filter"
          placeholder="筛选当前文件夹"
          value={filter}
          onChange={(event) => onFilterChange(event.target.value)}
          leading={<Search size={16} />}
        />
      </div>
      <div
        role="group"
        aria-label="视图切换"
        className="flex items-center rounded-control border border-line bg-white p-0.5"
      >
        <button
          type="button"
          aria-label="列表视图"
          aria-pressed={viewMode === 'list'}
          data-testid="view-list"
          onClick={() => onViewModeChange('list')}
          className={cx(
            'flex h-8 w-8 items-center justify-center rounded-control transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600',
            viewMode === 'list'
              ? 'bg-brand-50 text-brand-600'
              : 'text-ink-600 hover:bg-canvas hover:text-ink-900',
          )}
        >
          <List size={16} aria-hidden="true" />
        </button>
        <button
          type="button"
          aria-label="网格视图"
          aria-pressed={viewMode === 'grid'}
          data-testid="view-grid"
          onClick={() => onViewModeChange('grid')}
          className={cx(
            'flex h-8 w-8 items-center justify-center rounded-control transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600',
            viewMode === 'grid'
              ? 'bg-brand-50 text-brand-600'
              : 'text-ink-600 hover:bg-canvas hover:text-ink-900',
          )}
        >
          <LayoutGrid size={16} aria-hidden="true" />
        </button>
      </div>
      <div className="hidden sm:block">
        <IconButton
          label="刷新"
          data-testid="refresh-files"
          onClick={onRefresh}
        >
          <RotateCw size={16} />
        </IconButton>
      </div>
    </div>
  )
}
