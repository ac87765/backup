import { existsSync } from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import process from 'node:process'

/**
 * E2E 中通过命令行授予管理员（add-admin-console task 5.1）。
 *
 * - 本地 E2E：使用仓库 target/debug|release/server 二进制，只设置
 *   `BACKUP_DATABASE_URL`（管理命令只连数据库）。
 * - 容器 E2E：通过 `docker compose exec server server admin grant <email>`；
 *   设置 E2E_MAIL_CONTAINER_SERVICE=server 即启用。
 * - 其他环境可通过 E2E_ADMIN_GRANT_COMMAND 提供命令模板（{email} 占位）。
 */
export function grantAdmin(email: string): void {
  if (process.env.E2E_ADMIN_GRANT_COMMAND) {
    const template = process.env.E2E_ADMIN_GRANT_COMMAND
    const command = template.replaceAll('{email}', email)
    const result = spawnSync(command, { shell: true, encoding: 'utf8' })
    assertSuccess(result, `E2E_ADMIN_GRANT_COMMAND 授予 ${email} 管理员失败`)
    return
  }

  if (process.env.E2E_MAIL_CONTAINER_SERVICE) {
    const composeFile =
      process.env.E2E_COMPOSE_FILE ??
      path.resolve(process.cwd(), '..', 'deploy', 'docker-compose.e2e.yml')
    const result = spawnSync(
      'docker',
      [
        'compose',
        '-f',
        composeFile,
        'exec',
        '-T',
        'server',
        '/usr/local/bin/server',
        'admin',
        'grant',
        email,
      ],
      { encoding: 'utf8' },
    )
    assertSuccess(result, `容器内授予 ${email} 管理员失败`)
    return
  }

  const repositoryRoot = path.resolve(process.cwd(), '..')
  const names = process.platform === 'win32' ? ['server.exe'] : ['server']
  const candidates = [
    process.env.E2E_SERVER_BIN,
    ...names.flatMap((name) => [
      path.join(repositoryRoot, 'target', 'debug', name),
      path.join(repositoryRoot, 'target', 'release', name),
    ]),
  ].filter(Boolean)
  const binary = candidates.find((candidate) => existsSync(candidate as string))
  if (!binary) {
    throw new Error(`找不到 server 二进制；候选：${candidates.join(', ')}`)
  }
  const databaseUrl =
    process.env.E2E_DATABASE_URL ??
    'postgres://backup:backup-dev-password@127.0.0.1:55432/backup'
  const result = spawnSync(binary, ['admin', 'grant', email], {
    encoding: 'utf8',
    env: { ...process.env, BACKUP_DATABASE_URL: databaseUrl },
  })
  assertSuccess(result, `授予 ${email} 管理员失败`)
}

function assertSuccess(
  result: ReturnType<typeof spawnSync>,
  message: string,
): void {
  if (result.status !== 0) {
    throw new Error(
      `${message}（exit ${result.status ?? 'null'}）：${String(result.stderr ?? '')}`,
    )
  }
}
