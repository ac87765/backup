import { useNavigate } from 'react-router-dom'
import { Clock3, LogOut, ShieldCheck } from 'lucide-react'

import { useAuth } from '../auth/context'
import { Brand } from '../shell/Brand'
import { Button } from '../ui'

/**
 * 待审核账号的等待页（add-admin-console D6）。
 *
 * pending 用户的全部登录态路由都显示该页面：不展示文件内容，也不提供上传入口。
 */
export function PendingPage() {
  const { user, logout } = useAuth()
  const navigate = useNavigate()

  return (
    <main
      data-testid="pending-page"
      className="flex min-h-dvh flex-col bg-canvas"
    >
      <header className="flex h-16 items-center px-5 sm:px-8">
        <Brand to="/files" />
      </header>
      <div className="flex flex-1 items-center justify-center px-4 pb-16">
        <section className="w-full max-w-lg rounded-card border border-line bg-white p-8 shadow-pop sm:p-10">
          <div
            aria-hidden="true"
            className="mb-6 flex h-16 w-16 items-center justify-center rounded-full bg-brand-50 text-brand-600"
          >
            <Clock3 size={30} />
          </div>
          <h1 className="text-2xl font-semibold text-ink-900">
            正在等待管理员审核
          </h1>
          <p className="mt-3 text-sm leading-6 text-ink-600">
            你的账号（
            <strong className="break-all font-medium text-ink-900">
              {user?.email ?? ''}
            </strong>
            ）已注册成功，正在等待管理员审核。开通后会发邮件通知你，届时刷新页面即可开始使用。
          </p>
          <ul className="mt-5 space-y-2 text-sm leading-6 text-ink-600">
            <li>· 审核通过前不能上传文件或新建文件夹。</li>
            <li>· 你的文件在浏览器里加密，服务端看不到内容。</li>
          </ul>
          <div className="mt-8 flex flex-col gap-3 sm:flex-row">
            <Button
              variant="primary"
              data-testid="pending-logout"
              icon={<LogOut size={16} />}
              onClick={() => {
                void logout()
                  .catch(() => undefined)
                  .finally(() => {
                    void navigate('/login', { replace: true })
                  })
              }}
            >
              退出登录
            </Button>
            <Button
              variant="secondary"
              icon={<ShieldCheck size={16} />}
              onClick={() => void navigate('/security')}
            >
              安全与隐私
            </Button>
          </div>
        </section>
      </div>
    </main>
  )
}
