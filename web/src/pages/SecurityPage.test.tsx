import { render, screen, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AuthContext, type AuthContextValue } from '../auth/context'
import type { WorkerApi } from '../worker/types'
import { SecurityPage } from './SecurityPage'

function renderPage() {
  const value: AuthContextValue = {
    api: {} as WorkerApi,
    user: null,
    loading: false,
    unlocked: false,
    setSession: vi.fn(),
    markUnlocked: vi.fn(),
    logout: vi.fn(),
  }
  return render(
    <MemoryRouter initialEntries={['/security']}>
      <AuthContext.Provider value={value}>
        <SecurityPage />
      </AuthContext.Provider>
    </MemoryRouter>,
  )
}

/** 统计“详细说明”正文之外可见文本里的汉字数量（摘要仍计入）。 */
function countVisibleHan(root: Node): number {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
  let text = ''
  while (walker.nextNode()) {
    const node = walker.currentNode
    if (!node.parentElement?.closest('.security-details-body')) {
      text += node.textContent ?? ''
    }
  }
  return (text.match(/[\u4e00-\u9fff]/g) ?? []).length
}

const REQUIRED_PHRASES = [
  '账号邮箱',
  '文件夹树结构',
  '密文大小与块数',
  '创建、修改、删除',
  'IP 地址',
  '不会收到明文文件内容',
  '服务端下发',
  '无法从数学上完全排除',
  '桌面客户端或浏览器扩展',
  '永久无法解密',
  '恢复密钥',
  '至少准备两份',
  '密码管理器',
  '轮换恢复密钥',
]

describe('安全与隐私页', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('保留全部旧 testid 与 E2E 断言短语', () => {
    renderPage()

    const page = screen.getByTestId('security-page')
    for (const phrase of REQUIRED_PHRASES) {
      expect(page).toHaveTextContent(phrase)
    }

    expect(screen.getByTestId('server-visible-list')).toBeInTheDocument()
    expect(screen.getByTestId('web-e2ee-limits')).toBeInTheDocument()
    expect(screen.getByTestId('lost-keys')).toBeInTheDocument()
    expect(screen.getByTestId('recovery-advice')).toBeInTheDocument()
  })

  it('首屏原理图有无障碍描述，三组内容数量符合设计', () => {
    renderPage()

    expect(
      screen.getByRole('img', { name: /原理图/ }),
    ).toHaveAttribute('data-motion', 'animated')
    expect(
      within(screen.getByTestId('web-e2ee-limits')).getAllByRole('article'),
    ).toHaveLength(3)
    expect(
      within(screen.getByTestId('lost-keys')).getAllByRole('article'),
    ).toHaveLength(3)
    expect(
      within(screen.getByTestId('recovery-advice')).getAllByRole('listitem'),
    ).toHaveLength(6)
  })

  it('完整原文默认收起，并保留原页面全部信息', () => {
    renderPage()

    const details = screen.getByTestId('security-details')
    expect(details).not.toHaveAttribute('open')
    expect(details).toHaveTextContent('服务端能看到的信息')
    expect(details).toHaveTextContent('Web 版 E2EE 的局限')
    expect(details).toHaveTextContent('密码与恢复密钥同时丢失会怎样')
    expect(details).toHaveTextContent('恢复密钥保管建议')
    expect(details).toHaveTextContent(
      '注册时按分组抄写到纸张或金属助记板',
    )
    expect(details).toHaveTextContent(
      '服务端日志也不记录请求体、Cookie、会话令牌、密钥或预签名 URL 的签名部分',
    )
  })

  it('默认可见文字不超过 350 个汉字（不含详细说明）', () => {
    renderPage()

    expect(countVisibleHan(document.body)).toBeLessThanOrEqual(350)
  })

  it('减少动态效果时不挂动画 class，内容直接可见', () => {
    vi.stubGlobal(
      'matchMedia',
      vi.fn().mockReturnValue({
        matches: true,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      }),
    )

    const { container } = renderPage()

    expect(
      screen.getByRole('img', { name: /原理图/ }),
    ).toHaveAttribute('data-motion', 'static')
    expect(container.querySelector('.security-hero-animated')).toBeNull()
    expect(container.querySelector('.motion-reveal')).toBeNull()
    expect(container.querySelector('animate')).toBeNull()
  })
})
