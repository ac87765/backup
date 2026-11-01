/**
 * 复制文本：优先使用异步剪贴板 API，旧浏览器退回隐藏 textarea。
 * 返回是否成功，调用方负责给用户可见的成功/失败提示。
 */
export async function copyText(text: string): Promise<boolean> {
  try {
    if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text)
      return true
    }
  } catch {
    // 权限被拒绝或非安全上下文：继续尝试同步回退。
  }

  if (typeof document === 'undefined') {
    return false
  }

  try {
    const textarea = document.createElement('textarea')
    textarea.value = text
    textarea.setAttribute('readonly', '')
    textarea.className = 'fixed left-[-9999px] top-0 opacity-0'
    document.body.appendChild(textarea)
    try {
      textarea.select()
      return (
        typeof document.execCommand === 'function' &&
        document.execCommand('copy')
      )
    } finally {
      textarea.remove()
    }
  } catch {
    return false
  }
}
