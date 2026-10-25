/**
 * Playwright 的真实后端入口：先执行迁移，再用 `server serve` 托管 web/dist 与 API。
 *
 * 邮件写入 E2E_MAIL_DIR（file 模式），测试从 .eml 解码验证令牌。
 * 本脚本只用于 E2E；生产使用镜像内的 server 二进制。
 */
import { spawn, spawnSync } from 'node:child_process'
import { existsSync, mkdirSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'

const webRoot = process.cwd()
const repositoryRoot = path.resolve(webRoot, '..')
const binNames =
  process.platform === 'win32' ? ['server.exe'] : ['server']
const candidates = [
  process.env.E2E_SERVER_BIN,
  ...binNames.flatMap((name) => [
    path.join(repositoryRoot, 'target', 'debug', name),
    path.join(repositoryRoot, 'target', 'release', name),
  ]),
].filter(Boolean)
const serverBinary = candidates.find((candidate) => existsSync(candidate))
if (!serverBinary) {
  console.error(`找不到 server 二进制；候选：${candidates.join(', ')}`)
  process.exit(1)
}

const mailDir =
  process.env.E2E_MAIL_DIR ?? path.join(webRoot, 'e2e-tmp', 'mail')
mkdirSync(mailDir, { recursive: true })

const databaseUrl =
  process.env.E2E_DATABASE_URL ??
  'postgres://backup:backup-dev-password@127.0.0.1:55432/backup'
const storageEndpoint = process.env.E2E_STORAGE_ENDPOINT ?? 'http://127.0.0.1:59000'
const baseUrl = process.env.E2E_BASE_URL ?? 'http://127.0.0.1:4174'

const env = {
  ...process.env,
  BACKUP_BIND_ADDR: new URL(baseUrl).host,
  BACKUP_METRICS_ADDR: '127.0.0.1:0',
  BACKUP_STATIC_DIR: path.join(webRoot, 'dist'),
  BACKUP_DATABASE_URL: databaseUrl,
  BACKUP_STORAGE_KIND: 's3',
  BACKUP_STORAGE_BUCKET: 'backup',
  BACKUP_STORAGE_REGION: 'us-east-1',
  BACKUP_STORAGE_INTERNAL_ENDPOINT: storageEndpoint,
  BACKUP_STORAGE_PUBLIC_ENDPOINT: storageEndpoint,
  BACKUP_STORAGE_ACCESS_KEY_ID: 'backup',
  BACKUP_STORAGE_SECRET_ACCESS_KEY: 'backup-dev-secret',
  BACKUP_STORAGE_ALLOW_HTTP: 'true',
  BACKUP_JOBS_ENABLED: 'true',
  BACKUP_MAIL_MODE: 'file',
  BACKUP_MAIL_DIR: mailDir,
  BACKUP_PUBLIC_APP_URL: baseUrl,
  BACKUP_AUTH_PEPPER_VERSION: '1',
  BACKUP_AUTH_PEPPER: 'e2e-pepper-value-0123456789abcdef',
  BACKUP_AUTH_FAKE_SALT_KEY: 'e2e-fake-salt-key-0123456789abcdef',
  // 只在 E2E 显式要求时降低每用户写限流（用于 1000 小文件 429 退避场景）；
  // 不设置时沿用服务端默认值。
  ...(process.env.E2E_USER_WRITE_RATE_LIMIT
    ? {
        BACKUP_USER_WRITE_RATE_LIMIT_PER_MINUTE:
          process.env.E2E_USER_WRITE_RATE_LIMIT,
      }
    : {}),
}

const migrated = spawnSync(serverBinary, ['migrate'], {
  env,
  stdio: 'inherit',
})
if (migrated.status !== 0) {
  console.error('server migrate 失败')
  process.exit(migrated.status ?? 1)
}

const server = spawn(serverBinary, ['serve'], { env, stdio: 'inherit' })
const stop = (signal) => {
  if (!server.killed) {
    server.kill(signal)
  }
}
process.on('SIGTERM', () => stop('SIGTERM'))
process.on('SIGINT', () => stop('SIGINT'))
server.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal)
  } else {
    process.exit(code ?? 0)
  }
})
