/** 恢复密钥回填确认的 UI 辅助函数（密钥本身不落盘、不写日志）。 */

export function recoveryGroups(display: string): string[] {
  return display.split('-').filter(Boolean)
}

/** 用 CSPRNG 随机抽取若干组（不重复）。 */
export function pickChallengeIndices(
  groupCount: number,
  count: number,
): number[] {
  const picks = Math.min(count, groupCount)
  const indices = new Set<number>()
  while (indices.size < picks) {
    const random = crypto.getRandomValues(new Uint32Array(1))[0]
    indices.add(random % groupCount)
  }
  return [...indices].sort((left, right) => left - right)
}

export function normalizeRecoveryGroup(value: string): string {
  return value.replace(/[\s-]/g, '').toUpperCase()
}

/** 校验用户回填的片段是否与展示串一致。 */
export function verifyRecoveryFragments(
  display: string,
  indices: number[],
  answers: string[],
): boolean {
  const groups = recoveryGroups(display)
  if (indices.length !== answers.length) {
    return false
  }
  return indices.every((index, position) => {
    const expected = groups[index]
    return (
      expected !== undefined &&
      normalizeRecoveryGroup(answers[position] ?? '') ===
        normalizeRecoveryGroup(expected)
    )
  })
}
