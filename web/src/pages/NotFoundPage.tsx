import { Link } from 'react-router-dom'
import { FileQuestion } from 'lucide-react'

import { Brand } from '../shell/Brand'

/** 非管理员访问 /admin 时显示（D6：与不存在的页面不可区分）。 */
export function NotFoundPage() {
  return (
    <main
      data-testid="not-found-page"
      className="flex min-h-dvh flex-col bg-canvas"
    >
      <header className="flex h-16 items-center px-5 sm:px-8">
        <Brand to="/files" />
      </header>
      <div className="flex flex-1 items-center justify-center px-4 pb-16">
        <section className="w-full max-w-md rounded-card border border-line bg-white p-8 text-center shadow-pop">
          <div
            aria-hidden="true"
            className="mx-auto mb-5 flex h-14 w-14 items-center justify-center rounded-full bg-canvas text-ink-600"
          >
            <FileQuestion size={28} />
          </div>
          <h1 className="text-2xl font-semibold text-ink-900">页面不存在</h1>
          <p className="mt-2 text-sm leading-6 text-ink-600">
            你访问的页面不存在，或者你没有权限查看它。
          </p>
          <Link
            to="/files"
            className="mt-6 inline-flex font-medium text-brand-600 hover:underline"
          >
            返回我的文件
          </Link>
        </section>
      </div>
    </main>
  )
}
