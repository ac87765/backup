import { afterEach, describe, expect, it } from 'vitest'

import { selectDownloadTarget } from './download-target'

afterEach(() => {
  delete (
    window as unknown as { __BACKUP_FORCE_DOWNLOAD_FALLBACK__?: string }
  ).__BACKUP_FORCE_DOWNLOAD_FALLBACK__
})

describe('下载目标选择', () => {
  it('测试开关可强制走 Blob 降级路径', async () => {
    ;(
      window as unknown as { __BACKUP_FORCE_DOWNLOAD_FALLBACK__?: string }
    ).__BACKUP_FORCE_DOWNLOAD_FALLBACK__ = 'blob'
    const selection = await selectDownloadTarget('a.bin', 'application/octet-stream', 10)
    expect(selection.target).toEqual({ kind: 'blob' })
    selection.cleanup()
  })

  it('Blob 降级路径拒绝超过 2 GiB 的文件', async () => {
    ;(
      window as unknown as { __BACKUP_FORCE_DOWNLOAD_FALLBACK__?: string }
    ).__BACKUP_FORCE_DOWNLOAD_FALLBACK__ = 'blob'
    await expect(
      selectDownloadTarget('huge.bin', 'application/octet-stream', 2 * 1024 ** 3 + 1),
    ).rejects.toMatchObject({ code: 'file_too_large' })
  })
})
