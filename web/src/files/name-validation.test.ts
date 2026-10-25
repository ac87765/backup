import { describe, expect, it } from 'vitest'

import { validateName } from './name-validation'

describe('文件名称校验', () => {
  it('拒绝空名称与只含空白的名称', () => {
    expect(validateName('')).toBe('名称不能为空')
    expect(validateName('   ')).not.toBeNull()
    expect(validateName('\t\n')).not.toBeNull()
  })

  it('拒绝路径分隔符、NUL 与特殊目录名', () => {
    expect(validateName('a/b')).not.toBeNull()
    expect(validateName('a\0b')).not.toBeNull()
    expect(validateName('.')).not.toBeNull()
    expect(validateName('..')).not.toBeNull()
  })

  it('允许普通名称与 255 字节边界', () => {
    expect(validateName('照片')).toBeNull()
    expect(validateName('a'.repeat(255))).toBeNull()
  })

  it('按 UTF-8 字节数拒绝超过 255 字节的名称', () => {
    expect(validateName('a'.repeat(256))).not.toBeNull()
    // 85 个中文字符 = 255 字节；86 个 = 258 字节。
    expect(validateName('中'.repeat(85))).toBeNull()
    expect(validateName('中'.repeat(86))).not.toBeNull()
  })
})
