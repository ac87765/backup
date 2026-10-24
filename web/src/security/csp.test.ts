import { readFileSync } from 'node:fs'
import path from 'node:path'

import { describe, expect, it } from 'vitest'

import { buildContentSecurityPolicy } from './csp'

/**
 * 与 Rust 服务端共享的 CSP 基础策略文件（仓库根目录 `csp/base-policy.txt`）。
 * npm 脚本都在 `web/` 下执行，因此以 `process.cwd()` 为基准。
 */
const SHARED_CSP_FILE = path.resolve(process.cwd(), '../csp/base-policy.txt')
const sharedBasePolicy = readFileSync(SHARED_CSP_FILE, 'utf8').trim()

describe('CSP 基础策略共享', () => {
  it('Vite 配置读取与服务端相同的共享文件', () => {
    const viteConfig = readFileSync(path.resolve(process.cwd(), 'vite.config.ts'), 'utf8')
    expect(viteConfig).toContain('csp/base-policy.txt')
    expect(viteConfig).toContain('buildContentSecurityPolicy')
  })

  it('共享策略包含严格指令且禁止内联脚本', () => {
    expect(sharedBasePolicy).toContain("default-src 'self'")
    expect(sharedBasePolicy).toContain("script-src 'self' 'wasm-unsafe-eval'")
    expect(sharedBasePolicy).toContain("connect-src 'self'")
    expect(sharedBasePolicy).toContain("frame-ancestors 'none'")
    expect(sharedBasePolicy).not.toContain("'unsafe-inline'")
    // `'wasm-unsafe-eval'` 是允许 WASM 编译所必需的，但不得出现完整的 `'unsafe-eval'`。
    expect(sharedBasePolicy).not.toContain("'unsafe-eval'")
  })

  it('构建结果在无外部 origin 时等于共享基础策略', () => {
    expect(buildContentSecurityPolicy(sharedBasePolicy)).toBe(sharedBasePolicy)
  })

  it('追加对象存储 origin 时只扩展 connect-src', () => {
    const policy = buildContentSecurityPolicy(sharedBasePolicy, [
      'https://bucket.oss-cn-hangzhou.aliyuncs.com/',
    ])
    expect(policy).toContain(
      "connect-src 'self' https://bucket.oss-cn-hangzhou.aliyuncs.com",
    )
    expect(policy.match(/connect-src/g)).toHaveLength(1)
    expect(policy).toContain("script-src 'self' 'wasm-unsafe-eval'")
  })
})
