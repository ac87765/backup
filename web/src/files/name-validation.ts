/**
 * 文件/文件夹名称的客户端校验。
 *
 * 服务端只看到名称令牌，无法做这些检查；Worker 与页面共用同一规则，
 * 页面用于即时反馈，Worker 在真正加密/提交前再校验一次。
 */
export const MAX_NAME_BYTES = 255

const encoder = new TextEncoder()

/** 返回错误文案；合法时返回 null。 */
export function validateName(name: string): string | null {
  if (name.length === 0) {
    return '名称不能为空'
  }
  if (name.trim().length === 0) {
    return '名称不能只包含空白字符'
  }
  if (name.includes('/')) {
    return '名称不能包含 "/"'
  }
  if (name.includes('\0')) {
    return '名称不能包含 NUL 字符'
  }
  if (name === '.' || name === '..') {
    return '名称不能是 "." 或 ".."'
  }
  if (encoder.encode(name).length > MAX_NAME_BYTES) {
    return `名称不能超过 ${MAX_NAME_BYTES} 个 UTF-8 字节`
  }
  return null
}
