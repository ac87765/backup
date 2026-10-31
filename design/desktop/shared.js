// 界面稿共用脚本：平台 / 状态切换、图标、窗口外框与侧边栏。
// 查询参数：?os=win|mac（默认 win），?state=<页面定义的状态名>。
// 带 data-when="a b" 的元素只在 state 为 a 或 b 时保留。
// 图标统一使用 Lucide（stroke 2，圆角端点），Flutter 侧使用 lucide_icons 中同名图标。

const ICONS = {
  folder:
    '<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>',
  'folder-plus':
    '<path d="M12 10v6"/><path d="M9 13h6"/><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>',
  file: '<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/>',
  'file-text':
    '<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="M10 9H8"/><path d="M16 13H8"/><path d="M16 17H8"/>',
  sheet:
    '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M3 9h18"/><path d="M3 15h18"/><path d="M9 3v18"/>',
  image:
    '<rect width="18" height="18" x="3" y="3" rx="2" ry="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"/>',
  video:
    '<path d="m16 13 5.223 3.482a.5.5 0 0 0 .777-.416V7.87a.5.5 0 0 0-.752-.432L16 10.5"/><rect x="2" y="6" width="14" height="12" rx="2"/>',
  archive:
    '<rect width="20" height="5" x="2" y="3" rx="1"/><path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8"/><path d="M10 12h4"/>',
  search: '<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>',
  upload:
    '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="m17 8-5-5-5 5"/><path d="M12 3v12"/>',
  download:
    '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="m7 10 5 5 5-5"/><path d="M12 15V3"/>',
  'cloud-upload':
    '<path d="M4 14.899A7 7 0 1 1 15.71 8h1.79a4.5 4.5 0 0 1 2.5 8.242"/><path d="M12 12v9"/><path d="m16 16-4-4-4 4"/>',
  cloud: '<path d="M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z"/>',
  plus: '<path d="M5 12h14"/><path d="M12 5v14"/>',
  settings:
    '<path d="M20 7h-9"/><path d="M14 17H5"/><circle cx="17" cy="17" r="3"/><circle cx="7" cy="7" r="3"/>',
  trash:
    '<path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/>',
  clock: '<circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/>',
  shield:
    '<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/>',
  'shield-check':
    '<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/><path d="m9 12 2 2 4-4"/>',
  refresh:
    '<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/>',
  pause: '<rect x="14" y="4" width="4" height="16" rx="1"/><rect x="6" y="4" width="4" height="16" rx="1"/>',
  play: '<path d="M6 3l14 9-14 9z"/>',
  check: '<path d="M20 6 9 17l-5-5"/>',
  'check-circle': '<circle cx="12" cy="12" r="10"/><path d="m9 12 2 2 4-4"/>',
  alert:
    '<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>',
  lock: '<rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>',
  laptop:
    '<path d="M20 16V7a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v9m16 0H4m16 0 1.28 2.55a1 1 0 0 1-.9 1.45H3.62a1 1 0 0 1-.9-1.45L4 16"/>',
  'chevron-right': '<path d="m9 18 6-6-6-6"/>',
  'chevron-down': '<path d="m6 9 6 6 6-6"/>',
  'chevron-left': '<path d="m15 18-6-6 6-6"/>',
  more: '<circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/><circle cx="5" cy="12" r="1"/>',
  x: '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>',
  list: '<path d="M8 6h13"/><path d="M8 12h13"/><path d="M8 18h13"/><path d="M3 6h.01"/><path d="M3 12h.01"/><path d="M3 18h.01"/>',
  grid: '<rect width="7" height="7" x="3" y="3" rx="1"/><rect width="7" height="7" x="14" y="3" rx="1"/><rect width="7" height="7" x="14" y="14" rx="1"/><rect width="7" height="7" x="3" y="14" rx="1"/>',
  history:
    '<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l4 2"/>',
  external:
    '<path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>',
  info: '<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>',
  'arrow-right': '<path d="M5 12h14"/><path d="m12 5 7 7-7 7"/>',
  'hard-drive':
    '<path d="M22 12H2"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/>',
  filter: '<path d="M22 3H2l8 9.46V19l4 2v-8.54z"/>',
  minus: '<path d="M5 12h14"/>',
  square: '<rect width="14" height="14" x="5" y="5" rx="1"/>',
  power: '<path d="M12 2v10"/><path d="M18.4 6.6a9 9 0 1 1-12.77.04"/>',
  hourglass:
    '<path d="M5 22h14"/><path d="M5 2h14"/><path d="M17 22v-4.172a2 2 0 0 0-.586-1.414L12 12l-4.414 4.414A2 2 0 0 0 7 17.828V22"/><path d="M7 2v4.172a2 2 0 0 0 .586 1.414L12 12l4.414-4.414A2 2 0 0 0 17 6.172V2"/>',
  user: '<path d="M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/>',
  'message-circle': '<path d="M7.9 20A9 9 0 1 0 4 16.1L2 22Z"/>',
  database: '<ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M3 5V19A9 3 0 0 0 21 19V5"/><path d="M3 12A9 3 0 0 0 21 12"/>',
  smartphone: '<rect width="14" height="20" x="5" y="2" rx="2" ry="2"/><path d="M12 18h.01"/>',
  'rotate-ccw': '<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/>',
  sliders:
    '<path d="M4 21v-7"/><path d="M4 10V3"/><path d="M12 21v-9"/><path d="M12 8V3"/><path d="M20 21v-5"/><path d="M20 12V3"/><path d="M2 14h4"/><path d="M10 8h4"/><path d="M18 16h4"/>',
  monitor: '<rect width="20" height="14" x="2" y="3" rx="2"/><path d="M8 21h8"/><path d="M12 17v4"/>',
  'eye-off':
    '<path d="M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49"/><path d="M14.084 14.158a3 3 0 0 1-4.242-4.242"/><path d="M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143"/><path d="m2 2 20 20"/>',
  'arrow-up-down':
    '<path d="m21 16-4 4-4-4"/><path d="M17 20V4"/><path d="m3 8 4-4 4 4"/><path d="M7 4v16"/>',
}

const params = new URLSearchParams(location.search)
const root = document.documentElement
root.dataset.os = params.get('os') || 'win'
const state = params.get('state') || root.dataset.defaultState || ''
root.dataset.state = state
// ?embed=1：作为对话框背景嵌入 iframe 时，去掉画布留白与窗口阴影。
if (params.get('embed')) root.dataset.embed = '1'

const PARTIALS = {
  // 窗口标题栏：Windows 11 为左侧图标 + 标题、右侧三个按钮；macOS 为红绿灯 + 居中标题。
  titlebar: (el) => `
    <div class="traffic"><b></b><b></b><b></b></div>
    <div class="app-title"><span class="logo" style="--s:16px;--s-inner:10px"><i data-icon="lock"></i></span>${el.dataset.title || '加密备份'}</div>
    <div class="caption"><span><i data-icon="minus" style="--s:14px"></i></span><span><i data-icon="square" style="--s:12px"></i></span><span><i data-icon="x" style="--s:14px"></i></span></div>`,

  // 左侧导航：全部文件 / 最近 / 备份文件夹 / 回收站 | 安全与隐私；底部配额与账号。
  sidebar: (el) => {
    const active = el.dataset.active || 'files'
    const item = (key, icon, label, extra = '') =>
      `<a class="${key === active ? 'active' : ''}"><i data-icon="${icon}"></i>${label}${extra}</a>`
    return `
      <div class="brand"><span class="logo" style="--s:28px;--s-inner:16px"><i data-icon="lock"></i></span>加密备份</div>
      <nav class="nav">
        ${item('files', 'folder', '全部文件')}
        ${item('recent', 'clock', '最近')}
        ${item('backup', 'cloud-upload', '备份文件夹', el.dataset.backupBadge ? `<span class="badge">${el.dataset.backupBadge}</span>` : '')}
        ${el.dataset.wechat !== undefined ? item('wechat', 'message-circle', '微信备份') : ''}
        ${item('trash', 'trash', '回收站')}
        <div class="nav-sep"></div>
        ${item('security', 'shield', '安全与隐私')}
      </nav>
      <div class="quota">
        <div class="row" style="justify-content:space-between"><span>存储空间</span><span>12.4 GB / 50 GB</span></div>
        <div class="bar"><b style="width:25%"></b></div>
      </div>
      <div class="account">
        <span class="avatar">T</span>
        <div class="stack" style="min-width:0;flex:1"><b style="font-weight:500;overflow:hidden;text-overflow:ellipsis">tlj978626571@gmail.com</b><span class="small muted">已加密 · 本机已解锁</span></div>
        <button class="btn btn-ghost btn-icon"><i data-icon="settings"></i></button>
      </div>`
  },

  // 底部状态栏：左侧备份状态，右侧传输入口。
  statusbar: (el) => {
    const s = el.dataset.status || 'ok'
    const left = {
      ok: '<span class="dot"></span>已全部备份 · 3 个备份文件夹',
      busy: '<span class="dot busy"></span>正在备份 · 剩余 12 个文件',
      paused: '<span class="dot warn"></span>备份已暂停',
      none: '<span class="dot" style="background:var(--ink-400)"></span>还没有备份文件夹',
    }[s]
    const right = s === 'busy' ? '传输 · 3 项进行中' : '传输'
    return `<span class="item">${left}</span><span class="spacer"></span><span class="item link"><i data-icon="arrow-up-down" style="--s:14px"></i>${right}</span>`
  },
}

document.querySelectorAll('[data-when]').forEach((el) => {
  if (!el.dataset.when.split(' ').includes(state)) el.remove()
})

document.querySelectorAll('[data-partial]').forEach((el) => {
  el.innerHTML = PARTIALS[el.dataset.partial](el)
})

// macOS 版本：本地路径与设备名换成 macOS 写法。
if (root.dataset.os === 'mac') {
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT)
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    let t = n.nodeValue
    if (/[A-Z]:\\/.test(t)) t = t.replace(/C:\\Users\\tlj\\?/g, '~/').replace(/[A-Z]:\\/g, '~/').replace(/\\/g, '/')
    t = t.replace(/DESKTOP-TLJ/g, 'TLJ 的 MacBook Pro')
      .replace('Windows 凭据保护', 'macOS 钥匙串')
      .replace('任务栏右下角的托盘', '屏幕顶部的菜单栏')
    n.nodeValue = t
  }
}

document.querySelectorAll('i[data-icon]').forEach((el) => {
  el.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${ICONS[el.dataset.icon] || ''}</svg>`
})
