/** 受保护路由的登录/解锁回跳状态。 */
export interface AuthRedirectState {
  from?: string
}

/**
 * 只接受站内绝对路径，防止 `from` 被外部输入变成开放重定向。
 * 同时避免回跳到 /login 或 /unlock 形成循环。
 */
export function safeRedirect(from: unknown): string {
  if (
    typeof from === 'string' &&
    from.startsWith('/') &&
    !from.startsWith('//') &&
    from !== '/login' &&
    from !== '/unlock'
  ) {
    return from
  }
  return '/'
}
