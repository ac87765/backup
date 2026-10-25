import { describe, expect, it } from 'vitest'

import { safeRedirect } from './navigation'

describe('受保护路由回跳', () => {
  it('保留站内受保护路径', () => {
    expect(safeRedirect('/change-password')).toBe('/change-password')
    expect(safeRedirect('/sessions?tab=all')).toBe('/sessions?tab=all')
  })

  it('拒绝开放重定向与登录/解锁循环', () => {
    expect(safeRedirect('//evil.example')).toBe('/')
    expect(safeRedirect('https://evil.example')).toBe('/')
    expect(safeRedirect('/login')).toBe('/')
    expect(safeRedirect('/unlock')).toBe('/')
    expect(safeRedirect(undefined)).toBe('/')
  })
})
