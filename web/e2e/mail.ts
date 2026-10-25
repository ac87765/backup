import { execFile } from 'node:child_process'
import { readFile, readdir, stat } from 'node:fs/promises'
import path from 'node:path'
import { promisify } from 'node:util'

const execFileAsync = promisify(execFile)

/** 从原始 .eml 文本中取出解码后的正文（base64 / quoted-printable）。 */
export function decodeMailBody(raw: string): string {
  const separator = raw.includes('\r\n\r\n') ? '\r\n\r\n' : '\n\n'
  const [headers, body = ''] = raw.split(separator, 2)
  const encoding = headers
    .split(/\r?\n/)
    .map((line) => line.split(/:(.*)/s))
    .find(([name]) => name?.trim().toLowerCase() === 'content-transfer-encoding')?.[1]
    ?.trim()
    .toLowerCase()

  if (encoding === 'base64') {
    return Buffer.from(body.replace(/\s+/g, ''), 'base64').toString('utf8')
  }
  if (encoding === 'quoted-printable') {
    return decodeQuotedPrintable(body)
  }
  return body
}

function decodeQuotedPrintable(input: string): string {
  const bytes: number[] = []
  for (let index = 0; index < input.length; index += 1) {
    if (input[index] === '=') {
      if (input[index + 1] === '\r' && input[index + 2] === '\n') {
        index += 2
        continue
      }
      if (input[index + 1] === '\n') {
        index += 1
        continue
      }
      const hex = input.slice(index + 1, index + 3)
      if (/^[0-9a-fA-F]{2}$/.test(hex)) {
        bytes.push(Number.parseInt(hex, 16))
        index += 2
        continue
      }
    }
    bytes.push(input.charCodeAt(index))
  }
  return Buffer.from(bytes).toString('utf8')
}

/**
 * 读取容器内 file 模式写出的全部 .eml。
 *
 * 容器化 E2E（deploy/docker-compose.e2e.yml）中邮件写在 server 容器的 named volume，
 * 宿主机无法直接访问；通过 `docker compose exec` 读取。只在显式设置
 * E2E_MAIL_CONTAINER_SERVICE 时启用，普通本地 E2E 仍然直接读目录。
 */
async function readContainerMail(): Promise<string[]> {
  const service = process.env.E2E_MAIL_CONTAINER_SERVICE
  const composeFile = process.env.E2E_COMPOSE_FILE
  if (!service) {
    return []
  }
  if (!composeFile) {
    throw new Error(
      'E2E_MAIL_CONTAINER_SERVICE 需要同时设置 E2E_COMPOSE_FILE（docker-compose.e2e.yml 路径）',
    )
  }
  const directory =
    process.env.E2E_MAIL_CONTAINER_DIR ?? '/var/lib/backup/mail'
  const script = `for file in "${directory}"/*.eml; do [ -e "$file" ] || continue; printf '@@FILE@@%s\\n' "$file"; cat "$file"; done`
  const { stdout } = await execFileAsync(
    'docker',
    [
      'compose',
      '-f',
      composeFile,
      'exec',
      '-T',
      service,
      'sh',
      '-c',
      script,
    ],
    { maxBuffer: 16 * 1024 * 1024 },
  )
  return stdout
    .split('@@FILE@@')
    .slice(1)
    .map((part) => part.slice(part.indexOf('\n') + 1))
}

/** 轮询邮件来源，取出指定邮箱最新验证邮件里的 64 位令牌。 */
export async function waitForVerificationToken(
  mailDir: string,
  email: string,
  timeoutMs = 30_000,
): Promise<string> {
  const deadline = Date.now() + timeoutMs
  const containerService = Boolean(process.env.E2E_MAIL_CONTAINER_SERVICE)
  let lastReadError: unknown = null
  while (Date.now() < deadline) {
    const rawMails = containerService
      ? await readContainerMail().catch((error: unknown) => {
          lastReadError = error
          return [] as string[]
        })
      : await readLocalMail(mailDir)
    for (const raw of rawMails) {
      if (!raw.includes(email)) {
        continue
      }
      const body = decodeMailBody(raw)
      const match = body.match(/#token=([0-9a-f]{64})/)
      if (match) {
        return match[1]
      }
    }
    await new Promise((resolve) => setTimeout(resolve, 300))
  }
  const readErrorDetail =
    lastReadError instanceof Error
      ? lastReadError.message
      : lastReadError
        ? '未知错误'
        : ''
  throw new Error(
    `等待 ${email} 的验证邮件超时（来源：${containerService ? 'server 容器' : mailDir}）` +
      (readErrorDetail ? `；最近一次读取错误：${readErrorDetail}` : ''),
  )
}

async function readLocalMail(mailDir: string): Promise<string[]> {
  const files = await readdir(mailDir).catch(() => [] as string[])
  const withStats = await Promise.all(
    files
      .filter((file) => file.endsWith('.eml'))
      .map(async (file) => ({
        file,
        mtime: (await stat(path.join(mailDir, file))).mtimeMs,
      })),
  )
  withStats.sort((left, right) => right.mtime - left.mtime)
  return Promise.all(
    withStats.map(({ file }) => readFile(path.join(mailDir, file), 'utf8')),
  )
}

