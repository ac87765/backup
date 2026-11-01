import { describe, expect, it } from 'vitest'

import { detectSystem } from './system'

const WINDOWS_UA =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36'
const MAC_UA =
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15'
const IPHONE_UA =
  'Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1'
const LINUX_UA =
  'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36'

describe('访问者系统识别', () => {
  it('userAgentData.platform 优先于 userAgent', () => {
    expect(
      detectSystem({ userAgent: WINDOWS_UA, userAgentData: { platform: 'macOS' } }),
    ).toBe('macos')
    expect(
      detectSystem({ userAgent: MAC_UA, userAgentData: { platform: 'Windows' } }),
    ).toBe('windows')
    expect(
      detectSystem({ userAgent: WINDOWS_UA, userAgentData: { platform: 'Linux' } }),
    ).toBe('other')
  })

  it('没有 platform 时回退到 userAgent', () => {
    expect(detectSystem({ userAgent: WINDOWS_UA })).toBe('windows')
    expect(detectSystem({ userAgent: MAC_UA })).toBe('macos')
    expect(detectSystem({ userAgent: IPHONE_UA })).toBe('mobile')
    expect(detectSystem({ userAgent: LINUX_UA })).toBe('other')
  })

  it('iPad / Android / iOS 都识别为手机', () => {
    const IPAD_UA =
      'Mozilla/5.0 (iPad; CPU OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1'
    const ANDROID_UA =
      'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Mobile Safari/537.36'

    expect(detectSystem({ userAgent: IPAD_UA })).toBe('mobile')
    expect(detectSystem({ userAgent: ANDROID_UA })).toBe('mobile')
    expect(
      detectSystem({ userAgent: MAC_UA, userAgentData: { platform: 'iOS' } }),
    ).toBe('mobile')
  })

  it('空 navigator 视为未知系统', () => {
    expect(detectSystem(undefined)).toBe('other')
    expect(detectSystem({})).toBe('other')
  })
})
