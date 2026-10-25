import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { InputHTMLAttributes } from 'react'
import { FolderPlus, Search, Upload, UploadCloud } from 'lucide-react'

import { useAuth } from '../auth/context'
import { errorCode, errorMessage } from '../auth/errors'
import { AppShell } from '../shell/AppShell'
import { PageHeader } from '../shell/PageHeader'
import { useUsage } from '../shell/usage-context'
import {
  Button,
  EmptyState,
  FormError,
  prefersReducedMotion,
  useMediaQuery,
  useToast,
} from '../ui'
import type {
  FileNodeView,
  TransferProgressView,
  VersionView,
} from '../worker/types'
import { CreateFolderDialog } from './dialogs/CreateFolderDialog'
import { MoveDialog } from './dialogs/MoveDialog'
import { RenameDialog } from './dialogs/RenameDialog'
import { VersionsDialog } from './dialogs/VersionsDialog'
import { saveBlob, selectDownloadTarget, selectFolderDownloadTarget } from './download-target'
import { FileGrid } from './FileGrid'
import { FileList } from './FileList'
import { FileSkeleton } from './FileSkeleton'
import type { FileActions } from './file-actions'
import {
  entriesFromDataTransfer,
  scanDroppedEntries,
  scanFileList,
  type FolderScanTask,
} from './folder-scan'
import { FolderUploadJob, type FolderJobView } from './folder-job'
import { FileToolbar, ViewToggle } from './FileToolbar'
import { sortByName } from './format'
import { validateName } from './name-validation'
import {
  TransferPanel,
  type TransferItemView,
} from './TransferPanel'
import {
  filterFileNodes,
  loadSort,
  loadViewMode,
  nextSort,
  saveSort,
  saveViewMode,
  sortFileNodes,
  type SortField,
  type ViewMode,
} from './view-preferences'

interface PathEntry {
  id: string
  name: string
}

type PickerState = { mode: 'move'; node: FileNodeView } | null

export function FileBrowser() {
  const { api, user } = useAuth()
  const rootId = user?.rootNodeId ?? ''
  const { refreshUsage } = useUsage()
  const { showToast } = useToast()
  const isMobile = useMediaQuery('(max-width: 767px)')

  const [path, setPath] = useState<PathEntry[]>(() =>
    rootId ? [{ id: rootId, name: '全部文件' }] : [],
  )
  const [items, setItems] = useState<FileNodeView[]>([])
  const [nextCursor, setNextCursor] = useState<string | null>(null)
  const [loadingMore, setLoadingMore] = useState(false)
  const loadingMoreRef = useRef(false)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [reloadToken, setReloadToken] = useState(0)

  const [filter, setFilter] = useState('')
  const [viewMode, setViewMode] = useState<ViewMode>(() => loadViewMode())
  const [sort, setSort] = useState(() => loadSort())
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [removingIds, setRemovingIds] = useState<Set<string>>(
    () => new Set(),
  )

  const [createOpen, setCreateOpen] = useState(false)
  const [createName, setCreateName] = useState('')
  const [createError, setCreateError] = useState<string | null>(null)

  const [renameTarget, setRenameTarget] = useState<FileNodeView | null>(null)
  const [renameName, setRenameName] = useState('')
  const [renameError, setRenameError] = useState<string | null>(null)

  const [picker, setPicker] = useState<PickerState>(null)
  const [versionsTarget, setVersionsTarget] = useState<FileNodeView | null>(null)
  const [versions, setVersions] = useState<VersionView[]>([])
  const [versionsError, setVersionsError] = useState<string | null>(null)

  const [transfers, setTransfers] = useState<TransferItemView[]>([])
  const [folderJobs, setFolderJobs] = useState<FolderJobView[]>([])
  const [scanning, setScanning] = useState(false)
  const [scannedItems, setScannedItems] = useState(0)
  const [dragActive, setDragActive] = useState(false)
  const uploadInputRef = useRef<HTMLInputElement>(null)
  const folderInputRef = useRef<HTMLInputElement>(null)
  const versionInputRef = useRef<HTMLInputElement>(null)
  const versionTargetRef = useRef<FileNodeView | null>(null)
  const folderJobsRef = useRef(new Map<string, FolderUploadJob>())
  const folderDownloadCancels = useRef(new Map<string, () => void>())
  const pendingUploadsLoaded = useRef(false)
  const lastOpenRef = useRef<{ id: string; at: number }>({ id: '', at: 0 })
  const dragDepthRef = useRef(0)

  const current = path[path.length - 1]
  const currentId = current?.id ?? null

  const refresh = useCallback(() => {
    setLoading(true)
    setReloadToken((value) => value + 1)
  }, [])

  const removeWithAnimation = useCallback(
    (id: string, after: () => void) => {
      setRemovingIds((previous) => new Set(previous).add(id))
      const delay = prefersReducedMotion() ? 0 : 180
      window.setTimeout(() => {
        setRemovingIds((previous) => {
          const next = new Set(previous)
          next.delete(id)
          return next
        })
        after()
      }, delay)
    },
    [],
  )

  const patchTransfer = useCallback(
    (localId: string, patch: Partial<TransferItemView>) => {
      setTransfers((previous) =>
        previous.map((item) =>
          item.localId === localId ? { ...item, ...patch } : item,
        ),
      )
    },
    [],
  )

  const onTransferProgress = useCallback(
    (localId: string, progress: TransferProgressView) => {
      setTransfers((previous) =>
        previous.map((item) =>
          item.localId === localId
            ? {
                ...item,
                status: progress.status,
                completedChunks: progress.completedChunks,
                totalChunks: progress.totalChunks,
                completedBytes: progress.completedBytes,
                totalBytes: progress.totalBytes,
                sessionId:
                  item.direction === 'upload'
                    ? progress.transferId
                    : item.sessionId,
                downloadId:
                  item.direction === 'download'
                    ? progress.transferId
                    : item.downloadId,
                errorCode: progress.errorCode,
                errorMessage: progress.errorMessage,
              }
            : item,
        ),
      )
    },
    [],
  )

  const startUpload = useCallback(
    async (file: File, parentId: string, versionNodeId?: string) => {
      if (!api) {
        return
      }
      const localId = crypto.randomUUID()
      const item: TransferItemView = {
        localId,
        direction: 'upload',
        fileName: file.name,
        status: 'pending',
        completedChunks: 0,
        totalChunks: 0,
        completedBytes: 0,
        totalBytes: file.size,
        file,
        parentId,
        ...(versionNodeId ? { nodeId: versionNodeId } : {}),
      }
      setTransfers((previous) => [...previous, item])
      try {
        const progress = (event: TransferProgressView) =>
          onTransferProgress(localId, event)
        if (versionNodeId) {
          await api.uploadNewVersion(versionNodeId, file, progress)
        } else {
          await api.uploadFile(parentId, file, progress)
        }
        patchTransfer(localId, {
          status: 'completed',
          completedBytes: file.size,
          totalBytes: file.size,
          file: undefined,
        })
        showToast(`“${file.name}”上传完成`)
        refresh()
        refreshUsage()
      } catch (caught) {
        const code = errorCode(caught) ?? undefined
        patchTransfer(localId, {
          status: code === 'cancelled' ? 'cancelled' : 'failed',
          errorCode: code,
          errorMessage: errorMessage(caught),
        })
      }
    },
    [
      api,
      onTransferProgress,
      patchTransfer,
      refresh,
      refreshUsage,
      showToast,
    ],
  )

  const updateFolderJob = useCallback((snapshot: FolderJobView) => {
    setFolderJobs((previous) => {
      const index = previous.findIndex((job) => job.id === snapshot.id)
      if (index === -1) {
        return [...previous, snapshot]
      }
      const next = [...previous]
      next[index] = snapshot
      return next
    })
  }, [])

  const startFolderTasks = useCallback(
    async (tasks: FolderScanTask[]) => {
      if (!api || !current) {
        return
      }
      const parentId = current.id
      for (const task of tasks) {
        const job = new FolderUploadJob({
          task,
          parentId,
          api,
          onUpdate: updateFolderJob,
        })
        folderJobsRef.current.set(job.id, job)
        updateFolderJob(job.snapshot())
        await job.start()
        const snapshot = job.snapshot()
        if (snapshot.status === 'done') {
          showToast(
            `“${task.rootName}”备份完成：新增 ${snapshot.counts.added} · 更新 ${snapshot.counts.updated} · 未变化 ${snapshot.counts.unchanged}`,
          )
        } else if (snapshot.status === 'failed' && snapshot.errorMessage) {
          showToast(snapshot.errorMessage, 'error')
        }
        if (task.skipped.length > 0) {
          showToast(
            `有 ${task.skipped.length} 项被跳过：${task.skipped[0]?.reason ?? '名称不合法'}`,
            'info',
          )
        }
        refresh()
        refreshUsage()
      }
    },
    [api, current, refresh, refreshUsage, showToast, updateFolderJob],
  )

  const handleFolderInput = (files: FileList | null) => {
    if (!files || files.length === 0) {
      return
    }
    const result = scanFileList(Array.from(files))
    if (result.tasks.length === 0) {
      showToast('这个文件夹里没有可备份的文件', 'info')
      return
    }
    void startFolderTasks(result.tasks)
  }

  const startDownload = useCallback(
    async (
      nodeId: string,
      versionId: string,
      fileName: string,
      mime: string,
      size: number,
    ) => {
      if (!api) {
        return
      }
      const localId = crypto.randomUUID()
      const item: TransferItemView = {
        localId,
        direction: 'download',
        fileName,
        status: 'pending',
        completedChunks: 0,
        totalChunks: 0,
        completedBytes: 0,
        totalBytes: size,
        nodeId,
        versionId,
        mime,
        size,
      }
      setTransfers((previous) => [...previous, item])
      let cleanup: () => void = () => undefined
      try {
        const selection = await selectDownloadTarget(fileName, mime, size)
        cleanup = selection.cleanup
        const downloadId =
          selection.target.kind === 'service-worker'
            ? selection.target.downloadId
            : crypto.randomUUID()
        patchTransfer(localId, { downloadId })
        const result = await api.downloadVersion(
          nodeId,
          versionId,
          downloadId,
          fileName,
          selection.target,
          (event) => onTransferProgress(localId, event),
        )
        if (result.blob) {
          saveBlob(result.blob, fileName)
        }
        patchTransfer(localId, {
          status: 'completed',
          completedBytes: size,
          totalBytes: size,
        })
        showToast(`“${fileName}”下载完成`)
      } catch (caught) {
        const code =
          errorCode(caught) ??
          (caught instanceof DOMException && caught.name === 'AbortError'
            ? 'cancelled'
            : undefined)
        patchTransfer(localId, {
          status: code === 'cancelled' ? 'cancelled' : 'failed',
          errorCode: code,
          errorMessage: errorMessage(caught),
        })
      } finally {
        cleanup()
      }
    },
    [api, onTransferProgress, patchTransfer, showToast],
  )

  const startFolderDownload = useCallback(
    async (folderId: string, folderName: string) => {
      if (!api) {
        return
      }
      const baseName = folderId === rootId ? '加密备份' : folderName
      const fileName = `${baseName}.zip`
      const jobId = crypto.randomUUID()
      let latest: FolderJobView = {
        id: jobId,
        name: baseName,
        kind: 'folder',
        direction: 'download',
        status: 'running',
        doneFiles: 0,
        totalFiles: 0,
        completedBytes: 0,
        totalBytes: 0,
        counts: { added: 0, updated: 0, unchanged: 0, failed: 0 },
        children: [],
        hiddenChildren: 0,
        retrying: false,
        retryable: false,
      }
      const patch = (changes: Partial<FolderJobView>) => {
        latest = { ...latest, ...changes }
        updateFolderJob(latest)
      }
      updateFolderJob(latest)
      let cleanup: () => void = () => undefined
      try {
        const selection = await selectFolderDownloadTarget(fileName)
        cleanup = selection.cleanup
        const downloadId =
          selection.target.kind === 'service-worker'
            ? selection.target.downloadId
            : crypto.randomUUID()
        folderDownloadCancels.current.set(jobId, () => {
          void api.cancelDownload(downloadId)
        })
        const result = await api.downloadFolder(
          folderId,
          fileName,
          downloadId,
          selection.target,
          (event) => {
            patch({
              status:
                event.status === 'completed'
                  ? 'done'
                  : event.status === 'cancelled'
                    ? 'cancelled'
                    : event.status === 'failed'
                      ? 'failed'
                      : 'running',
              doneFiles: event.completedChunks,
              totalFiles: event.totalChunks,
              completedBytes: event.completedBytes,
              totalBytes: event.totalBytes,
              retrying: event.status === 'retrying',
              ...(event.errorMessage
                ? { errorMessage: event.errorMessage }
                : {}),
            })
          },
        )
        if (result.blob) {
          saveBlob(result.blob, fileName)
        }
        patch({ status: 'done', retrying: false })
        showToast(`“${fileName}”下载完成`)
      } catch (caught) {
        const code = errorCode(caught) ?? undefined
        patch({
          status: code === 'cancelled' ? 'cancelled' : 'failed',
          retrying: false,
          errorCode: code,
          errorMessage: errorMessage(caught),
        })
      } finally {
        cleanup()
        folderDownloadCancels.current.delete(jobId)
      }
    },
    [api, rootId, showToast, updateFolderJob],
  )

  const pauseTransfer = useCallback(
    async (item: TransferItemView) => {
      if (!api) {
        return
      }
      try {
        if (item.direction === 'upload' && item.sessionId) {
          await api.pauseUpload(item.sessionId)
        } else if (item.direction === 'download' && item.downloadId) {
          await api.pauseDownload(item.downloadId)
        }
        patchTransfer(item.localId, { status: 'paused' })
      } catch (caught) {
        patchTransfer(item.localId, { errorMessage: errorMessage(caught) })
      }
    },
    [api, patchTransfer],
  )

  const resumeTransfer = useCallback(
    async (item: TransferItemView) => {
      if (!api) {
        return
      }
      try {
        if (item.direction === 'upload' && item.sessionId) {
          await api.resumeUpload(item.sessionId)
        } else if (item.direction === 'download' && item.downloadId) {
          await api.resumeDownload(item.downloadId)
        }
        patchTransfer(item.localId, {
          status: item.direction === 'upload' ? 'uploading' : 'downloading',
        })
      } catch (caught) {
        patchTransfer(item.localId, { errorMessage: errorMessage(caught) })
      }
    },
    [api, patchTransfer],
  )

  const cancelTransfer = useCallback(
    async (item: TransferItemView) => {
      if (!api) {
        return
      }
      try {
        if (item.direction === 'upload' && item.sessionId) {
          await api.cancelUpload(item.sessionId)
          refresh()
          refreshUsage()
        } else if (item.direction === 'download' && item.downloadId) {
          await api.cancelDownload(item.downloadId)
        }
        patchTransfer(item.localId, {
          status: 'cancelled',
          file: undefined,
          pendingResume: false,
        })
      } catch (caught) {
        patchTransfer(item.localId, {
          status: 'failed',
          errorMessage: errorMessage(caught),
        })
      }
    },
    [api, patchTransfer, refresh, refreshUsage],
  )

  const retryTransfer = useCallback(
    (item: TransferItemView) => {
      if (item.direction === 'download' && item.nodeId && item.versionId) {
        setTransfers((previous) =>
          previous.filter((entry) => entry.localId !== item.localId),
        )
        void startDownload(
          item.nodeId,
          item.versionId,
          item.fileName,
          item.mime ?? 'application/octet-stream',
          item.size ?? item.totalBytes,
        )
        return
      }
      if (item.direction === 'upload' && item.sessionId && item.file) {
        patchTransfer(item.localId, {
          status: 'uploading',
          errorCode: undefined,
          errorMessage: undefined,
        })
        void api
          ?.resumePendingUpload(
            item.sessionId,
            item.file,
            (event) => onTransferProgress(item.localId, event),
          )
          .then(() => {
            patchTransfer(item.localId, {
              status: 'completed',
              completedBytes: item.totalBytes,
              file: undefined,
            })
            refresh()
            refreshUsage()
          })
          .catch((caught: unknown) => {
            patchTransfer(item.localId, {
              status: 'failed',
              errorCode: errorCode(caught) ?? undefined,
              errorMessage: errorMessage(caught),
            })
          })
        return
      }
      if (item.direction === 'upload' && item.file) {
        // 会话尚未创建成功（例如创建请求网络失败）：重新开始一次上传。
        setTransfers((previous) =>
          previous.filter((entry) => entry.localId !== item.localId),
        )
        void startUpload(item.file, item.parentId ?? rootId, item.nodeId)
      }
    },
    [
      api,
      onTransferProgress,
      patchTransfer,
      refresh,
      refreshUsage,
      startDownload,
      startUpload,
      rootId,
    ],
  )

  const resumeUploadFile = useCallback(
    async (item: TransferItemView, file: File) => {
      if (!api || !item.sessionId) {
        return
      }
      patchTransfer(item.localId, {
        status: 'uploading',
        file,
        pendingResume: false,
        errorCode: undefined,
        errorMessage: undefined,
      })
      try {
        await api.resumePendingUpload(
          item.sessionId,
          file,
          (event) => onTransferProgress(item.localId, event),
        )
        patchTransfer(item.localId, {
          status: 'completed',
          completedBytes: file.size,
          totalBytes: file.size,
          file: undefined,
        })
        showToast(`“${file.name}”上传完成`)
        refresh()
        refreshUsage()
      } catch (caught) {
        const code = errorCode(caught)
        patchTransfer(item.localId, {
          status: code === 'cancelled' ? 'cancelled' : 'failed',
          pendingResume: code === 'fingerprint_mismatch',
          errorCode: code ?? undefined,
          errorMessage: errorMessage(caught),
        })
      }
    },
    [
      api,
      onTransferProgress,
      patchTransfer,
      refresh,
      refreshUsage,
      showToast,
    ],
  )

  const dismissTransfer = useCallback((item: TransferItemView) => {
    setTransfers((previous) =>
      previous.filter((entry) => entry.localId !== item.localId),
    )
  }, [])

  const dismissCompleted = useCallback(() => {
    setTransfers((previous) =>
      previous.filter(
        (entry) =>
          entry.status !== 'completed' &&
          entry.status !== 'cancelled' &&
          entry.status !== 'failed',
      ),
    )
  }, [])

  const handleUploadInput = (files: FileList | null) => {
    if (!files || !current) {
      return
    }
    for (const file of Array.from(files)) {
      void startUpload(file, current.id)
    }
  }

  const handleVersionInput = (files: FileList | null) => {
    const file = files?.[0]
    const target = versionTargetRef.current
    versionTargetRef.current = null
    if (!file || !target) {
      return
    }
    void startUpload(file, target.parentId ?? rootId, target.id)
  }

  const startVersionUpload = (node: FileNodeView) => {
    versionTargetRef.current = node
    versionInputRef.current?.click()
  }

  const downloadCurrentVersion = useCallback(
    (node: FileNodeView) => {
      if (!node.currentVersionId) {
        showToast('该文件没有可下载的版本', 'error')
        return
      }
      void startDownload(
        node.id,
        node.currentVersionId,
        node.name,
        node.mime ?? 'application/octet-stream',
        node.size ?? 0,
      )
    },
    [showToast, startDownload],
  )

  const openNode = useCallback(
    (node: FileNodeView) => {
      const now = Date.now()
      if (
        lastOpenRef.current.id === node.id &&
        now - lastOpenRef.current.at < 400
      ) {
        return
      }
      lastOpenRef.current = { id: node.id, at: now }
      if (node.kind === 'folder') {
        setLoading(true)
        setFilter('')
        setSelectedId(null)
        setRemovingIds(new Set())
        setPath((previous) => [
          ...previous,
          { id: node.id, name: node.name },
        ])
        return
      }
      downloadCurrentVersion(node)
    },
    [downloadCurrentVersion],
  )

  const navigateTo = useCallback((index: number) => {
    setLoading(true)
    setFilter('')
    setSelectedId(null)
    setRemovingIds(new Set())
    setPath((previous) => previous.slice(0, index + 1))
  }, [])

  const changeViewMode = (mode: ViewMode) => {
    setViewMode(mode)
    saveViewMode(mode)
  }

  const changeSort = (field: SortField) => {
    const next = nextSort(sort, field)
    setSort(next)
    saveSort(next)
  }

  const downloadVersion = (version: VersionView) => {
    if (!versionsTarget) {
      return
    }
    void startDownload(
      versionsTarget.id,
      version.id,
      versionsTarget.name,
      versionsTarget.mime ?? 'application/octet-stream',
      version.size,
    )
  }

  // 全页拖拽上传：只在页面内出现文件时显示覆盖层，松开后上传到当前文件夹。
  useEffect(() => {
    const hasFiles = (event: DragEvent): boolean =>
      Boolean(event.dataTransfer?.types.includes('Files'))

    const onDragEnter = (event: DragEvent) => {
      if (!hasFiles(event)) {
        return
      }
      event.preventDefault()
      dragDepthRef.current += 1
      setDragActive(true)
    }
    const onDragOver = (event: DragEvent) => {
      if (!hasFiles(event)) {
        return
      }
      event.preventDefault()
      if (event.dataTransfer) {
        event.dataTransfer.dropEffect = 'copy'
      }
    }
    const onDragLeave = (event: DragEvent) => {
      if (!hasFiles(event)) {
        return
      }
      dragDepthRef.current = Math.max(0, dragDepthRef.current - 1)
      if (dragDepthRef.current === 0) {
        setDragActive(false)
      }
    }
    const onDrop = (event: DragEvent) => {
      const transfer = event.dataTransfer
      if (!transfer || !transfer.types.includes('Files')) {
        return
      }
      event.preventDefault()
      dragDepthRef.current = 0
      setDragActive(false)
      if (!current) {
        return
      }
      const items = transfer.items
      const entries =
        items && items.length > 0 ? entriesFromDataTransfer(items) : []
      if (entries.length > 0) {
        setScanning(true)
        setScannedItems(0)
        void scanDroppedEntries(entries, {
          onProgress: (count) => setScannedItems(count),
        })
          .then((result) => {
            setScanning(false)
            if (result.tasks.length === 0) {
              showToast('这个文件夹里没有可备份的文件', 'info')
              return
            }
            void startFolderTasks(result.tasks)
          })
          .catch((caught: unknown) => {
            setScanning(false)
            showToast(errorMessage(caught), 'error')
          })
        return
      }
      // 浏览器不支持 webkitGetAsEntry 时回退到普通文件拖拽。
      for (const file of Array.from(transfer.files)) {
        void startUpload(file, current.id)
      }
    }

    window.addEventListener('dragenter', onDragEnter)
    window.addEventListener('dragover', onDragOver)
    window.addEventListener('dragleave', onDragLeave)
    window.addEventListener('drop', onDrop)
    return () => {
      window.removeEventListener('dragenter', onDragEnter)
      window.removeEventListener('dragover', onDragOver)
      window.removeEventListener('dragleave', onDragLeave)
      window.removeEventListener('drop', onDrop)
    }
  }, [current, showToast, startFolderTasks, startUpload])

  const folderJobsRunning = folderJobs.some(
    (job) =>
      job.status === 'scanning' ||
      job.status === 'planning' ||
      job.status === 'running',
  )
  useEffect(() => {
    if (!folderJobsRunning) {
      return
    }
    const onBeforeUnload = (event: BeforeUnloadEvent) => {
      event.preventDefault()
      event.returnValue = ''
    }
    window.addEventListener('beforeunload', onBeforeUnload)
    return () => window.removeEventListener('beforeunload', onBeforeUnload)
  }, [folderJobsRunning])

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
    if (!api || pendingUploadsLoaded.current) {
      return
    }
    pendingUploadsLoaded.current = true
    void (async () => {
      try {
        const pending = await api.listPendingUploads()
        setTransfers((previous) => [
          ...previous,
          ...pending.map<TransferItemView>((entry) => ({
            localId: `pending-${entry.sessionId}`,
            direction: 'upload',
            fileName: entry.name,
            status: 'paused',
            completedChunks: 0,
            totalChunks: 0,
            completedBytes: 0,
            totalBytes: entry.size,
            sessionId: entry.sessionId,
            nodeId: entry.nodeId,
            pendingResume: true,
          })),
        ])
      } catch {
        // 读取 IndexedDB 失败不阻塞文件浏览。
      }
    })()
  }, [api])

  const openCreate = () => {
    setCreateOpen(true)
    setCreateName('')
    setCreateError(null)
  }

  const closeCreate = () => {
    setCreateOpen(false)
    setCreateName('')
    setCreateError(null)
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
      closeCreate()
      showToast('文件夹已创建')
      refresh()
      refreshUsage()
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

  const closeRename = () => {
    setRenameTarget(null)
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
      closeRename()
      showToast('已重命名')
      refresh()
      refreshUsage()
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
      showToast(`“${node.name}”已移入回收站`)
      removeWithAnimation(node.id, () => {
        refresh()
        refreshUsage()
      })
    } catch (caught) {
      showToast(errorMessage(caught), 'error')
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
      showToast(`已移动到“${target.name}”`)
      removeWithAnimation(picker.node.id, () => {
        refresh()
        refreshUsage()
      })
    } catch (caught) {
      showToast(errorMessage(caught), 'error')
      setPicker(null)
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

  const closeVersions = () => {
    setVersionsTarget(null)
    setVersions([])
    setVersionsError(null)
  }

  const restoreVersion = async (version: VersionView) => {
    if (!api || !versionsTarget) {
      return
    }
    setBusy(`version:${version.id}`)
    try {
      await api.restoreVersion(versionsTarget.id, version.id)
      setVersions(await api.listVersions(versionsTarget.id))
      showToast('已恢复该版本为当前版本')
      refresh()
      refreshUsage()
    } catch (caught) {
      setVersionsError(errorMessage(caught))
    } finally {
      setBusy(null)
    }
  }

  const visibleItems = useMemo(
    () => sortFileNodes(filterFileNodes(items, filter), sort),
    [filter, items, sort],
  )

  const actions: FileActions = {
    busy: busy !== null,
    onOpen: openNode,
    onDownload: downloadCurrentVersion,
    onRename: openRename,
    onMove: (node) => setPicker({ mode: 'move', node }),
    onVersions: (node) => void openVersions(node),
    onNewVersion: startVersionUpload,
    onDelete: (node) => void softDelete(node),
    onDownloadFolder: (node) => void startFolderDownload(node.id, node.name),
  }

  const emptyState =
    filter.trim().length > 0 ? (
      <EmptyState
        icon={<Search size={24} />}
        title="没有匹配的文件"
        description={`当前文件夹中没有名称包含“${filter.trim()}”的项目。`}
        actions={<Button onClick={() => setFilter('')}>清除筛选</Button>}
      />
    ) : (
      <EmptyState
        icon={<FolderPlus size={24} />}
        title="这个文件夹还是空的"
        description="拖入文件，或从上传菜单开始整理你的加密备份。"
        actions={
          <>
            <Button
              variant="primary"
              icon={<Upload size={16} />}
              onClick={() => uploadInputRef.current?.click()}
            >
              上传文件
            </Button>
            <Button
              icon={<FolderPlus size={16} />}
              onClick={openCreate}
            >
              新建文件夹
            </Button>
          </>
        }
      />
    )

  return (
    <AppShell
      title={current?.name ?? '全部文件'}
      toolbar={
        <FileToolbar
          filter={filter}
          onFilterChange={setFilter}
          viewMode={viewMode}
          onViewModeChange={changeViewMode}
          onRefresh={refresh}
          onDownloadFolder={() => {
            if (current) {
              void startFolderDownload(current.id, current.name)
            }
          }}
        />
      }
      mobileSearch={{ value: filter, onChange: setFilter }}
      onUploadRequest={() => uploadInputRef.current?.click()}
      onUploadFolderRequest={() => folderInputRef.current?.click()}
      onNewFolderRequest={openCreate}
    >
      <div data-testid="file-drop-zone" className="space-y-4">
        <input
          ref={uploadInputRef}
          type="file"
          multiple
          data-testid="upload-input"
          className="hidden"
          onChange={(event) => {
            handleUploadInput(event.target.files)
            event.target.value = ''
          }}
        />
        <input
          ref={folderInputRef}
          type="file"
          multiple
          data-testid="folder-upload-input"
          className="hidden"
          {...({
            webkitdirectory: '',
            directory: '',
          } as unknown as InputHTMLAttributes<HTMLInputElement>)}
          onChange={(event) => {
            handleFolderInput(event.target.files)
            event.target.value = ''
          }}
        />
        <input
          ref={versionInputRef}
          type="file"
          data-testid="version-input"
          className="hidden"
          onChange={(event) => {
            handleVersionInput(event.target.files)
            event.target.value = ''
          }}
        />

        <PageHeader
          ancestors={path.slice(0, -1).map((entry, index) => ({
            id: entry.id,
            label: entry.name,
            onClick: () => navigateTo(index),
          }))}
          title={current?.name ?? '全部文件'}
          subtitle={
            loading
              ? '正在加载…'
              : `${visibleItems.length} 项${filter.trim() ? '（已筛选）' : ''}`
          }
          actions={
            <div className="flex items-center gap-2">
              {filter.trim() && (
                <Button size="sm" variant="ghost" onClick={() => setFilter('')}>
                  清除筛选
                </Button>
              )}
              {isMobile && (
                <ViewToggle
                  viewMode={viewMode}
                  onViewModeChange={changeViewMode}
                />
              )}
            </div>
          }
        />

        <FormError message={error} />

        <div
          key={loading ? 'loading' : `${currentId}:${viewMode}`}
          className="animate-content-in"
        >
        {loading ? (
          <FileSkeleton />
        ) : visibleItems.length === 0 ? (
          <div>{emptyState}</div>
        ) : viewMode === 'list' ? (
          <FileList
            items={visibleItems}
            sort={sort}
            onSort={changeSort}
            selectedId={selectedId}
            onSelect={setSelectedId}
            actions={actions}
            onEndReached={() => void loadMore()}
            removingIds={removingIds}
            hasMore={nextCursor !== null}
          />
        ) : (
          <FileGrid
            items={visibleItems}
            selectedId={selectedId}
            onSelect={setSelectedId}
            actions={actions}
            removingIds={removingIds}
          />
        )}
        </div>

        {!loading && loadingMore && (
          <p aria-live="polite" className="text-sm text-ink-600">
            加载更多…
          </p>
        )}
      </div>

      <TransferPanel
        key={transfers.length === 0 ? 'empty' : 'panel'}
        items={transfers}
        folderJobs={folderJobs}
        onPause={(item) => void pauseTransfer(item)}
        onResume={(item) => void resumeTransfer(item)}
        onCancel={(item) => void cancelTransfer(item)}
        onRetry={retryTransfer}
        onResumeFile={(item, file) => void resumeUploadFile(item, file)}
        onDismiss={dismissTransfer}
        onDismissCompleted={dismissCompleted}
        onCancelFolder={(job) => {
          const cancelDownload = folderDownloadCancels.current.get(job.id)
          if (cancelDownload) {
            cancelDownload()
          } else {
            void folderJobsRef.current.get(job.id)?.cancel()
          }
        }}
        onRetryFolder={(job) =>
          void folderJobsRef.current.get(job.id)?.retryFailed()
        }
        onDismissFolder={(job) => {
          folderJobsRef.current.delete(job.id)
          setFolderJobs((previous) =>
            previous.filter((entry) => entry.id !== job.id),
          )
        }}
      />

      <CreateFolderDialog
        open={createOpen}
        name={createName}
        error={createError}
        busy={busy !== null}
        onNameChange={(value) => {
          setCreateName(value)
          setCreateError(null)
        }}
        onClose={closeCreate}
        onSubmit={() => void submitCreate()}
      />

      {renameTarget && (
        <RenameDialog
          open
          name={renameName}
          error={renameError}
          busy={busy !== null}
          onNameChange={(value) => {
            setRenameName(value)
            setRenameError(null)
          }}
          onClose={closeRename}
          onSubmit={() => void submitRename()}
        />
      )}

      {picker?.mode === 'move' && (
        <MoveDialog
          node={picker.node}
          api={api}
          rootId={rootId}
          onClose={() => setPicker(null)}
          onSelect={(folder) => void submitMove(folder)}
        />
      )}

      {versionsTarget && (
        <VersionsDialog
          node={versionsTarget}
          versions={versions}
          error={versionsError}
          busy={busy !== null}
          onClose={closeVersions}
          onDownload={downloadVersion}
          onRestore={(version) => void restoreVersion(version)}
        />
      )}
      {scanning && (
        <div
          data-testid="folder-scan-overlay"
          className="animate-overlay-in pointer-events-none fixed inset-0 z-50 flex items-center justify-center bg-brand-600/90 p-6 backdrop-blur-sm"
        >
          <div className="flex max-w-lg flex-col items-center gap-3 rounded-card bg-white px-10 py-8 text-center shadow-pop">
            <UploadCloud size={40} className="text-brand-600" aria-hidden="true" />
            <p className="text-sm font-semibold text-ink-900">
              正在扫描文件夹…
            </p>
            <p className="text-sm text-ink-600">已发现 {scannedItems} 项</p>
          </div>
        </div>
      )}
      {dragActive && current && (
        <div
          data-testid="drop-overlay"
          className="animate-overlay-in pointer-events-none fixed inset-0 z-50 flex items-center justify-center bg-brand-600 p-6 backdrop-blur-sm"
        >
          <div className="animate-breathe flex max-w-lg flex-col items-center gap-3 rounded-card border-2 border-dashed border-white/80 bg-white/10 px-10 py-12 text-center text-white">
            <UploadCloud size={48} aria-hidden="true" />
            <p className="text-lg font-semibold">
              松开以上传到“{current.name}”
            </p>
            <p className="text-sm text-white">
              文件或文件夹会在本机加密后上传
            </p>
          </div>
        </div>
      )}
    </AppShell>
  )
}
