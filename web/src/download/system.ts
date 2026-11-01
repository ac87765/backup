/**
 * 下载页的访问者系统识别。
 *
 * 优先级与 add-client-releases D6 一致：`navigator.userAgentData.platform`
 * 优先，其次 `navigator.userAgent`。识别只用于突出对应平台的下载按钮，
 * 不影响页面上两个平台都可下载。
 */

export type VisitorSystem = 'windows' | 'macos' | 'mobile' | 'other'

export interface NavigatorLike {
  userAgent?: string
  userAgentData?: { platform?: string }
}

function browserNavigator(): NavigatorLike | undefined {
  return typeof navigator === 'undefined' ? undefined : navigator
}

export function detectSystem(
  nav: NavigatorLike | undefined = browserNavigator(),
): VisitorSystem {
  if (!nav) {
    return 'other'
  }

  const dataPlatform = nav.userAgentData?.platform?.trim().toLowerCase() ?? ''
  if (dataPlatform.length > 0) {
    if (dataPlatform.includes('windows')) {
      return 'windows'
    }
    if (dataPlatform.includes('mac')) {
      return 'macos'
    }
    if (
      dataPlatform.includes('android') ||
      dataPlatform.includes('ios') ||
      dataPlatform.includes('iphone') ||
      dataPlatform.includes('ipad')
    ) {
      return 'mobile'
    }
    return 'other'
  }

  const userAgent = nav.userAgent ?? ''
  if (/iPhone|iPad|iPod|Android/i.test(userAgent)) {
    return 'mobile'
  }
  if (/Windows/i.test(userAgent)) {
    return 'windows'
  }
  if (/Macintosh|Mac OS X/i.test(userAgent)) {
    return 'macos'
  }
  return 'other'
}
