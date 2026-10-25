import { expect, test } from '@playwright/test'

test('安全与隐私页面可访问，并列出服务端可见信息、Web E2EE 局限与密钥丢失后果', async ({
  page,
}) => {
  // 从登录页的真实导航入口进入，确认页面在导航中可达。
  await page.goto('/login')
  await page
    .getByRole('link', { name: '了解加密方式' })
    .click()
  await expect(page).toHaveURL(/\/security$/)
  await expect(
    page.getByRole('heading', { name: '安全与隐私' }),
  ).toBeVisible()

  // 首屏原理图带完整无障碍描述，并配一句话结论。
  const hero = page.getByRole('img', { name: /原理图/ })
  await expect(hero).toBeVisible()
  await expect(hero).toHaveAttribute('aria-label', /浏览器/)
  await expect(
    page.getByText('文件在你的浏览器里加密后才上传，我们和云厂商都打不开。'),
  ).toBeVisible()

  const visible = page.getByTestId('server-visible-list')
  await expect(visible).toContainText('账号邮箱')
  await expect(visible).toContainText('文件夹树结构')
  await expect(visible).toContainText('密文大小与块数')
  await expect(visible).toContainText('创建、修改、删除')
  await expect(visible).toContainText('IP 地址')
  await expect(page.getByTestId('security-page')).toContainText(
    '不会收到明文文件内容',
  )

  const limits = page.getByTestId('web-e2ee-limits')
  await expect(limits).toContainText('服务端下发')
  await expect(limits).toContainText('无法从数学上完全排除')
  await expect(limits).toContainText('桌面客户端或浏览器扩展')

  const lost = page.getByTestId('lost-keys')
  await expect(lost).toContainText('永久无法解密')
  await expect(lost).toContainText('无法')
  await expect(lost).toContainText('恢复密钥')

  const advice = page.getByTestId('recovery-advice')
  await expect(advice).toContainText('至少准备两份')
  await expect(advice).toContainText('密码管理器')
  await expect(advice).toContainText('轮换恢复密钥')

  // “详细说明”默认收起，展开后保留原页面全文，再次点击可收起。
  const details = page.getByTestId('security-details')
  await expect(details).not.toHaveAttribute('open', '')
  await expect(
    page.getByRole('heading', { name: '服务端能看到的信息' }),
  ).toBeHidden()
  await details.locator('summary').click()
  await expect(details).toHaveAttribute('open', '')
  await expect(
    page.getByRole('heading', { name: '服务端能看到的信息' }),
  ).toBeVisible()
  await expect(details).toContainText('服务端日志也不记录请求体')
  await details.locator('summary').click()
  await expect(details).not.toHaveAttribute('open', '')
})

test('减少动态效果时原理图静止，页面没有运行中的动画', async ({ page }) => {
  // 显式开启，不依赖各 Playwright 配置（容器化配置未全局开启 reducedMotion）。
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.goto('/security')
  await expect(
    page.getByRole('heading', { name: '安全与隐私' }),
  ).toBeVisible()

  expect(
    await page.evaluate(
      () => window.matchMedia('(prefers-reduced-motion: reduce)').matches,
    ),
  ).toBe(true)
  await expect(page.getByTestId('security-hero')).toHaveAttribute(
    'data-motion',
    'static',
  )

  const runningAnimations = await page.evaluate(
    () =>
      document
        .getAnimations()
        .filter((animation) => animation.playState === 'running').length,
  )
  expect(runningAnimations).toBe(0)
})

test('默认可见文字不超过 350 个汉字，手机端无横向滚动', async ({ page }) => {
  await page.goto('/security')
  await expect(page.getByTestId('security-page')).toBeVisible()

  const visibleHan = await page.evaluate(() => {
    const root = document.querySelector('[data-testid="security-page"]')
    if (!root) {
      return -1
    }
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
    let text = ''
    while (walker.nextNode()) {
      const node = walker.currentNode
      if (!node.parentElement?.closest('.security-details-body')) {
        text += node.textContent ?? ''
      }
    }
    return (text.match(/[\u4e00-\u9fff]/g) ?? []).length
  })
  expect(visibleHan).toBeLessThanOrEqual(350)

  await page.setViewportSize({ width: 390, height: 844 })
  await page.reload()
  await expect(page.getByTestId('security-hero')).toBeVisible()
  const horizontalOverflow = await page.evaluate(
    () =>
      document.documentElement.scrollWidth -
      document.documentElement.clientWidth,
  )
  expect(horizontalOverflow).toBeLessThanOrEqual(0)
})
