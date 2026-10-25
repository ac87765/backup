/**
 * 主线程选择下载目标（task 8.8 / design D11）。
 *
 * - File System Access API 可用时优先拿到文件句柄，交给 Worker 直接写；
 * - 否则使用同源自托管 Service Worker 流式下载；
 * - 两者都不可用时退化为内存 Blob（≤ 2 GiB）。
 *
 * 测试可通过 `window.__BACKUP_FORCE_DOWNLOAD_FALLBACK__ = 'blob'` 强制走 Blob，
 * 生产代码不会设置该开关，也不会因此降低默认路径的安全性。
 */
import type {
  DownloadTargetView,
  FileSystemFileHandleLike,
} from '../worker/types'

const MAX_BLOB_BYTES = 2 * 1024 * 1024 * 1024

interface PickerWindow extends Window {
  showSaveFilePicker?: (options?: {
    suggestedName?: string
  }) => Promise<FileSystemFileHandleLike>
  __BACKUP_FORCE_DOWNLOAD_FALLBACK__?: string
}

export interface DownloadSelection {
  target: DownloadTargetView
  cleanup: () => void
}

function forcedFallback(): string | undefined {
  return (window as PickerWindow).__BACKUP_FORCE_DOWNLOAD_FALLBACK__
}

export async function selectDownloadTarget(
  fileName: string,
  mime: string,
  size: number,
): Promise<DownloadSelection> {
  const forced = forcedFallback()
  if (forced === 'blob') {
    return selectBlob(size)
  }
  if (forced === 'service-worker') {
    return selectServiceWorker(fileName, mime)
  }

  const picker = (window as PickerWindow).showSaveFilePicker
  if (typeof picker === 'function') {
    const handle = await picker({ suggestedName: fileName })
    return { target: { kind: 'file-system', handle }, cleanup: () => undefined }
  }

  if ('serviceWorker' in navigator) {
    return selectServiceWorker(fileName, mime)
  }

  return selectBlob(size)
}

async function selectServiceWorker(
  fileName: string,
  mime: string,
): Promise<DownloadSelection> {
  if (!('serviceWorker' in navigator)) {
    throw Object.assign(new Error('当前浏览器不支持流式下载'), {
      code: 'service_worker_unavailable',
    })
  }
  const downloadId = crypto.randomUUID()
  const cleanup = await prepareServiceWorkerDownload(downloadId, fileName, mime)
  return { target: { kind: 'service-worker', downloadId }, cleanup }
}

function selectBlob(size: number): DownloadSelection {
  if (size > MAX_BLOB_BYTES) {
    throw Object.assign(new Error('当前浏览器只能以内存方式下载，文件不能超过 2 GiB'), {
      code: 'file_too_large',
    })
  }
  return { target: { kind: 'blob' }, cleanup: () => undefined }
}

/** 触发浏览器保存 Worker 返回的 Blob（仅显式降级路径）。 */
export function saveBlob(blob: Blob, fileName: string): void {
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = fileName
  anchor.rel = 'noopener'
  document.body.appendChild(anchor)
  anchor.click()
  anchor.remove()
  setTimeout(() => URL.revokeObjectURL(url), 60_000)
}

async function prepareServiceWorkerDownload(
  downloadId: string,
  fileName: string,
  mime: string,
): Promise<() => void> {
  const registration = await navigator.serviceWorker.register('/download-sw.js')
  await navigator.serviceWorker.ready
  // 首次注册时页面可能还没有被 SW 控制；clients.claim 后会触发 controllerchange。
  if (!navigator.serviceWorker.controller) {
    await new Promise<void>((resolve) => {
      const timer = setTimeout(resolve, 3_000)
      navigator.serviceWorker.addEventListener(
        'controllerchange',
        () => {
          clearTimeout(timer)
          resolve()
        },
        { once: true },
      )
    })
  }
  const worker = registration.active ?? navigator.serviceWorker.controller
  if (!worker) {
    throw Object.assign(new Error('下载准备尚未完成，请重试'), {
      code: 'service_worker_unavailable',
    })
  }

  await new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(() => {
      navigator.serviceWorker.removeEventListener('message', onMessage)
      reject(
        Object.assign(new Error('下载准备超时，请重试'), {
          code: 'service_worker_timeout',
        }),
      )
    }, 5_000)
    const onMessage = (event: MessageEvent) => {
      const data = event.data as { type?: string; downloadId?: string } | undefined
      if (data?.type === 'download-ready' && data.downloadId === downloadId) {
        clearTimeout(timeout)
        navigator.serviceWorker.removeEventListener('message', onMessage)
        resolve()
      }
    }
    navigator.serviceWorker.addEventListener('message', onMessage)
    worker.postMessage({ type: 'prepare', downloadId, fileName, mime })
  })

  const iframe = document.createElement('iframe')
  iframe.hidden = true
  iframe.src = `/download-sw.js?download=${encodeURIComponent(downloadId)}`
  document.body.appendChild(iframe)
  return () => {
    setTimeout(() => iframe.remove(), 1_000)
  }
}
