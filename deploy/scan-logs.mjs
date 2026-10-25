#!/usr/bin/env node
/**
 * 服务端日志敏感信息扫描（task 9.5）。
 *
 * 输入：
 *   --logs <file>        server 容器日志文件（可重复；默认 deploy/e2e-out/server.log）
 *   --manifest <file>    由 web/e2e/full-journey.spec.ts 写出的敏感值清单
 *
 * 断言日志中不出现：
 *   - 清单中的会话 Cookie 值、邮件令牌、明文文件名/文件夹名/内容片段、测试密码；
 *   - 清单中记录的预签名签名参数值；
 *   - 预签名 URL 的签名参数名（X-Amz-* / Signature / OSSAccessKeyId / x-oss-* 等）；
 *   - auth_key、包裹密钥、恢复密钥等密钥字段名；
 *   - 64 位十六进制令牌/密钥（会话令牌、auth_key、邮件令牌的通用形态）；
 *   - URL 片段中的 #token= 邮件令牌。
 *
 * 只打印违规的行号与类别，绝不打印匹配到的值或整行日志，避免把敏感信息二次泄漏到 CI。
 */
import { readFile } from 'node:fs/promises'
import path from 'node:path'
import process from 'node:process'

function usage() {
  console.error(
    '用法：node deploy/scan-logs.mjs --logs <server.log> [--logs ...] --manifest <sensitive.json>',
  )
}

function parseArguments(argv) {
  const logs = []
  let manifest = null
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index]
    if (argument === '--logs') {
      const value = argv[index + 1]
      if (!value) throw new Error('--logs 缺少路径')
      logs.push(value)
      index += 1
    } else if (argument === '--manifest') {
      const value = argv[index + 1]
      if (!value) throw new Error('--manifest 缺少路径')
      manifest = value
      index += 1
    } else if (argument === '--help' || argument === '-h') {
      usage()
      process.exit(0)
    } else {
      throw new Error(`未知参数：${argument}`)
    }
  }
  if (logs.length === 0) {
    logs.push('deploy/e2e-out/server.log')
  }
  if (!manifest) {
    manifest = 'web/e2e-tmp/container-sensitive.json'
  }
  return { logs, manifest }
}

const FORBIDDEN_PATTERNS = [
  {
    name: '预签名 URL 签名参数名',
    regex:
      /(?:X-Amz-Signature|X-Amz-Credential|X-Amz-Security-Token|OSSAccessKeyId|x-oss-signature|x-oss-credential|x-oss-security-token|Signature)\s*[=:]/i,
  },
  {
    name: '密钥字段名（auth_key/rk_auth/wrapped_mk/recovery_wrapped_mk）',
    regex: /\b(?:auth_key|rk_auth|wrapped_mk|recovery_wrapped_mk)\b/i,
  },
  {
    name: '恢复密钥/密码字段',
    regex: /"(?:recovery_key|recoveryKey|password)"\s*:/,
  },
  {
    name: '邮件令牌 URL 片段',
    regex: /#token=/i,
  },
  {
    name: '64 位十六进制令牌或密钥',
    regex: /\b[0-9a-fA-F]{64}\b/,
  },
]

async function main() {
  const { logs, manifest } = parseArguments(process.argv.slice(2))
  let sensitive
  try {
    sensitive = JSON.parse(await readFile(manifest, 'utf8'))
  } catch (error) {
    console.error(`无法读取敏感值清单 ${manifest}：${error.message}`)
    console.error('请先运行容器化全链路 E2E；测试结束时会写出该清单。')
    process.exit(2)
  }
  if (!Array.isArray(sensitive.literals) || !Array.isArray(sensitive.presignedUrls)) {
    console.error(`敏感值清单格式不正确：${manifest}`)
    process.exit(2)
  }

  let combined = ''
  for (const file of logs) {
    let content
    try {
      content = await readFile(file, 'utf8')
    } catch (error) {
      console.error(`无法读取日志 ${file}：${error.message}`)
      process.exit(2)
    }
    if (!content.trim()) {
      console.error(`日志 ${file} 为空；无法证明敏感信息未被记录`)
      process.exit(1)
    }
    combined += content.endsWith('\n') ? content : `${content}\n`
  }

  const literals = sensitive.literals.filter(
    (value) => typeof value === 'string' && value.length >= 8,
  )
  const presigned = sensitive.presignedUrls.filter(
    (value) => typeof value === 'string' && value.length >= 8,
  )
  const lines = combined.split(/\r?\n/)
  const violations = []

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]
    for (let literalIndex = 0; literalIndex < literals.length; literalIndex += 1) {
      if (line.includes(literals[literalIndex])) {
        violations.push({
          line: index + 1,
          name: `清单敏感值 #${literalIndex}（长度 ${literals[literalIndex].length}）`,
        })
      }
    }
    for (let urlIndex = 0; urlIndex < presigned.length; urlIndex += 1) {
      if (line.includes(presigned[urlIndex])) {
        violations.push({
          line: index + 1,
          name: `预签名 URL #${urlIndex}（仅记录 origin+path）`,
        })
      }
    }
    for (const pattern of FORBIDDEN_PATTERNS) {
      if (pattern.regex.test(line)) {
        violations.push({ line: index + 1, name: pattern.name })
      }
    }
  }

  console.log(
    `已扫描 ${logs.length} 个日志文件、${lines.length} 行、${combined.length} 字节；` +
      `清单含 ${literals.length} 个敏感字面值、${presigned.length} 个预签名 URL。`,
  )
  if (violations.length > 0) {
    console.error(`发现 ${violations.length} 处疑似敏感信息（不打印具体值）：`)
    for (const violation of violations) {
      console.error(`  第 ${violation.line} 行：${violation.name}`)
    }
    process.exit(1)
  }
  console.log('通过：日志中未出现令牌、auth_key/密钥、预签名签名参数、邮件令牌或测试明文。')
}

main().catch((error) => {
  console.error(`扫描失败：${error.message}`)
  process.exit(2)
})
