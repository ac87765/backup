import { describe, expect, it } from 'vitest'

import { assessPassword } from './password-strength'

describe('密码强度', () => {
  it('拒绝 spec 中的弱密码 password1234', () => {
    expect(assessPassword('password1234').ok).toBe(false)
  })

  it('拒绝长度不足 12 的密码', () => {
    expect(assessPassword('short-pass').ok).toBe(false)
  })

  it('接受足够长的随机短语', () => {
    expect(assessPassword('correct horse battery staple').ok).toBe(true)
  })
})
