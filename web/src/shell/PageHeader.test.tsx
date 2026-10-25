import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'

import { PageHeader } from './PageHeader'

describe('PageHeader', () => {
  it('根目录只显示“全部文件”标题，不显示多余面包屑', () => {
    render(<PageHeader title="全部文件" subtitle="3 项" />)
    expect(
      screen.getByRole('heading', { name: '全部文件' }),
    ).toBeVisible()
    expect(screen.getByTestId('breadcrumb')).toHaveTextContent('全部文件')
    expect(screen.queryByRole('button')).not.toBeInTheDocument()
  })

  it('祖先路径是灰色可点击链接，当前文件夹是 H1', async () => {
    const user = userEvent.setup()
    const onNavigate = vi.fn()
    render(
      <PageHeader
        ancestors={[
          { id: 'root', label: '全部文件', onClick: onNavigate },
          { id: 'a', label: '照片', onClick: onNavigate },
        ]}
        title="合同"
      />,
    )
    expect(screen.getByRole('heading', { name: '合同' })).toBeVisible()
    await user.click(screen.getByRole('button', { name: '全部文件' }))
    expect(onNavigate).toHaveBeenCalledTimes(1)
  })
})
