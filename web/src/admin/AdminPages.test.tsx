import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ReactNode } from 'react'

import { ToastProvider } from '../ui'
import { AdminOverviewPage } from './AdminOverviewPage'
import { AdminSettingsPage } from './AdminSettingsPage'
import { AdminUsersPage } from './AdminUsersPage'

const OVERVIEW = {
  pending_users: 3,
  status_counts: { pending: 3, active: 5, suspended: 1, rejected: 0 },
  total_used_bytes: 1024,
  total_reserved_bytes: 0,
  total_quota_bytes: 10240,
  registrations_last_7_days: 4,
  queues: { email_outbox: 2, object_deletions: 1 },
  health: { status: 'ready', database: 'ok', storage: 'ok' },
  version: '0.1.0',
}

const USER = {
  id: '550e8400-e29b-41d4-a716-446655440001',
  email: 'pending@example.com',
  status: 'pending',
  is_admin: false,
  created_at: '2026-01-01T00:00:00Z',
  email_verified_at: null,
  approved_at: null,
  last_active_at: null,
  used_bytes: 0,
  reserved_bytes: 0,
  quota_bytes: 10 * 1024 * 1024 * 1024,
  file_count: 0,
  folder_count: 0,
  version_count: 0,
  active_session_count: 0,
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  })
}

function urlOf(input: RequestInfo | URL): string {
  if (typeof input === 'string') {
    return input
  }
  return input instanceof URL ? input.href : input.url
}

function requestBody(init?: RequestInit): unknown {
  const body = init?.body
  return typeof body === 'string' ? (JSON.parse(body) as unknown) : undefined
}

function renderWithRouter(node: ReactNode) {
  return render(
    <MemoryRouter>
      <ToastProvider>{node}</ToastProvider>
    </MemoryRouter>,
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('管理后台页面', () => {
  it('概览显示醒目的待审核数量与队列积压', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.resolve(json(OVERVIEW))),
    )
    renderWithRouter(<AdminOverviewPage />)

    await waitFor(() => {
      expect(screen.getByTestId('admin-pending-count')).toHaveTextContent('3')
    })
    expect(screen.getByText('发信队列')).toBeInTheDocument()
    expect(screen.getByText('对象删除队列')).toBeInTheDocument()
    expect(screen.getByText('0.1.0')).toBeInTheDocument()
  })

  it('用户页批准时提交所选 50 GB 额度', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const url = urlOf(input)
      if (url.startsWith('/api/v1/admin/overview')) {
        return Promise.resolve(json(OVERVIEW))
      }
      if (url.startsWith('/api/v1/admin/users?') && (init?.method ?? 'GET') === 'GET') {
        return Promise.resolve(json({ items: [USER], next_cursor: null }))
      }
      if (url.endsWith('/approve')) {
        return Promise.resolve(json({ ok: true }))
      }
      return Promise.resolve(json({ code: 'not_found', message: 'not found' }, 404))
    })
    vi.stubGlobal('fetch', fetchMock)
    renderWithRouter(<AdminUsersPage />)

    await waitFor(() => {
      expect(screen.getByTestId('admin-user-row')).toBeInTheDocument()
    })
    await userEvent.click(screen.getByTestId('admin-user-approve'))
    await userEvent.click(screen.getByLabelText('50 GB'))
    await userEvent.click(screen.getByTestId('admin-approve-confirm'))

    await waitFor(() => {
      const approveCall = fetchMock.mock.calls.find(([input]) =>
        urlOf(input).endsWith('/approve'),
      )
      expect(approveCall).toBeTruthy()
      expect(requestBody(approveCall?.[1])).toEqual({
        quota_bytes: 50 * 1024 * 1024 * 1024,
      })
    })
  })

  it('设置页保存后提交注册模式与默认额度', async () => {
    const fetchMock = vi.fn(
      (input: RequestInfo | URL, init?: RequestInit) => {
        if (urlOf(input) === '/api/v1/admin/settings') {
          if (init?.method === 'PUT') {
            return Promise.resolve(json(requestBody(init)))
          }
          return Promise.resolve(
            json({
              registration_mode: 'approval',
              default_quota_bytes: 10 * 1024 * 1024 * 1024,
            }),
          )
        }
        return Promise.resolve(
          json({ code: 'not_found', message: 'not found' }, 404),
        )
      },
    )
    vi.stubGlobal('fetch', fetchMock)
    renderWithRouter(<AdminSettingsPage />)

    await waitFor(() => {
      expect(screen.getByTestId('admin-default-quota')).toHaveValue(10)
    })
    await userEvent.click(screen.getByTestId('admin-registration-mode-open'))
    await userEvent.clear(screen.getByTestId('admin-default-quota'))
    await userEvent.type(screen.getByTestId('admin-default-quota'), '5')
    await userEvent.click(screen.getByTestId('admin-settings-save'))

    await waitFor(() => {
      const putCall = fetchMock.mock.calls.find(
        ([input, init]) =>
          urlOf(input) === '/api/v1/admin/settings' && init?.method === 'PUT',
      )
      expect(putCall).toBeTruthy()
      expect(requestBody(putCall?.[1])).toEqual({
        registration_mode: 'open',
        default_quota_bytes: 5 * 1024 * 1024 * 1024,
      })
    })
  })
})
