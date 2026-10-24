/**
 * 在每个测试/构建前确保 WASM 绑定存在。
 *
 * - 本地开发（默认）：每次调用 wasm-pack 重新构建，行为与之前的 pre* 钩子一致。
 * - CI：设置 SKIP_WASM_BUILD=1，且 rust job 已通过 artifact 提供绑定；此时只检查产物，
 *   不再重复构建（避免 pretest/prebuild 各触发一次 wasm 编译）。
 * - 如果设置了 SKIP_WASM_BUILD=1 但产物不存在，仍然会构建：宁可失败也不要静默使用旧产物。
 */
import { spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url))
const webDirectory = path.resolve(scriptDirectory, '..')
const target = path.join(
  webDirectory,
  'src',
  'generated',
  'crypto-wasm',
  'crypto_wasm_bg.wasm',
)

if (process.env.SKIP_WASM_BUILD === '1' && existsSync(target)) {
  process.exit(0)
}

const result = spawnSync(
  'wasm-pack',
  [
    'build',
    '../crates/crypto-wasm',
    '--target',
    'web',
    '--release',
    '--no-opt',
    '--out-dir',
    '../../web/src/generated/crypto-wasm',
    '--out-name',
    'crypto_wasm',
  ],
  {
    // 与 package.json 的 build:wasm 完全一致（wasm-pack 的相对路径以 crate 为基准）。
    cwd: webDirectory,
    stdio: 'inherit',
  },
)

if (result.error) {
  console.error(`无法执行 wasm-pack: ${result.error.message}`)
  console.error('请先安装 wasm-pack（cargo install wasm-pack --locked），或设置 SKIP_WASM_BUILD=1 使用已有产物。')
  process.exit(1)
}
process.exit(result.status ?? 1)
