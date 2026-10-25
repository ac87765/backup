# Tasks

## 1. 插图组件

- [x] 1.1 首屏原理图组件（design D1.1 / D2）：内联 SVG、CSS keyframes 循环动画、`role="img"` 与完整 `aria-label`、reduced motion 静帧；组件测试覆盖 aria 与 reduced motion 下无动画 class
- [x] 1.2 小插图：能看到/看不到两列图标、三张局限卡片图标、两把钥匙状态图（完整/丢失）、保管清单图标；全部用 D2 令牌着色

## 2. 页面重写

- [x] 2.1 按 design D1 重写 `SecurityPage.tsx`：首屏、能看到/看不到、局限卡片、三格对照、保管清单、`<details>` 完整说明；保留现有 5 个 `data-testid`
- [x] 2.2 区块进入视口淡入（IntersectionObserver 切 class，不支持时直接显示）；悬停上浮；reduced motion 全部关闭
- [ ] 2.3 文案按 design D3 控制长度；完整原文逐字迁移到“详细说明”；登录页与侧边栏入口不变；手机端（390px）布局无横向滚动

## 3. 测试

- [ ] 3.1 更新 `security.spec.ts`：保留现有全部断言（账号邮箱、文件夹树结构、密文大小与块数、创建、修改、删除、IP 地址、不会收到明文文件内容、服务端下发、无法从数学上完全排除、桌面客户端或浏览器扩展、永久无法解密、恢复密钥、至少准备两份、密码管理器、轮换恢复密钥）；新增：原理图可访问描述、详细说明展开/收起、reduced motion 下 `document.getAnimations()` 中没有运行中的动画、默认可见文字长度不超过 350 个汉字（统计 `<details>` 之外的可见文本）
- [ ] 3.2 CSP E2E 覆盖 `/security` 无违规；截图脚本加入安全页（桌面与手机）
