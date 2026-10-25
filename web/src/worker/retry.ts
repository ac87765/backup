/* eslint-disable @typescript-eslint/only-throw-error -- 跨 Comlink 需要普通对象保留稳定错误码 */
/**
 * 传输重试策略（file-transfer spec「传输重试」、add-folder-backup design D5）。
 *
 * - 网络错误、HTTP 5xx 与 408 使用带抖动的指数退避，单块最多 5 次尝试。
 * - `429 rate_limited` 是**通用**行为：按 `Retry-After` 等待（缺失时 5 秒），
 *   加 0–20% CSPRNG 抖动后重试同一请求，最多重试 8 次；等待期间可通过
 *   `onRetry` 向界面报告“服务器繁忙”，不把文件标记为失败。
 * - 抖动使用 CSPRNG（AGENTS.md），不依赖 Math.random。
 * - 等待可被 `isCancelled` 打断（用户取消文件夹任务时及时释放）。
 */

export interface TransferErrorPayload {
  code: string
  message: string
  status?: number
  retryAfter?: number
  missingChunks?: number[]
}

/** 429 最多重试次数（不含第一次请求）。 */
export const RATE_LIMIT_MAX_RETRIES = 8

/** 429 没有 `Retry-After` 时的默认等待秒数。 */
export const RATE_LIMIT_DEFAULT_RETRY_AFTER_SECONDS = 5

/** 429 等待抖动上限（0–20%，只向上加）。 */
export const RATE_LIMIT_JITTER_RATIO = 0.2

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

export function isRateLimited(error: TransferErrorPayload): boolean {
  return error.code === 'rate_limited' || error.status === 429
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

/** 429 等待时间：`Retry-After`（秒，缺失按 5 秒）加 0–20% 抖动。 */
export function rateLimitDelay(
  retryAfterSeconds: number | undefined,
  random: () => number = randomUnit,
): number {
  const seconds =
    retryAfterSeconds !== undefined &&
    Number.isFinite(retryAfterSeconds) &&
    retryAfterSeconds > 0
      ? retryAfterSeconds
      : RATE_LIMIT_DEFAULT_RETRY_AFTER_SECONDS
  return Math.round(seconds * 1000 * (1 + random() * RATE_LIMIT_JITTER_RATIO))
}

export function sleep(milliseconds: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, milliseconds)
  })
}

/**
 * 可取消的等待：每 50ms 检查一次取消标记，避免用户取消后还睡满 Retry-After。
 */
export async function sleepWithCancellation(
  milliseconds: number,
  isCancelled: (() => boolean) | undefined,
  sleepImpl: (ms: number) => Promise<void> = sleep,
): Promise<void> {
  if (!isCancelled) {
    await sleepImpl(milliseconds)
    return
  }
  if (isCancelled()) {
    throw transferError('cancelled', '传输已取消')
  }
  const deadline = Date.now() + milliseconds
  for (;;) {
    const remaining = deadline - Date.now()
    if (remaining <= 0) {
      return
    }
    await sleepImpl(Math.min(50, remaining))
    if (isCancelled()) {
      throw transferError('cancelled', '传输已取消')
    }
  }
}

export interface RetryOptions {
  /** 非 429 请求的最大尝试次数（含第一次）。 */
  maxAttempts?: number
  baseDelayMs?: number
  maxDelayMs?: number
  /** 429 最大重试次数（不含第一次），默认 8。 */
  maxRateLimitRetries?: number
  /** 每次等待前回调；参数是本次等待的序号、错误与毫秒数。 */
  onRetry?: (attempt: number, error: TransferErrorPayload, delayMs: number) => void
  /** 返回 true 时立即中止等待并抛出 `cancelled`。 */
  isCancelled?: () => boolean
  /** 测试注入的等待实现。 */
  sleepImpl?: (milliseconds: number) => Promise<void>
}

/**
 * 执行 `operation`，可重试错误按策略退避。
 *
 * - 429：最多重试 `maxRateLimitRetries` 次，等待 `Retry-After` + 抖动；
 * - 其他可重试错误：最多尝试 `maxAttempts` 次，指数退避。
 */
export async function withRetry<T>(
  operation: () => Promise<T>,
  options: RetryOptions = {},
): Promise<T> {
  const maxAttempts = options.maxAttempts ?? 5
  const maxRateLimitRetries = options.maxRateLimitRetries ?? RATE_LIMIT_MAX_RETRIES
  const baseDelayMs = options.baseDelayMs ?? 500
  const maxDelayMs = options.maxDelayMs ?? 8_000
  const sleepImpl = options.sleepImpl ?? sleep
  let attempt = 0
  let rateLimitRetries = 0
  for (;;) {
    if (options.isCancelled?.()) {
      throw transferError('cancelled', '传输已取消')
    }
    try {
      return await operation()
    } catch (error) {
      const payload = error as TransferErrorPayload
      if (options.isCancelled?.()) {
        throw transferError('cancelled', '传输已取消')
      }
      if (!isRetryable(payload)) {
        throw error
      }
      let delayMs: number
      let retryNumber: number
      if (isRateLimited(payload)) {
        if (rateLimitRetries >= maxRateLimitRetries) {
          throw error
        }
        rateLimitRetries += 1
        retryNumber = rateLimitRetries
        delayMs = rateLimitDelay(payload.retryAfter)
      } else {
        attempt += 1
        if (attempt >= maxAttempts) {
          throw error
        }
        retryNumber = attempt
        delayMs = backoffDelay(attempt - 1, baseDelayMs, maxDelayMs)
      }
      options.onRetry?.(retryNumber, payload, delayMs)
      await sleepWithCancellation(delayMs, options.isCancelled, sleepImpl)
    }
  }
}
