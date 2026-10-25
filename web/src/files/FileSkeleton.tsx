/** 与真实文件行等高的加载骨架；光泽扫过在 reduced motion 下停止。 */
export function FileSkeleton({ rows = 6 }: { rows?: number }) {
  return (
    <div
      data-testid="file-skeleton"
      role="status"
      aria-label="正在加载文件"
      className="w-full"
    >
      <div className="flex items-center border-y border-line bg-canvas px-3 py-2">
        <div className="skeleton h-4 w-24 rounded" />
      </div>
      {Array.from({ length: rows }, (_, index) => (
        <div
          key={index}
          className="flex h-[60px] items-center gap-3 border-b border-line px-3"
        >
          <div className="skeleton h-8 w-8 shrink-0 rounded-control" />
          <div className="min-w-0 flex-1 space-y-2">
            <div className="skeleton h-3.5 w-1/3 rounded" />
            <div className="skeleton h-3 w-1/5 rounded md:hidden" />
          </div>
          <div className="skeleton hidden h-3 w-24 rounded md:block" />
          <div className="skeleton hidden h-3 w-14 rounded md:block" />
        </div>
      ))}
    </div>
  )
}
