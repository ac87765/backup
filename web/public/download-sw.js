/*
 * 自托管流式下载 Service Worker（design D11 / task 8.8）。
 *
 * 主线程先 postMessage({type:'prepare', downloadId, fileName, mime})；Worker 再把
 * 解密后的块通过同名 BroadcastChannel 送入本 SW。页面创建一个隐藏 iframe 导航到
 * `/download-sw.js?download=<id>`，本 SW 以 Content-Disposition: attachment 响应，
 * 浏览器即把流式数据保存为下载文件；出错时让流 error，下载失败而不是保存残缺文件。
 *
 * 本脚本是 public/ 下的普通静态文件，不经过 Vite 打包，也不接触任何密钥。
 */

const downloads = new Map()

self.addEventListener('install', () => {
  self.skipWaiting()
})

self.addEventListener('activate', (event) => {
  event.waitUntil(self.clients.claim())
})

function cleanup(downloadId) {
  const entry = downloads.get(downloadId)
  if (!entry) {
    return
  }
  downloads.delete(downloadId)
  try {
    entry.channel.close()
  } catch {
    // 忽略重复关闭。
  }
}

function startEntry(downloadId, fileName, mime) {
  const channel = new BroadcastChannel(downloadId)
  const entry = {
    fileName,
    mime,
    channel,
    controller: null,
    pending: [],
    finished: false,
    error: null,
  }
  channel.onmessage = (event) => {
    const data = event.data
    if (!data || typeof data !== 'object') {
      return
    }
    if (data.type === 'chunk') {
      const bytes = new Uint8Array(data.data)
      if (entry.controller) {
        entry.controller.enqueue(bytes)
      } else {
        entry.pending.push(bytes)
      }
      return
    }
    if (data.type === 'error') {
      entry.error = new Error('下载失败')
      if (entry.controller) {
        entry.controller.error(entry.error)
        cleanup(downloadId)
      } else {
        // 流尚未开始（iframe 导航可能还没到）：保留条目让 fetch 能返回失败的流。
        setTimeout(() => cleanup(downloadId), 60_000)
      }
      return
    }
    if (data.type === 'end') {
      entry.finished = true
      if (entry.controller) {
        entry.controller.close()
        cleanup(downloadId)
      } else {
        setTimeout(() => cleanup(downloadId), 60_000)
      }
    }
  }
  downloads.set(downloadId, entry)
  return entry
}

self.addEventListener('message', (event) => {
  const data = event.data
  if (!data || data.type !== 'prepare' || typeof data.downloadId !== 'string') {
    return
  }
  // 同名下载覆盖旧条目；主线程每次使用新的随机 ID。
  cleanup(data.downloadId)
  startEntry(data.downloadId, String(data.fileName ?? 'download'), String(data.mime ?? 'application/octet-stream'))
  if (event.source && 'postMessage' in event.source) {
    event.source.postMessage({ type: 'download-ready', downloadId: data.downloadId })
  }
})

self.addEventListener('fetch', (event) => {
  const url = new URL(event.request.url)
  if (url.origin !== self.location.origin || url.pathname !== '/download-sw.js') {
    return
  }
  const downloadId = url.searchParams.get('download')
  if (!downloadId) {
    return
  }
  const entry = downloads.get(downloadId)
  if (!entry) {
    event.respondWith(new Response('download not prepared', { status: 404 }))
    return
  }
  const stream = new ReadableStream({
    start(controller) {
      entry.controller = controller
      for (const bytes of entry.pending) {
        controller.enqueue(bytes)
      }
      entry.pending.length = 0
      if (entry.error) {
        controller.error(entry.error)
        cleanup(downloadId)
        return
      }
      if (entry.finished) {
        controller.close()
        cleanup(downloadId)
      }
    },
    cancel() {
      cleanup(downloadId)
    },
  })
  const encoded = encodeURIComponent(entry.fileName).replaceAll("'", '%27')
  event.respondWith(
    new Response(stream, {
      headers: {
        'Content-Type': entry.mime,
        'Content-Disposition': `attachment; filename*=UTF-8''${encoded}`,
        'Cache-Control': 'no-store',
        'X-Content-Type-Options': 'nosniff',
      },
    }),
  )
})
