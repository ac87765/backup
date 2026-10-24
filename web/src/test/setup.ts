import '@testing-library/jest-dom/vitest'
import { cleanup } from '@testing-library/react'
import { afterEach } from 'vitest'

// 未开启 vitest globals，因此显式注册 RTL 的清理逻辑，避免用例之间互相污染。
afterEach(() => {
  cleanup()
})
