// 生成界面稿截图：node design/capture.mjs [desktop]
// 依赖 web/ 下已安装的 Playwright（npm ci 后即可），输出到 design/<area>/screenshots/<name>.png。
// 截图只截取每页的 .capture 元素（窗口本身），2x 像素密度。

import { createRequire } from 'node:module'
import { mkdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const require = createRequire(join(here, '..', 'web', 'package.json'))
const { chromium } = require('playwright')

// [页面, 状态, 截图名]；每项分别生成 Windows 与 macOS 两张（-win / -mac）。
const SCREENS = {
  desktop: [
    ['login.html', 'login', 'login'],
    ['login.html', 'working', 'login-working'],
    ['login.html', 'error', 'login-error'],
    ['login.html', 'unlock', 'unlock'],
    ['login.html', 'pending', 'pending'],
    ['onboarding.html', 'folders', 'onboarding-1-folders'],
    ['onboarding.html', 'background', 'onboarding-2-background'],
    ['files.html', 'default', 'files'],
    ['files.html', 'menu', 'files-menu'],
    ['files.html', 'transfers', 'files-transfers'],
    ['files.html', 'drop', 'files-drop'],
    ['backup-folders.html', 'default', 'backup-folders'],
    ['backup-folders.html', 'paused', 'backup-folders-paused'],
    ['backup-folders.html', 'empty', 'backup-folders-empty'],
    ['add-folder.html', 'default', 'add-folder'],
    ['add-folder.html', 'quota', 'add-folder-quota'],
    ['restore.html', 'default', 'restore'],
    ['settings.html', 'general', 'settings-general'],
    ['settings.html', 'security', 'settings-security'],
    ['settings.html', 'confirm', 'settings-keep-unlocked-confirm'],
    ['settings.html', 'about', 'settings-about'],
    ['tray.html', '', 'tray'],
    ['wechat.html', 'default', 'wechat'],
    ['wechat.html', 'off', 'wechat-off'],
    ['wechat.html', 'quota', 'wechat-quota'],
    ['wechat.html', 'notfound', 'wechat-notfound'],
    ['wechat.html', 'restore', 'wechat-restore'],
  ],
}

const areas = process.argv.slice(2).length ? process.argv.slice(2) : Object.keys(SCREENS)
const browser = await chromium.launch()
const page = await browser.newPage({ viewport: { width: 1600, height: 1000 }, deviceScaleFactor: 2, reducedMotion: 'reduce' })

for (const area of areas) {
  const out = join(here, area, 'screenshots')
  mkdirSync(out, { recursive: true })
  for (const [file, state, name] of SCREENS[area]) {
    for (const os of ['win', 'mac']) {
      const url = `${pathToFileURL(join(here, area, file)).href}?os=${os}${state ? `&state=${state}` : ''}`
      await page.goto(url)
      // 对话框页面的背景是 iframe，等它加载完成。
      for (const frame of page.frames()) await frame.waitForLoadState('load')
      await page.waitForTimeout(150)
      await page.locator('.capture').screenshot({ path: join(out, `${name}-${os}.png`) })
      console.log(`${area}/screenshots/${name}-${os}.png`)
    }
  }
}

await browser.close()
