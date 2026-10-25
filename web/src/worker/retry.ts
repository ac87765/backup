/**
 * 传输重试策略（file-transfer spec「传输重试」）。
 *
 * 网络错误、HTTP 5xx 与 429 使用带抖动的指数退避，单块最多 5 次。
 * 抖动使用 CSPRNG（AGENTS.md），不依赖 Math.random。
 */

export interface TransferErrorPayload {
  code: string
  message: string
  status?: number
  retryAfter?: number
  missingChunks?: number[]
}

export function transferError(
  code: string,
  message: string,
  extra?: Partial<Pick<TransferErrorPayload, 'status' | 'retryAfter' | 'missingChunks'>>,
): TransferErrorPayload {
  return {
    code,
    message,
    ...(extra?.status === undefined ? {} : { status: extra.status }),
    ...(extra?.retryAfter === undefined ? {} : { retryAfter: extra.retryAfter }),
    ...(extra?.missingChunks === undefined
      ? {}
      : { missingChunks: extra.missingChunks }),
  }
}

export function isRetryable(error: TransferErrorPayload): boolean {
  if (error.code === 'network_error') {
    return true
  }
  if (error.status === undefined) {
    return false
  }
  return error.status >= 500 || error.status === 429 || error.status === 408
}

/** CSPRNG 抖动（0–1）。 */
function randomUnit(): number {
  const value = crypto.getRandomValues(new Uint32Array(1))[0] ?? 0
  return value / 0x1_0000_0000
}

/** 第 `attempt` 次失败后的等待时间：base * 2^attempt（上限 max），再加 ±25% 抖动。 */
export function backoffDelay(
  attempt: number,
  baseDelayMs = 500,
  maxDelayMs = 8_000,
): number {
  const exponential = Math.min(maxDelayMs, baseDelayMs * 2 ** attempt)
  const jitter = 0.75 + randomUnit() * 0.5
  return Math.round(exponential * jitter)
}

export function sleep(milliseconds: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, milliseconds)
  })
}

export interface RetryOptions {
  maxAttempts?: number
  baseDelayMs?: number
  maxDelayMs?: number
  onRetry?: (attempt: number, error: TransferErrorPayload, delayMs: number) => void
}

/**
 * 执行 `operation`，可重试错误最多尝试 `maxAttempts` 次（包含第一次）。
 * 429 的 Retry-After 会覆盖退避时间（取较大者）。
 */
export async function withRetry<T>(
  operation: () => Promise<T>,
  options: RetryOptions = {},
): Promise<T> {
  const maxAttempts = options.maxAttempts ?? 5
  const baseDelayMs = options.baseDelayMs ?? 500
  const maxDelayMs = options.maxDelayMs ?? 8_000
  let attempt = 0
  for (;;) {
    try {
      return await operation()
    } catch (error) {
      const payload = error as TransferErrorPayload
      attempt += 1
      if (attempt >= maxAttempts || !isRetryable(payload)) {
        throw error
      }
      const retryAfterMs =
        payload.retryAfter === undefined ? 0 : payload.retryAfter * 1000
      const delayMs = Math.max(
        retryAfterMs,
        backoffDelay(attempt - 1, baseDelayMs, maxDelayMs),
      )
      options.onRetry?.(attempt, payload, delayMs)
      await sleep(delayMs)
    }
  }
}
