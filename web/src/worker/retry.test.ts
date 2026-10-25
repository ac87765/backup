/* eslint-disable @typescript-eslint/only-throw-error -- 重试层用普通对象错误载荷 */
import { describe, expect, it, vi } from 'vitest'

import {
  RATE_LIMIT_MAX_RETRIES,
  rateLimitDelay,
  transferError,
  withRetry,
} from './retry'

describe('429 限流退避（design D5）', () => {
  it('缺失 Retry-After 时按 5 秒并加 0–20% 抖动', async () => {
    const delays: number[] = []
    const operation = vi
      .fn<() => Promise<string>>()
      .mockRejectedValueOnce(
        transferError('rate_limited', '请求过于频繁', { status: 429 }),
      )
      .mockResolvedValueOnce('ok')

    const result = await withRetry(operation, {
      onRetry: (_attempt, _error, delayMs) => delays.push(delayMs),
      sleepImpl: () => Promise.resolve(),
    })

    expect(result).toBe('ok')
    expect(operation).toHaveBeenCalledTimes(2)
    expect(delays).toHaveLength(1)
    expect(delays[0]).toBeGreaterThanOrEqual(5_000)
    expect(delays[0]).toBeLessThanOrEqual(6_000)

    // 抖动只会向上加，且不超过 20%。
    const base = 2_000
    for (let index = 0; index < 32; index += 1) {
      const delay = rateLimitDelay(2)
      expect(delay).toBeGreaterThanOrEqual(base)
      expect(delay).toBeLessThanOrEqual(base * 1.2)
    }
    expect(rateLimitDelay(undefined)).toBeGreaterThanOrEqual(5_000)
  })

  it('超过最多 8 次重试后抛出原始 429 错误', async () => {
    const operation = vi.fn<() => Promise<never>>().mockRejectedValue(
      transferError('rate_limited', '请求过于频繁', {
        status: 429,
        retryAfter: 1,
      }),
    )
    const retries: number[] = []

    await expect(
      withRetry(operation, {
        onRetry: (attempt) => retries.push(attempt),
        sleepImpl: () => Promise.resolve(),
      }),
    ).rejects.toMatchObject({ code: 'rate_limited', status: 429 })

    // 第一次请求 + 8 次重试。
    expect(operation).toHaveBeenCalledTimes(RATE_LIMIT_MAX_RETRIES + 1)
    expect(retries).toEqual([1, 2, 3, 4, 5, 6, 7, 8])
  })

  it('等待期间取消会立即中止且不再重试', async () => {
    let cancelled = false
    const operation = vi
      .fn<() => Promise<never>>()
      .mockRejectedValue(
        transferError('rate_limited', '请求过于频繁', { status: 429 }),
      )

    await expect(
      withRetry(operation, {
        isCancelled: () => cancelled,
        sleepImpl: () => {
          cancelled = true
          return Promise.resolve()
        },
      }),
    ).rejects.toMatchObject({ code: 'cancelled' })

    expect(operation).toHaveBeenCalledTimes(1)
  })

  it('非 429 错误不消耗限流重试次数', async () => {
    let calls = 0
    const operation = vi.fn(() => {
      calls += 1
      if (calls === 1) {
        throw transferError('network_error', '网络中断')
      }
      return Promise.resolve('ok')
    })

    await expect(
      withRetry(operation, {
        sleepImpl: () => Promise.resolve(),
        onRetry: (_attempt, error) => {
          expect(error.code).toBe('network_error')
        },
      }),
    ).resolves.toBe('ok')
  })
})
