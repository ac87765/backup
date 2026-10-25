/**
 * 密码强度校验（user-accounts spec：长度 ≥ 12 且 zxcvbn 得分 ≥ 3）。
 *
 * 只在主线程执行；弱密码不会触发任何 Worker/HTTP 调用。
 */
import { zxcvbn, zxcvbnOptions } from '@zxcvbn-ts/core'
import { adjacencyGraphs, dictionary } from '@zxcvbn-ts/language-common'

zxcvbnOptions.setOptions({ dictionary, graphs: adjacencyGraphs })

export interface PasswordAssessment {
  ok: boolean
  score: number
  message?: string
}

export function assessPassword(password: string): PasswordAssessment {
  if (password.length < 12) {
    return { ok: false, score: 0, message: '密码至少需要 12 个字符' }
  }
  const result = zxcvbn(password)
  if (result.score < 3) {
    return {
      ok: false,
      score: result.score,
      message: '密码强度不足，请使用更长的随机短语',
    }
  }
  return { ok: true, score: result.score }
}
