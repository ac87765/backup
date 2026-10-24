/**
 * CSP 基础策略的唯一读取入口（与 Rust 服务端共享同一个文件）。
 *
 * 服务端在 `crates/server/src/api/security.rs` 用 `include_str!` 读取
 * `csp/base-policy.txt`，并在启动时把对象存储公网 origin 追加到 `connect-src`；
 * 这里只做纯函数拼接，便于测试两边使用同一份基础策略。
 */
export function buildContentSecurityPolicy(
  base: string,
  connectSrcOrigins: string[] = [],
): string {
  const directives = base
    .split(';')
    .map((directive) => directive.trim())
    .filter((directive) => directive.length > 0)

  const origins = connectSrcOrigins
    .map((origin) => origin.trim().replace(/\/+$/, ''))
    .filter((origin) => origin.length > 0)
    .join(' ')

  if (origins.length === 0) {
    return directives.join('; ')
  }

  return directives
    .map((directive) => {
      const [name] = directive.split(/\s+/)
      return name.toLowerCase() === 'connect-src' ? `${directive} ${origins}` : directive
    })
    .join('; ')
}
