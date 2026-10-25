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
})
