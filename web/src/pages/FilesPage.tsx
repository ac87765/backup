import { FileBrowser } from '../files/FileBrowser'

/** 文件页入口：数据与 Worker 逻辑在 FileBrowser 中，页面只负责挂载。 */
export function FilesPage() {
  return <FileBrowser />
}
