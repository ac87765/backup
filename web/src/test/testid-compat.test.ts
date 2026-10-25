import { readdirSync, readFileSync } from 'node:fs'
import path from 'node:path'

import { describe, expect, it } from 'vitest'

/**
 * 从 main 分支（改造前）导出的全部 data-testid。
 * 新界面必须继续保留这些钩子；新增钩子不受限制。
 */
const MAIN_TESTIDS = [
  'breadcrumb',
  'change-confirm-password',
  'change-new-password',
  'change-old-password',
  'change-submit',
  'confirm-permanent-delete',
  'create-folder-submit',
  'file-drop-zone',
  'file-row',
  'folder-name',
  'folder-picker',
  'home-email',
  'home-verified',
  'login-email',
  'login-password',
  'login-submit',
  'lost-keys',
  'new-folder',
  'open-trash',
  'picker-breadcrumb',
  'picker-folder',
  'picker-select-current',
  'recovery-advice',
  'recovery-display',
  'register-email',
  'register-password',
  'register-submit',
  'rename-name',
  'rename-submit',
  'reset-confirm-password',
  'reset-email',
  'reset-new-password',
  'reset-recovery-key',
  'reset-submit',
  'rotate-current-password',
  'rotate-prepare',
  'security-page',
  'server-visible-list',
  'transfer-cancel',
  'transfer-dismiss',
  'transfer-item',
  'transfer-pause',
  'transfer-progress',
  'transfer-resume',
  'transfer-retry',
  'trash-item',
  'unlock-password',
  'unlock-submit',
  'upload-file',
  'upload-input',
  'usage-summary',
  'version-input',
  'web-e2ee-limits',
] as const

const MAIN_DYNAMIC_TESTIDS = [
  'data-testid={`download-${item.id}`}',
  'data-testid={`download-version-${index}`}',
  'data-testid={`recovery-confirm-${position}`}',
  'data-testid={`restore-version-${index}`}',
  'data-testid={`resume-upload-input-${item.sessionId ?? \'\'}`}',
] as const

function collectSourceFiles(directory: string): string[] {
  const files: string[] = []
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const target = path.join(directory, entry.name)
    if (entry.isDirectory()) {
      if (entry.name === 'generated') {
        continue
      }
      files.push(...collectSourceFiles(target))
    } else if (
      entry.name.endsWith('.tsx') ||
      entry.name.endsWith('.ts')
    ) {
      if (!/\.(test|spec)\.tsx?$/.test(entry.name)) {
        files.push(target)
      }
    }
  }
  return files
}

describe('data-testid 兼容性', () => {
  const source = collectSourceFiles(path.resolve(process.cwd(), 'src'))
    .map((file) => readFileSync(file, 'utf8'))
    .join('\n')

  it('main 分支上的静态 data-testid 全部保留', () => {
    const missing = MAIN_TESTIDS.filter(
      (testId) =>
        !source.includes(`data-testid="${testId}"`) &&
        !source.includes(`testId: '${testId}'`) &&
        !source.includes(`testId="${testId}"`),
    )
    expect(missing).toEqual([])
  })

  it('main 分支上的动态 data-testid 模板全部保留', () => {
    const missing = MAIN_DYNAMIC_TESTIDS.filter(
      (pattern) => !source.includes(pattern),
    )
    expect(missing).toEqual([])
  })
})
