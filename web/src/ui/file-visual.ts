import {
  File,
  FileArchive,
  FileCode,
  FileSpreadsheet,
  FileText,
  Film,
  Folder,
  Image,
  Music,
  Presentation,
  type LucideIcon,
} from 'lucide-react'

import type { NodeKind } from '../worker/types'

/** D4 文件类型分类。 */
export type FileVisualKind =
  | 'folder'
  | 'image'
  | 'video'
  | 'audio'
  | 'pdf'
  | 'document'
  | 'spreadsheet'
  | 'presentation'
  | 'archive'
  | 'code'
  | 'text'
  | 'other'

const EXTENSION_KINDS: Record<string, FileVisualKind> = {
  // 图片
  jpg: 'image',
  jpeg: 'image',
  png: 'image',
  gif: 'image',
  webp: 'image',
  svg: 'image',
  bmp: 'image',
  heic: 'image',
  heif: 'image',
  avif: 'image',
  tif: 'image',
  tiff: 'image',
  ico: 'image',
  // 视频
  mp4: 'video',
  m4v: 'video',
  mov: 'video',
  avi: 'video',
  mkv: 'video',
  webm: 'video',
  wmv: 'video',
  flv: 'video',
  mpeg: 'video',
  mpg: 'video',
  // 音频
  mp3: 'audio',
  wav: 'audio',
  flac: 'audio',
  aac: 'audio',
  ogg: 'audio',
  m4a: 'audio',
  wma: 'audio',
  opus: 'audio',
  // PDF
  pdf: 'pdf',
  // 文档
  doc: 'document',
  docx: 'document',
  odt: 'document',
  rtf: 'document',
  pages: 'document',
  // 表格
  xls: 'spreadsheet',
  xlsx: 'spreadsheet',
  ods: 'spreadsheet',
  csv: 'spreadsheet',
  numbers: 'spreadsheet',
  // 演示
  ppt: 'presentation',
  pptx: 'presentation',
  odp: 'presentation',
  key: 'presentation',
  // 压缩包
  zip: 'archive',
  rar: 'archive',
  '7z': 'archive',
  tar: 'archive',
  gz: 'archive',
  bz2: 'archive',
  xz: 'archive',
  tgz: 'archive',
  zst: 'archive',
  // 代码
  js: 'code',
  jsx: 'code',
  ts: 'code',
  tsx: 'code',
  json: 'code',
  html: 'code',
  htm: 'code',
  css: 'code',
  scss: 'code',
  less: 'code',
  rs: 'code',
  go: 'code',
  py: 'code',
  java: 'code',
  c: 'code',
  h: 'code',
  cpp: 'code',
  hpp: 'code',
  cs: 'code',
  rb: 'code',
  php: 'code',
  sh: 'code',
  ps1: 'code',
  sql: 'code',
  yml: 'code',
  yaml: 'code',
  toml: 'code',
  xml: 'code',
  // 文本
  txt: 'text',
  log: 'text',
  md: 'text',
  markdown: 'text',
  ini: 'text',
  cfg: 'text',
  conf: 'text',
}

export interface FileVisualMeta {
  label: string
  className: string
  Icon: LucideIcon
}

export const FILE_VISUAL_META: Record<FileVisualKind, FileVisualMeta> = {
  folder: { label: '文件夹', className: 'text-file-folder', Icon: Folder },
  image: { label: '图片', className: 'text-file-image', Icon: Image },
  video: { label: '视频', className: 'text-file-video', Icon: Film },
  audio: { label: '音频', className: 'text-file-audio', Icon: Music },
  pdf: { label: 'PDF', className: 'text-file-pdf', Icon: FileText },
  document: { label: '文档', className: 'text-file-doc', Icon: FileText },
  spreadsheet: {
    label: '表格',
    className: 'text-file-sheet',
    Icon: FileSpreadsheet,
  },
  presentation: {
    label: '演示',
    className: 'text-file-slides',
    Icon: Presentation,
  },
  archive: { label: '压缩包', className: 'text-file-archive', Icon: FileArchive },
  code: { label: '代码', className: 'text-file-code', Icon: FileCode },
  text: { label: '文本', className: 'text-file-text', Icon: FileText },
  other: { label: '其他', className: 'text-file-other', Icon: File },
}

/** 根据节点类型与文件名（扩展名）判断展示分类；不读取内容、不发请求。 */
export function fileVisualKind(name: string, kind: NodeKind): FileVisualKind {
  if (kind === 'folder') {
    return 'folder'
  }
  const normalized = name.trim().toLowerCase()
  const dot = normalized.lastIndexOf('.')
  if (dot <= 0 || dot === normalized.length - 1) {
    return 'other'
  }
  return EXTENSION_KINDS[normalized.slice(dot + 1)] ?? 'other'
}
