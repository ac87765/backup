import { CryptoApiError } from '../worker/crypto-api'

export function errorCode(error: unknown): string | null {
  if (error instanceof CryptoApiError) {
    return error.code
  }
  const candidate = error as { code?: unknown } | null
  return candidate && typeof candidate.code === 'string' ? candidate.code : null
}

export function errorMessage(error: unknown): string {
  if (error instanceof CryptoApiError) {
    if (error.code === 'account_temporarily_locked') {
      const seconds = error.retryAfter ?? 0
      return `账号已被临时锁定，请 ${seconds} 秒后重试`
    }
    if (error.code === 'invalid_credentials') {
      return '邮箱或密码错误'
    }
    if (error.code === 'invalid_recovery_key') {
      return '恢复密钥无效，请核对后重试'
    }
    if (error.code === 'invalid_token') {
      return '链接无效或已过期，请重新发送验证邮件'
    }
    if (error.code === 'email_not_verified') {
      return '请先完成邮箱验证'
    }
    if (error.code === 'account_pending') {
      return '账号正在等待管理员审核'
    }
    if (error.code === 'account_suspended') {
      return `账号已被停用：${error.message}`
    }
    if (error.code === 'account_rejected') {
      return `账号未通过审核：${error.message}`
    }
    if (error.code === 'registration_closed') {
      return '暂不开放注册'
    }
    if (error.code === 'reauth_required') {
      return '为确保安全，请重新登录后再继续'
    }
    if (error.code === 'cannot_modify_self') {
      return '不能停用或拒绝自己的账号'
    }
    if (error.code === 'unauthenticated') {
      return '登录已失效，请重新登录'
    }
    return error.message
  }
  if (error instanceof Error) {
    return error.message
  }
  const candidate = error as { message?: unknown } | null
  if (candidate && typeof candidate.message === 'string') {
    return candidate.message
  }
  return '操作失败，请稍后重试'
}
