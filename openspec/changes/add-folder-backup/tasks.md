# Tasks

## 1. 基础能力

- [x] 1.1 Worker API 层统一 429 退避（design D5）：读取 `Retry-After`、抖动、最多 8 次，向主线程报告“等待中”状态；单测覆盖缺失 `Retry-After`、超过重试次数、等待期间取消
- [x] 1.2 `crypto-core` 的 `VersionMetadata` 新增可选字段 `content_sha256`（design D3，含格式校验与 Debug 脱敏）并同步 WASM 绑定/TS 类型；Worker 上传时流式计算明文 SHA-256 写入；测试：无该字段时序列化与现有测试向量逐字节相同、含该字段的密文可被旧结构（无该字段）解密、非法格式被拒绝；`wasm-pack test` 通过
- [x] 1.3 本地选择与遍历（design D1）：`webkitdirectory` 与拖拽 `webkitGetAsEntry` 递归（含空文件夹、`readEntries` 分批）、忽略列表、名称校验、50,000 上限；单测用伪造的 Entry 树覆盖

## 2. 文件夹上传编排

- [x] 2.1 `FolderJob` 状态机与计划阶段（design D2/D4）：逐层列出并合并同名文件夹、`name_conflict` 竞态重试、文件/文件夹同名冲突跳过
- [x] 2.2 增量判定（design D3）：大小比较 → 同大小时计算 SHA-256 比较；旧文件按 `mtime` 回退；判定结果计入摘要
- [x] 2.3 预检（配额、文件数上限）通过后才创建目录；派发到 TransferClient：新文件新建、变化文件上传新版本；取消、重试失败项、`beforeunload` 提示
- [x] 2.4 上传菜单“上传文件夹”入口与整页拖入文件夹（与现有拖拽文件覆盖层合并）

## 3. 文件夹打包下载

- [x] 3.1 引入 `client-zip`（记录体积增量与许可证）；子树遍历与条目清单（含空文件夹、`mtime`）
- [x] 3.2 逐文件解密流 → zip 流（STORE、ZIP64）；输出路径：File System Access → Service Worker 流 → ≤500 MiB Blob 回退；任一文件失败即中止
- [x] 3.3 入口：文件夹行“更多”菜单与当前文件夹工具栏“下载为 .zip”；根目录文件名 `加密备份.zip`

## 4. 界面与反馈

- [x] 4.1 传输面板文件夹任务分组（design D6）：扫描/计划/进行中/完成状态、计数与字节进度、展开列表（最多 200 行）、取消/重试/清除、结束摘要；“服务器繁忙”提示
- [x] 4.2 配额不足、超过上限、冲突与非法名称的提示文案（口语化，参见 redesign-web-ui 文案规范）；动效沿用 D10

## 5. 测试与部署

- [x] 5.1 Playwright：上传带子目录的文件夹并逐个下载比对；拖入含空文件夹的目录；第二次备份只更新变化文件且旧版本可恢复；忽略系统文件；配额不足预检；取消与重试；1000 个小文件在低限流配置下全部完成
- [x] 5.2 Playwright：文件夹下载为 zip，解压后结构与每个文件 SHA-256 一致；注入密文篡改时打包中止
- [x] 5.3 明文扫描扩展：数据库与对象存储中不出现上传文件的明文 SHA-256；更新截图脚本加入文件夹任务面板
- [x] 5.4 生产配置 `BACKUP_USER_WRITE_RATE_LIMIT_PER_MINUTE=1200` 写入 `deploy/oss/production.env.example` 与 runbook
