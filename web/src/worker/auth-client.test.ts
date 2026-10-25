/**
 * AuthClient 事务式行为测试（Vitest，不加载 WASM）。
 *
 * 用假 CryptoBackend 验证：服务端拒绝时 discard 且不 commit；恢复重置失败不进入解锁；
 * 错误码与 Retry-After 原样透传。
 */
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AuthClient } from './auth-client'
import type { CryptoBackend } from './types'

const ME_RESPONSE = {
  user_id: '550e8400-e29b-41d4-a716-446655440000',
  email: 'user@example.com',
  email_verified: true,
  root_node_id: '550e8400-e29b-41d4-a716-446655440001',
  kdf_params: {
    algorithm: 'argon2id',
    m_cost_kib: 65536,
    t_cost: 3,
    p_cost: 1,
  },
  kdf_salt: '00'.repeat(16),
  wrapped_mk: '01'.repeat(73),
}

const PASSWORD_CHANGE = {
  oldAuthKey: new Uint8Array(32).fill(1),
  newAuthKey: new Uint8Array(32).fill(2),
  newKdfParamsJson:
    '{"algorithm":"argon2id","m_cost_kib":65536,"t_cost":3,"p_cost":1}',
  newKdfSalt: new Uint8Array(16).fill(3),
  newWrappedMk: new Uint8Array(73).fill(4),
}

const RECOVERY_RESET = {
  recoveryAuthKey: new Uint8Array(32).fill(5),
  newAuthKey: new Uint8Array(32).fill(6),
  newKdfParamsJson:
    '{"algorithm":"argon2id","m_cost_kib":65536,"t_cost":3,"p_cost":1}',
  newKdfSalt: new Uint8Array(16).fill(7),
  newWrappedMk: new Uint8Array(73).fill(8),
}

function response(body: unknown, status = 200, headers?: Record<string, string>) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json', ...headers },
  })
}

function fakeBackend(overrides: Partial<CryptoBackend> = {}) {
  const calls = {
    preparePasswordChange: vi.fn(() => PASSWORD_CHANGE),
    commitPasswordChange: vi.fn(),
    prepareRecoveryReset: vi.fn(() => RECOVERY_RESET),
    commitRecoveryReset: vi.fn(),
    discardPending: vi.fn(),
    validateRecoveryKey: vi.fn(),
    lock: vi.fn(),
    isLocked: vi.fn(() => false),
  }
  const backend = {
    ...calls,
    ...overrides,
  } as unknown as CryptoBackend
  return { backend, calls }
}

async function refreshedClient(backend: CryptoBackend) {
  const fetchMock = vi
    .fn()
    .mockResolvedValueOnce(response(ME_RESPONSE))
  vi.stubGlobal('fetch', fetchMock)
  const client = new AuthClient(Promise.resolve(backend))
  await client.refreshSession()
  return { client, fetchMock }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('AuthClient 事务式认证', () => {
  it('服务端拒绝改密时 discard，不 commit，旧密钥保持可用', async () => {
    const { backend, calls } = fakeBackend()
    const { client, fetchMock } = await refreshedClient(backend)
    fetchMock.mockResolvedValueOnce(
      response({ code: 'invalid_credentials', message: 'bad' }, 401),
    )

    await expect(
      client.changePassword('wrong old', 'new password'),
    ).rejects.toMatchObject({ code: 'invalid_credentials' })

    expect(calls.preparePasswordChange).toHaveBeenCalledTimes(1)
    expect(calls.discardPending).toHaveBeenCalledTimes(1)
    expect(calls.commitPasswordChange).not.toHaveBeenCalled()
    expect(calls.isLocked()).toBe(false)
  })

  it('恢复重置失败时 discard，且不进入解锁状态', async () => {
    const { backend, calls } = fakeBackend()
    calls.isLocked.mockReturnValue(true)
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        response({ recovery_wrapped_mk: '01'.repeat(73) }),
      )
      .mockResolvedValueOnce(
        response({ code: 'invalid_recovery_key', message: 'bad' }, 401),
      )
    vi.stubGlobal('fetch', fetchMock)
    const client = new AuthClient(Promise.resolve(backend))

    await expect(
      client.resetWithRecovery(
        'user@example.com',
        '0000-0000',
        'new password',
      ),
    ).rejects.toMatchObject({ code: 'invalid_recovery_key' })

    expect(calls.validateRecoveryKey).toHaveBeenCalledTimes(1)
    expect(calls.prepareRecoveryReset).toHaveBeenCalledTimes(1)
    expect(calls.discardPending).toHaveBeenCalledTimes(1)
    expect(calls.commitRecoveryReset).not.toHaveBeenCalled()
    expect(calls.isLocked()).toBe(true)
  })

  it('429 错误保留 retryAfter', async () => {
    const { backend } = fakeBackend()
    const fetchMock = vi.fn().mockResolvedValueOnce(
      response(
        { code: 'account_temporarily_locked', message: 'locked' },
        429,
        { 'retry-after': '42' },
      ),
    )
    vi.stubGlobal('fetch', fetchMock)
    const client = new AuthClient(Promise.resolve(backend))

    await expect(
      client.login('user@example.com', 'password'),
    ).rejects.toMatchObject({
      code: 'account_temporarily_locked',
      retryAfter: 42,
    })
  })

  it('恢复密钥本地校验失败时不发送任何 HTTP 请求', async () => {
    const { backend, calls } = fakeBackend()
    calls.validateRecoveryKey.mockImplementation(() => {
      throw Object.assign(new Error('bad'), {
        code: 'invalid_recovery_key',
      })
    })
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)
    const client = new AuthClient(Promise.resolve(backend))

    await expect(
      client.resetWithRecovery('user@example.com', 'bad', 'new password'),
    ).rejects.toMatchObject({ code: 'invalid_recovery_key' })
    expect(fetchMock).not.toHaveBeenCalled()
  })
})
