import { Trash2 } from 'lucide-react'

import { AppShell } from '../shell/AppShell'
import { EmptyState } from '../ui'

/**
 * 回收站视图的临时外壳（第 5 组替换为完整 TrashView）。
 * 路由与侧边栏入口在此先接通，避免第 2 组出现死链。
 */
export function TrashPage() {
  return (
    <AppShell
      breadcrumb={
        <nav aria-label="面包屑" className="flex items-center gap-1.5 text-sm">
          <span className="font-medium text-ink-900">回收站</span>
        </nav>
      }
    >
      <section className="rounded-card border border-line bg-white">
        <EmptyState
          icon={<Trash2 size={24} />}
          title="回收站是空的"
          description="删除的文件会先放在这里，可随时恢复。"
        />
      </section>
    </AppShell>
  )
}
