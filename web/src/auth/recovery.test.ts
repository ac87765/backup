import { describe, expect, it } from 'vitest'

import {
  pickChallengeIndices,
  recoveryGroups,
  verifyRecoveryFragments,
} from './recovery'

const DISPLAY = 'ABCDE-FGHJK-MNPQR-STVWX-12345-67890-ABCDE-FGHJK-MNPQR-STVWX-12345'

describe('恢复密钥回填确认', () => {
  it('按连字符拆分展示串', () => {
    expect(recoveryGroups(DISPLAY)).toHaveLength(11)
  })

  it('大小写与空白不影响片段匹配', () => {
    const indices = [0, 3, 7]
    const groups = recoveryGroups(DISPLAY)
    const answers = indices.map((index) =>
      (groups[index] ?? '').toLowerCase(),
    )
    expect(verifyRecoveryFragments(DISPLAY, indices, answers)).toBe(true)
  })

  it('错误片段被拒绝', () => {
    const indices = [0, 3, 7]
    const answers = ['AAAAA', 'BBBBB', 'CCCCC']
    expect(verifyRecoveryFragments(DISPLAY, indices, answers)).toBe(false)
  })

  it('随机抽取的组不重复且在范围内', () => {
    const indices = pickChallengeIndices(11, 3)
    expect(new Set(indices).size).toBe(3)
    expect(indices.every((index) => index >= 0 && index < 11)).toBe(true)
  })
})
