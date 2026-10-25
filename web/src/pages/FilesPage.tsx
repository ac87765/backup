import { useCallback, useEffect, useRef, useState } from 'react'
import { Link } from 'react-router-dom'

import { useAuth } from '../auth/context'
import { errorCode, errorMessage } from '../auth/errors'
import { FormError, FormSuccess } from '../components/PageShell'
import { FolderPicker } from '../files/FolderPicker'
import { VirtualList } from '../files/VirtualList'
import {
  formatBytes,
  formatTimestamp,
  formatUnixTime,
  sortByName,
} from '../files/format'
import { validateName } from '../files/name-validation'
import type {
  FileNodeView,
  TrashNodeView,
  UsageView,
  VersionView,
} from '../worker/types'

interface Breadcrumb {
  id: string
  name: string
}

type PickerState =
  | { mode: 'move'; node: FileNodeView }
  | { mode: 'restore'; item: TrashNodeView }
  | null

export function FilesPage() {
  const { api, user } = useAuth()
  const rootId = user?.rootNodeId ?? ''
  const [path, setPath] = useState<Breadcrumb[]>(() =>
    rootId ? [{ id: rootId, name: '我的文件' }] : [],
  )
  const [items, setItems] = useState<FileNodeView[]>([])
  const [nextCursor, setNextCursor] = useState<string | null>(null)
  const [loadingMore, setLoadingMore] = useState(false)
  const loadingMoreRef = useRef(false)
  const [usage, setUsage] = useState<UsageView | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [reloadToken, setReloadToken] = useState(0)

  const [createOpen, setCreateOpen] = useState(false)
  const [createName, setCreateName] = useState('')
  const [createError, setCreateError] = useState<string | null>(null)

  const [renameTarget, setRenameTarget] = useState<FileNodeView | null>(null)
  const [renameName, setRenameName] = useState('')
  const [renameError, setRenameError] = useState<string | null>(null)

  const [trashOpen, setTrashOpen] = useState(false)
  const [trash, setTrash] = useState<TrashNodeView[]>([])
  const [trashError, setTrashError] = useState<string | null>(null)
  const [confirmPermanentId, setConfirmPermanentId] = useState<string | null>(
    null,
  )

  const [picker, setPicker] = useState<PickerState>(null)
  const [versionsTarget, setVersionsTarget] = useState<FileNodeView | null>(null)
  const [versions, setVersions] = useState<VersionView[]>([])
  const [versionsError, setVersionsError] = useState<string | null>(null)

  const current = path[path.length - 1]
  const currentId = current?.id ?? null

  const refresh = useCallback(() => {
    setLoading(true)
    setReloadToken((value) => value + 1)
  }, [])

  useEffect(() => {
    if (!api || !currentId) {
      return
    }
    let cancelled = false
    void (async () => {
      try {
        const page = await api.listFolder(currentId)
        if (!cancelled) {
          setItems(sortByName(page.items))
          setNextCursor(page.nextCursor)
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
  }, [api, currentId, reloadToken])

  const loadMore = useCallback(async () => {
    if (!api || !currentId || !nextCursor || loadingMoreRef.current) {
      return
    }
    loadingMoreRef.current = true
    setLoadingMore(true)
    try {
      const page = await api.listFolder(currentId, nextCursor)
      setItems((previous) => sortByName([...previous, ...page.items]))
      setNextCursor(page.nextCursor)
    } catch (caught) {
      setError(errorMessage(caught))
    } finally {
      loadingMoreRef.current = false
      setLoadingMore(false)
    }
  }, [api, currentId, nextCursor])

  useEffect(() => {
    if (!api) {
      return
    }
    let cancelled = false
    void (async () => {
      try {
        const next = await api.getUsage()
        if (!cancelled) {
          setUsage(next)
        }
      } catch {
        // 用量失败不阻塞文件浏览；页面会显示占位符。
      }
    })()
    return () => {
      cancelled = true
    }
  }, [api, reloadToken])

  const loadTrash = useCallback(async () => {
    if (!api) {
      return
    }
    try {
      setTrash(sortByName(await api.listTrash()))
      setTrashError(null)
    } catch (caught) {
      setTrashError(errorMessage(caught))
    }
  }, [api])

  const openTrash = () => {
    setTrashOpen(true)
    setConfirmPermanentId(null)
    void loadTrash()
  }

  const submitCreate = async () => {
    if (!api || !current) {
      return
    }
    const message = validateName(createName)
    if (message) {
      setCreateError(message)
      return
    }
    setBusy('create')
    try {
      await api.createFolder(current.id, createName)
      setCreateOpen(false)
      setCreateName('')
      setCreateError(null)
      setNotice('文件夹已创建')
      refresh()
    } catch (caught) {
      setCreateError(errorMessage(caught))
    } finally {
      setBusy(null)
    }
  }

  const openRename = (node: FileNodeView) => {
    setRenameTarget(node)
    setRenameName(node.name)
    setRenameError(null)
  }

  const submitRename = async () => {
    if (!api || !renameTarget) {
      return
    }
    const message = validateName(renameName)
    if (message) {
      setRenameError(message)
      return
    }
    setBusy('rename')
    try {
      await api.renameNode(
        renameTarget.id,
        renameTarget.parentId ?? currentId ?? rootId,
        renameName,
        renameTarget.rowVersion,
      )
      setRenameTarget(null)
      setNotice('已重命名')
      refresh()
    } catch (caught) {
      setRenameError(errorMessage(caught))
      if (errorCode(caught) === 'conflict') {
        refresh()
      }
    } finally {
      setBusy(null)
    }
  }

  const softDelete = async (node: FileNodeView) => {
    if (!api) {
      return
    }
    setBusy(`delete:${node.id}`)
    try {
      await api.deleteNode(node.id)
      setNotice(`“${node.name}”已移入回收站`)
      refresh()
      if (trashOpen) {
        void loadTrash()
      }
    } catch (caught) {
      setError(errorMessage(caught))
    } finally {
      setBusy(null)
    }
  }

  const submitMove = async (target: { id: string; name: string }) => {
    if (!api || !picker || picker.mode !== 'move') {
      return
    }
    setBusy('move')
    try {
      await api.moveNode(
        picker.node.id,
        target.id,
        picker.node.name,
        picker.node.rowVersion,
      )
      setPicker(null)
      setNotice(`已移动到“${target.name}”`)
      refresh()
    } catch (caught) {
      setError(errorMessage(caught))
      setPicker(null)
    } finally {
      setBusy(null)
    }
  }

  const restoreItem = async (item: TrashNodeView) => {
    if (!api) {
      return
    }
    setBusy(`restore:${item.id}`)
    try {
      await api.restoreNode(item.id)
      setNotice(`“${item.name}”已恢复`)
      refresh()
      void loadTrash()
    } catch (caught) {
      if (errorCode(caught) === 'target_unavailable') {
        setError('原位置不可用，请选择新的目标文件夹')
        setPicker({ mode: 'restore', item })
      } else {
        setError(errorMessage(caught))
      }
    } finally {
      setBusy(null)
    }
  }

  const submitRestoreTo = async (target: { id: string; name: string }) => {
    if (!api || !picker || picker.mode !== 'restore') {
      return
    }
    setBusy('restore')
    try {
      await api.restoreNode(picker.item.id, target.id, picker.item.name)
      setPicker(null)
      setNotice(`“${picker.item.name}”已恢复到“${target.name}”`)
      refresh()
      void loadTrash()
    } catch (caught) {
      setError(errorMessage(caught))
      setPicker(null)
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
      setConfirmPermanentId(null)
      setNotice(`已永久删除，释放 ${formatBytes(result.freedBytes)} 配额`)
      refresh()
      void loadTrash()
    } catch (caught) {
      setError(errorMessage(caught))
    } finally {
      setBusy(null)
    }
  }

  const openVersions = async (node: FileNodeView) => {
    if (!api) {
      return
    }
    setVersionsTarget(node)
    setVersions([])
    setVersionsError(null)
    try {
      setVersions(await api.listVersions(node.id))
    } catch (caught) {
      setVersionsError(errorMessage(caught))
    }
  }

  const restoreVersion = async (version: VersionView) => {
    if (!api || !versionsTarget) {
      return
    }
    setBusy(`version:${version.id}`)
    try {
      await api.restoreVersion(versionsTarget.id, version.id)
      setVersions(await api.listVersions(versionsTarget.id))
      setNotice('已恢复该版本为当前版本')
      refresh()
    } catch (caught) {
      setVersionsError(errorMessage(caught))
    } finally {
      setBusy(null)
    }
  }

  return (
    <main className="mx-auto flex min-h-dvh w-full max-w-5xl flex-col gap-6 p-6">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-3xl font-semibold tracking-tight text-slate-900">
            文件管理
          </h1>
          <p className="text-sm text-slate-600">
            名称与文件内容在 Worker 内加密；服务端只保存结构与密文。
          </p>
          <p data-testid="usage-summary" className="mt-2 text-sm text-slate-700">
            {usage
              ? `已用 ${formatBytes(usage.usedBytes)} / 总额 ${formatBytes(usage.quotaBytes)}（预留 ${formatBytes(usage.reservedBytes)}）`
              : '用量加载中…'}
          </p>
        </div>
        <nav className="flex flex-wrap gap-2 text-sm">
          <Link className="underline" to="/">
            返回主页
          </Link>
          <Link className="underline" to="/sessions">
            会话管理
          </Link>
        </nav>
      </header>

      <section className="space-y-3 rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <nav
            data-testid="breadcrumb"
            className="flex flex-wrap items-center gap-1 text-sm text-slate-700"
          >
            {path.map((entry, index) => (
              <span key={entry.id} className="flex items-center gap-1">
                {index > 0 && <span>/</span>}
                <button
                  type="button"
                  className="underline disabled:no-underline disabled:text-slate-500"
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
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              data-testid="new-folder"
              className="rounded-lg bg-slate-900 px-3 py-2 text-sm font-semibold text-white"
              onClick={() => {
                setCreateOpen(true)
                setCreateName('')
                setCreateError(null)
              }}
            >
              新建文件夹
            </button>
            <button
              type="button"
              data-testid="open-trash"
              className="rounded-lg border border-slate-300 px-3 py-2 text-sm"
              onClick={openTrash}
            >
              回收站
            </button>
            <button
              type="button"
              className="rounded-lg border border-slate-300 px-3 py-2 text-sm"
              onClick={refresh}
            >
              刷新
            </button>
          </div>
        </div>

        <FormError message={error} />
        <FormSuccess message={notice} />

        {loading ? (
          <p role="status" className="p-4 text-sm text-slate-600">
            加载中…
          </p>
        ) : (
          <VirtualList
            items={items}
            getKey={(item) => item.id}
            testId="file-list"
            onEndReached={() => void loadMore()}
            empty={
              <p className="p-4 text-sm text-slate-600">
                此文件夹为空。新建一个文件夹开始整理。
              </p>
            }
            renderItem={(item) => (
              <div
                data-testid="file-row"
                data-name={item.name}
                data-kind={item.kind}
                className="flex w-full items-center gap-3"
              >
                <span aria-hidden="true">
                  {item.kind === 'folder' ? '📁' : '📄'}
                </span>
                <div className="min-w-0 flex-1">
                  {item.kind === 'folder' ? (
                    <button
                      type="button"
                      aria-label={`打开 ${item.name}`}
                      className="max-w-full truncate text-left font-medium text-slate-900 underline"
                      onClick={() => {
                        setLoading(true)
                        setPath([...path, { id: item.id, name: item.name }])
                      }}
                    >
                      {item.name}
                    </button>
                  ) : (
                    <span className="block truncate font-medium text-slate-900">
                      {item.name}
                    </span>
                  )}
                  <p className="truncate text-xs text-slate-500">
                    {item.kind === 'folder'
                      ? '文件夹'
                      : `${formatBytes(item.size)} · ${formatUnixTime(item.mtime)}`}
                  </p>
                </div>
                <div className="flex shrink-0 gap-1 text-xs">
                  <button
                    type="button"
                    aria-label={`重命名 ${item.name}`}
                    disabled={busy !== null}
                    className="rounded border border-slate-300 px-2 py-1"
                    onClick={() => openRename(item)}
                  >
                    重命名
                  </button>
                  <button
                    type="button"
                    aria-label={`移动 ${item.name}`}
                    disabled={busy !== null}
                    className="rounded border border-slate-300 px-2 py-1"
                    onClick={() => setPicker({ mode: 'move', node: item })}
                  >
                    移动
                  </button>
                  {item.kind === 'file' && (
                    <button
                      type="button"
                      aria-label={`版本历史 ${item.name}`}
                      disabled={busy !== null}
                      className="rounded border border-slate-300 px-2 py-1"
                      onClick={() => void openVersions(item)}
                    >
                      版本
                    </button>
                  )}
                  <button
                    type="button"
                    aria-label={`删除 ${item.name}`}
                    disabled={busy !== null}
                    className="rounded border border-red-200 px-2 py-1 text-red-700"
                    onClick={() => void softDelete(item)}
                  >
                    删除
                  </button>
                </div>
              </div>
            )}
          />
        )}
        {!loading && loadingMore && (
          <p role="status" className="text-sm text-slate-500">
            加载更多…
          </p>
        )}
      </section>

      {trashOpen && (
        <section className="space-y-3 rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
          <div className="flex items-center justify-between">
            <h2 className="text-lg font-semibold text-slate-900">回收站</h2>
            <button
              type="button"
              className="text-sm underline"
              onClick={() => setTrashOpen(false)}
            >
              关闭
            </button>
          </div>
          <FormError message={trashError} />
          {trash.length === 0 ? (
            <p className="text-sm text-slate-600">回收站为空。</p>
          ) : (
            <div className="space-y-2">
              {trash.map((item) => (
                <article
                  key={item.id}
                  data-testid="trash-item"
                  data-name={item.name}
                  className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-slate-200 p-2"
                >
                  <div>
                    <p className="font-medium text-slate-900">{item.name}</p>
                    <p className="text-xs text-slate-500">
                      删除于 {formatTimestamp(item.deletedAt)}
                    </p>
                  </div>
                  <div className="flex gap-2 text-xs">
                    <button
                      type="button"
                      aria-label={`恢复 ${item.name}`}
                      disabled={busy !== null}
                      className="rounded border border-slate-300 px-2 py-1"
                      onClick={() => void restoreItem(item)}
                    >
                      恢复
                    </button>
                    {confirmPermanentId === item.id ? (
                      <button
                        type="button"
                        data-testid="confirm-permanent-delete"
                        aria-label={`确认永久删除 ${item.name}`}
                        disabled={busy !== null}
                        className="rounded bg-red-700 px-2 py-1 text-white"
                        onClick={() => void permanentlyDelete(item)}
                      >
                        确认永久删除
                      </button>
                    ) : (
                      <button
                        type="button"
                        aria-label={`永久删除 ${item.name}`}
                        disabled={busy !== null}
                        className="rounded border border-red-200 px-2 py-1 text-red-700"
                        onClick={() => setConfirmPermanentId(item.id)}
                      >
                        永久删除
                      </button>
                    )}
                  </div>
                </article>
              ))}
            </div>
          )}
        </section>
      )}

      {picker?.mode === 'move' && (
        <FolderPicker
          key={`move-${picker.node.id}`}
          api={api}
          startId={rootId}
          startName="我的文件"
          title={`移动“${picker.node.name}”`}
          excludeId={picker.node.id}
          onCancel={() => setPicker(null)}
          onSelect={(folder) => void submitMove(folder)}
        />
      )}

      {picker?.mode === 'restore' && (
        <FolderPicker
          key={`restore-${picker.item.id}`}
          api={api}
          startId={rootId}
          startName="我的文件"
          title={`恢复“${picker.item.name}”到新位置`}
          onCancel={() => setPicker(null)}
          onSelect={(folder) => void submitRestoreTo(folder)}
        />
      )}

      {createOpen && (
        <div
          role="dialog"
          aria-modal="true"
          aria-label="新建文件夹"
          className="fixed inset-0 z-30 flex items-center justify-center bg-slate-900/40 p-4"
        >
          <div className="w-full max-w-sm space-y-3 rounded-2xl bg-white p-4 shadow-xl">
            <h2 className="text-lg font-semibold text-slate-900">新建文件夹</h2>
            <input
              data-testid="folder-name"
              className="w-full rounded-lg border border-slate-300 px-3 py-2"
              value={createName}
              onChange={(event) => setCreateName(event.target.value)}
              placeholder="文件夹名称"
              autoFocus
            />
            <FormError message={createError} />
            <div className="flex justify-end gap-2">
              <button
                type="button"
                className="rounded-lg border border-slate-300 px-3 py-2 text-sm"
                onClick={() => setCreateOpen(false)}
              >
                取消
              </button>
              <button
                type="button"
                data-testid="create-folder-submit"
                disabled={busy !== null}
                className="rounded-lg bg-slate-900 px-3 py-2 text-sm font-semibold text-white"
                onClick={() => void submitCreate()}
              >
                创建
              </button>
            </div>
          </div>
        </div>
      )}

      {renameTarget && (
        <div
          role="dialog"
          aria-modal="true"
          aria-label="重命名"
          className="fixed inset-0 z-30 flex items-center justify-center bg-slate-900/40 p-4"
        >
          <div className="w-full max-w-sm space-y-3 rounded-2xl bg-white p-4 shadow-xl">
            <h2 className="text-lg font-semibold text-slate-900">重命名</h2>
            <input
              data-testid="rename-name"
              className="w-full rounded-lg border border-slate-300 px-3 py-2"
              value={renameName}
              onChange={(event) => setRenameName(event.target.value)}
              autoFocus
            />
            <FormError message={renameError} />
            <div className="flex justify-end gap-2">
              <button
                type="button"
                className="rounded-lg border border-slate-300 px-3 py-2 text-sm"
                onClick={() => setRenameTarget(null)}
              >
                取消
              </button>
              <button
                type="button"
                data-testid="rename-submit"
                disabled={busy !== null}
                className="rounded-lg bg-slate-900 px-3 py-2 text-sm font-semibold text-white"
                onClick={() => void submitRename()}
              >
                保存
              </button>
            </div>
          </div>
        </div>
      )}

      {versionsTarget && (
        <div
          role="dialog"
          aria-modal="true"
          aria-label="版本历史"
          className="fixed inset-0 z-30 flex items-center justify-center bg-slate-900/40 p-4"
        >
          <div className="w-full max-w-lg space-y-3 rounded-2xl bg-white p-4 shadow-xl">
            <div className="flex items-center justify-between">
              <h2 className="text-lg font-semibold text-slate-900">
                版本历史 · {versionsTarget.name}
              </h2>
              <button
                type="button"
                className="text-sm underline"
                onClick={() => setVersionsTarget(null)}
              >
                关闭
              </button>
            </div>
            <FormError message={versionsError} />
            {versions.length === 0 ? (
              <p className="text-sm text-slate-600">没有可用版本。</p>
            ) : (
              <ul className="space-y-2">
                {versions.map((version, index) => (
                  <li
                    key={version.id}
                    className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-slate-200 p-2 text-sm"
                  >
                    <div>
                      <p className="font-medium text-slate-900">
                        {formatTimestamp(version.createdAt)}
                        {version.isCurrent && (
                          <span className="ml-2 rounded bg-emerald-100 px-2 py-0.5 text-xs text-emerald-800">
                            当前版本
                          </span>
                        )}
                      </p>
                      <p className="text-xs text-slate-500">
                        {formatBytes(version.size)} · {version.mime || '未知类型'}
                      </p>
                    </div>
                    {!version.isCurrent && (
                      <button
                        type="button"
                        data-testid={`restore-version-${index}`}
                        aria-label={`恢复版本 ${version.id}`}
                        disabled={busy !== null}
                        className="rounded border border-slate-300 px-2 py-1 text-xs"
                        onClick={() => void restoreVersion(version)}
                      >
                        恢复此版本
                      </button>
                    )}
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      )}
    </main>
  )
}
