# Tasks

> 迭代流程：设计稿（`design/web/download.html`、`design/desktop/update.html`）→ 用户确认 → DeepSeek 按组实现 → 审查者验收。签名私钥的生成与保管由用户在本机完成（审查者提供命令）。

## 0. 设计稿与决定（审查者 / 用户）

- [x] 0.1 `design/web/download.html`：下载页面（Windows 访问者、macOS 访问者、手机宽度、清单不可用）
- [x] 0.2 `design/desktop/update.html`：更新横幅、“关于”中的新版本与进度、强制更新页、托盘菜单项
- [x] 0.3 用户确认设计稿与签名私钥的保管方式

## 1. 发布基础设施

- [ ] 1.1 `release-tool`（Rust 小工具）：生成密钥对、签名清单、验签；`deploy/release-signing.pub`
- [ ] 1.2 `deploy/release-client.ps1`：取回标签 CI 产物、生成并签名 `latest.json`、自检、上传与原子替换、保留 3 个版本
- [ ] 1.3 Caddyfile 模板 `/downloads/` 静态目录与缓存头；`install.sh` / `upgrade.sh` 创建目录；运维文档
- [ ] 1.4 测试：清单生成与验签往返、篡改检测、发布脚本的干跑模式

## 2. 客户端更新（Rust + Flutter）

- [ ] 2.1 `client-core`：检查、验签、防回滚记录、下载与哈希校验（`desktop` feature）
- [ ] 2.2 Windows 静默安装与重启；macOS 替换应用与重启（失败退回打开 .dmg）
- [ ] 2.3 Flutter：横幅、“关于”中的检查 / 下载进度 / 安装、托盘菜单项、强制更新页；golden
- [ ] 2.4 测试：篡改清单 / 篡改安装包 / 旧清单降级 / 最低支持版本 / 不携带凭据

## 3. 下载页面

- [ ] 3.1 `web/`：公开路由 `/download`，系统识别、清单读取与错误状态、复制 SHA-256、首次打开说明、更新日志；登录页与设置的入口
- [ ] 3.2 测试：组件测试（三种系统、清单不可用）；Playwright（未登录可访问、严格 CSP 下正常）

## 4. 首次发布与验收

- [ ] 4.1 部署 Caddy 配置（需用户同意写入服务器）；发布 `v0.1.0`
- [ ] 4.2 Windows 真机：从下载页安装 `v0.1.0`，发布 `v0.1.1` 后客户端提示并一键升级成功
- [ ] 4.3 macOS：用户 Mac 上的验收清单（写入 `docs/macos-handoff.md`）
