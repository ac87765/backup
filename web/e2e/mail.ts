import { readFile, readdir, stat } from 'node:fs/promises'
import path from 'node:path'

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

/** 轮询邮件目录，取出指定邮箱最新验证邮件里的 64 位令牌。 */
export async function waitForVerificationToken(
  mailDir: string,
  email: string,
  timeoutMs = 30_000,
): Promise<string> {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
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
    for (const { file } of withStats) {
      const raw = await readFile(path.join(mailDir, file), 'utf8')
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
  throw new Error(`等待 ${email} 的验证邮件超时`)
}
