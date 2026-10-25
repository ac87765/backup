import type { components } from '../api/schema'

type AdminOverview = components['schemas']['AdminOverview']
type AdminUserSummary = components['schemas']['AdminUserSummary']
type AdminUserListResponse = components['schemas']['AdminUserListResponse']
type AdminUserDetailResponse = components['schemas']['AdminUserDetailResponse']
type AdminSettingsResponse = components['schemas']['AdminSettingsResponse']
type AdminAuditResponse = components['schemas']['AdminAuditResponse']
type UpdateQuotaResponse = components['schemas']['UpdateQuotaResponse']

export type { AdminAuditResponse, AdminOverview, AdminSettingsResponse, AdminUserDetailResponse, AdminUserListResponse, AdminUserSummary, UpdateQuotaResponse }

export class AdminApiError extends Error {
  readonly code: string
  readonly status: number

  constructor(code: string, message: string, status: number) {
    super(message)
    this.name = 'AdminApiError'
    this.code = code
    this.status = status
  }
}

async function request<T>(
  path: string,
  init: { method: string; body?: unknown } = { method: 'GET' },
): Promise<T> {
  let response: Response
  try {
    response = await fetch(path, {
      method: init.method,
      credentials: 'same-origin',
      headers: {
        'X-Requested-With': 'backup-web',
        ...(init.body !== undefined ? { 'Content-Type': 'application/json' } : {}),
      },
      body: init.body !== undefined ? JSON.stringify(init.body) : undefined,
    })
  } catch {
    throw new AdminApiError('network_error', '网络请求失败，请稍后重试', 0)
  }
  const text = await response.text()
  let payload: unknown
  try {
    payload = text ? (JSON.parse(text) as unknown) : undefined
  } catch {
    payload = undefined
  }
  if (!response.ok) {
    const error = payload as { code?: unknown; message?: unknown } | undefined
    throw new AdminApiError(
      typeof error?.code === 'string' ? error.code : 'http_error',
      typeof error?.message === 'string' ? error.message : '请求失败',
      response.status,
    )
  }
  return payload as T
}

function queryString(params: Record<string, string | number | undefined>): string {
  const search = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== '') {
      search.set(key, String(value))
    }
  }
  const text = search.toString()
  return text ? `?${text}` : ''
}

export const adminApi = {
  getOverview: () => request<AdminOverview>('/api/v1/admin/overview'),
  listUsers: (params: {
    status?: string
    q?: string
    cursor?: string
    limit?: number
  }) =>
    request<AdminUserListResponse>(
      `/api/v1/admin/users${queryString(params)}`,
    ),
  getUser: (userId: string) =>
    request<AdminUserDetailResponse>(
      `/api/v1/admin/users/${encodeURIComponent(userId)}`,
    ),
  approve: (userId: string, quotaBytes?: number) =>
    request<unknown>(
      `/api/v1/admin/users/${encodeURIComponent(userId)}/approve`,
      { method: 'POST', body: quotaBytes === undefined ? {} : { quota_bytes: quotaBytes } },
    ),
  reject: (userId: string, reason: string) =>
    request<unknown>(
      `/api/v1/admin/users/${encodeURIComponent(userId)}/reject`,
      { method: 'POST', body: { reason } },
    ),
  suspend: (userId: string, reason: string) =>
    request<unknown>(
      `/api/v1/admin/users/${encodeURIComponent(userId)}/suspend`,
      { method: 'POST', body: { reason } },
    ),
  reactivate: (userId: string) =>
    request<unknown>(
      `/api/v1/admin/users/${encodeURIComponent(userId)}/reactivate`,
      { method: 'POST', body: {} },
    ),
  updateQuota: (userId: string, quotaBytes: number) =>
    request<UpdateQuotaResponse>(
      `/api/v1/admin/users/${encodeURIComponent(userId)}/quota`,
      { method: 'PUT', body: { quota_bytes: quotaBytes } },
    ),
  revokeSessions: (userId: string) =>
    request<unknown>(
      `/api/v1/admin/users/${encodeURIComponent(userId)}/revoke-sessions`,
      { method: 'POST', body: {} },
    ),
  getSettings: () =>
    request<AdminSettingsResponse>('/api/v1/admin/settings'),
  updateSettings: (registrationMode: string, defaultQuotaBytes: number) =>
    request<AdminSettingsResponse>('/api/v1/admin/settings', {
      method: 'PUT',
      body: {
        registration_mode: registrationMode,
        default_quota_bytes: defaultQuotaBytes,
      },
    }),
  listAudit: (params: {
    cursor?: string
    targetUserId?: string
    limit?: number
  }) =>
    request<AdminAuditResponse>(
      `/api/v1/admin/audit${queryString({
        cursor: params.cursor,
        target_user_id: params.targetUserId,
        limit: params.limit,
      })}`,
    ),
}
