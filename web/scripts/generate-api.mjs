/**
 * 从服务端 OpenAPI 规范生成前端 TypeScript 类型。
 *
 * 服务端是唯一事实来源：`cargo run -p server -- openapi` 把规范输出到 stdout，
 * 本脚本再交给 openapi-typescript 生成 `src/api/schema.d.ts`。
 * CI 会重新执行本脚本并检查生成物无漂移。
 */
import { spawnSync } from 'node:child_process'
import { mkdirSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import openapiTS, { astToString } from 'openapi-typescript'

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url))
const webDirectory = path.resolve(scriptDirectory, '..')
const repositoryRoot = path.resolve(webDirectory, '..')
const outputPath = path.join(webDirectory, 'src', 'api', 'schema.d.ts')

const cargo = process.env.CARGO ?? 'cargo'
const build = spawnSync(
  cargo,
  [
    'run',
    '--quiet',
    '--manifest-path',
    path.join(repositoryRoot, 'crates', 'server', 'Cargo.toml'),
    '--',
    'openapi',
  ],
  {
    encoding: 'utf8',
    // CI 不提供数据库：使用仓库中提交的 .sqlx 离线元数据。
    env: { ...process.env, SQLX_OFFLINE: process.env.SQLX_OFFLINE ?? 'true' },
    maxBuffer: 16 * 1024 * 1024,
  },
)

if (build.error) {
  console.error(`无法执行 ${cargo}: ${build.error.message}`)
  process.exit(1)
}
if (build.status !== 0) {
  process.stderr.write(build.stderr ?? '')
  process.exit(build.status ?? 1)
}

// 提前解析一次，确保拿到的是合法 JSON（而不是构建日志混入 stdout）。
const specification = JSON.parse(build.stdout)

const ast = await openapiTS(specification)
mkdirSync(path.dirname(outputPath), { recursive: true })
writeFileSync(
  outputPath,
  [
    '/**',
    ' * 自动生成，请勿手改。',
    ' * 来源：crates/server/src/openapi.rs（`cargo run -p server -- openapi`）。',
    ' * 重新生成：cd web && npm run generate:api',
    ' */',
    astToString(ast),
  ].join('\n'),
)
console.log(`generated ${path.relative(repositoryRoot, outputPath)}`)
