import { useCallback, useEffect, useState } from 'react'
import { RotateCcw, Trash2 } from 'lucide-react'

import { useAuth } from '../auth/context'
import { errorCode, errorMessage } from '../auth/errors'
import { AppShell } from '../shell/AppShell'
import { PageHeader } from '../shell/PageHeader'
import { useUsage } from '../shell/usage-context'
import { Button, Dialog, EmptyState, FileIcon, FormError, useToast } from '../ui'
import type { TrashNodeView } from '../worker/types'
import { FileSkeleton } from './FileSkeleton'
import { FolderPicker } from './FolderPicker'
import { formatBytes, formatTimestamp, sortByName } from './format'

export function TrashView() {
  const { api, user } = useAuth()
  const rootId = user?.rootNodeId ?? ''
  const { refreshUsage } = useUsage()
  const { showToast } = useToast()

  const [trash, setTrash] = useState<TrashNodeView[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [confirmItem, setConfirmItem] = useState<TrashNodeView | null>(null)
  const [restorePicker, setRestorePicker] = useState<TrashNodeView | null>(null)

  const loadTrash = useCallback(async () => {
    if (!api) {
      return
    }
    try {
      setTrash(sortByName(await api.listTrash()))
      setError(null)
    } catch (caught) {
      setError(errorMessage(caught))
    } finally {
      setLoading(false)
    }
  }, [api])

  useEffect(() => {
    if (!api) {
      return
    }
    let cancelled = false
    void (async () => {
      try {
        const rows = await api.listTrash()
        if (!cancelled) {
          setTrash(sortByName(rows))
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
  }, [api])

  const restoreItem = async (item: TrashNodeView) => {
    if (!api) {
      return
    }
    setBusy(`restore:${item.id}`)
    try {
      await api.restoreNode(item.id)
      showToast(`“${item.name}”已恢复`)
      await loadTrash()
      refreshUsage()
    } catch (caught) {
      if (errorCode(caught) === 'target_unavailable') {
        setRestorePicker(item)
      } else {
        showToast(errorMessage(caught), 'error')
      }
    } finally {
      setBusy(null)
    }
  }

  const submitRestoreTo = async (target: { id: string; name: string }) => {
    if (!api || !restorePicker) {
      return
    }
    setBusy('restore')
    try {
      await api.restoreNode(restorePicker.id, target.id, restorePicker.name)
      showToast(`“${restorePicker.name}”已恢复到“${target.name}”`)
      setRestorePicker(null)
      await loadTrash()
      refreshUsage()
    } catch (caught) {
      showToast(errorMessage(caught), 'error')
      setRestorePicker(null)
    } finally {
      setBusy(null)
    }
  }

  const permanentlyDelete = async (item: TrashNodeView) => {
    if (!api) {
      return
    }
    setBusy(`permanent:${item.id}`)
    try {
      const result = await api.permanentlyDeleteNode(item.id)
      setConfirmItem(null)
      showToast(`已永久删除，释放 ${formatBytes(result.freedBytes)} 配额`)
      await loadTrash()
      refreshUsage()
    } catch (caught) {
      showToast(errorMessage(caught), 'error')
    } finally {
      setBusy(null)
    }
  }

  return (
    <AppShell title="回收站">
      <div data-testid="trash-view" className="space-y-4">
        <PageHeader
          title="回收站"
          subtitle="已删除的项目会保留在这里，可随时恢复或永久删除。"
        />

        <FormError message={error} />

        {loading ? (
          <FileSkeleton rows={4} />
        ) : trash.length === 0 ? (
          <div className="rounded-card border border-line bg-white">
            <EmptyState
              icon={<Trash2 size={24} />}
              title="回收站是空的"
              description="删除的文件会先放在这里，可随时恢复。"
            />
          </div>
        ) : (
          <div
            role="table"
            aria-label="回收站"
            className="overflow-hidden rounded-card border border-line bg-white"
          >
            <div
              role="row"
              className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 border-b border-line bg-canvas px-4 py-2 text-xs font-medium text-ink-600 md:grid-cols-[minmax(0,1fr)_180px_100px_auto]"
            >
              <div role="columnheader">名称</div>
              <div role="columnheader" className="hidden md:block">
                删除时间
              </div>
              <div role="columnheader" className="hidden md:block">
                大小
              </div>
              <div role="columnheader" className="sr-only">
                操作
              </div>
            </div>
            <ul role="rowgroup">
              {trash.map((item) => (
                <li
                  key={item.id}
                  role="row"
                  data-testid="trash-item"
                  data-name={item.name}
                  className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 border-b border-line px-4 py-3 last:border-b-0 md:grid-cols-[minmax(0,1fr)_180px_100px_auto]"
                >
                  <div
                    role="cell"
                    className="flex min-w-0 items-center gap-3"
                  >
                    <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-control bg-canvas">
                      <FileIcon name={item.name} kind={item.kind} size="md" />
                    </span>
                    <div className="min-w-0">
                      <p className="truncate text-sm font-medium text-ink-900">
                        {item.name}
                      </p>
                      <p className="mt-0.5 text-xs leading-4 text-ink-600 md:hidden">
                        {formatTimestamp(item.deletedAt)} ·{' '}
                        {formatBytes(item.size)}
                      </p>
                    </div>
                  </div>
                  <div
                    role="cell"
                    className="hidden text-sm text-ink-600 md:block"
                  >
                    {formatTimestamp(item.deletedAt)}
                  </div>
                  <div
                    role="cell"
                    className="hidden text-sm text-ink-600 md:block"
                  >
                    {formatBytes(item.size)}
                  </div>
                  <div
                    role="cell"
                    className="flex justify-end gap-1.5"
                  >
                    <Button
                      size="sm"
                      icon={<RotateCcw size={14} />}
                      aria-label={`恢复 ${item.name}`}
                      disabled={busy !== null}
                      onClick={() => void restoreItem(item)}
                    >
                      恢复
                    </Button>
                    <Button
                      size="sm"
                      variant="dangerGhost"
                      icon={<Trash2 size={14} />}
                      aria-label={`永久删除 ${item.name}`}
                      disabled={busy !== null}
                      onClick={() => setConfirmItem(item)}
                    >
                      永久删除
                    </Button>
                  </div>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>

      {confirmItem && (
        <Dialog
          open
          onClose={() => setConfirmItem(null)}
          title="永久删除"
          size="sm"
          description={`“${confirmItem.name}”及其所有版本将被永久删除，且无法恢复。`}
          footer={
            <>
              <Button onClick={() => setConfirmItem(null)}>取消</Button>
              <Button
                variant="danger"
                data-testid="confirm-permanent-delete"
                aria-label={`确认永久删除 ${confirmItem.name}`}
                disabled={busy !== null}
                onClick={() => void permanentlyDelete(confirmItem)}
              >
                永久删除
              </Button>
            </>
          }
        >
          <p className="text-sm leading-6 text-ink-600">
            删除后不会进入回收站，也不能通过恢复密钥找回。
          </p>
        </Dialog>
      )}

      {restorePicker && (
        <Dialog
          open
          onClose={() => setRestorePicker(null)}
          title={`恢复“${restorePicker.name}”到新位置`}
          size="md"
        >
          <FolderPicker
            key={`restore-${restorePicker.id}`}
            api={api}
            startId={rootId}
            startName="全部文件"
            onCancel={() => setRestorePicker(null)}
            onSelect={(folder) => void submitRestoreTo(folder)}
          />
        </Dialog>
      )}
    </AppShell>
  )
}
