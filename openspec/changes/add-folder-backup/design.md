# Design

## Context

- 服务端已有：创建文件夹（名称密文 + name token 唯一约束，冲突返回 `name_conflict`）、列子节点（游标分页）、上传会话状态机（创建会话 → 批量预签名 → 直传 OSS → 批量确认 → 提交）、上传新版本、版本历史、配额预留、每用户写接口限流（默认 240/分钟，`BACKUP_USER_WRITE_RATE_LIMIT_PER_MINUTE` 可配）。
- 客户端：加密、名称密文、上传与下载都在 Worker 的 TransferClient 中；主线程只做编排与界面。下载已有三条路径：File System Access API、Service Worker 流式下载、Blob 回退（小文件）。
- 一个小文件的上传大约 4 次写请求。1000 个小文件 ≈ 4000 次写请求，默认限流下必然触发 429。

## Goals / Non-Goals

**Goals：** 结构保留的文件夹上传；重复备份只上传变化；可靠的大批量编排（限流、失败、取消）；整个文件夹流式打包下载。

**Non-Goals：** 定时/自动后台备份（属于桌面客户端）；删除同步（本地删掉的文件不会在云端删除）；服务端打包下载（服务端看不到明文，只能在客户端打包）；页面关闭后继续未开始的文件（已开始的单个文件沿用现有断点续传）。

## Decisions

### D1 选择与遍历

- 按钮：`<input type="file" webkitdirectory multiple>`，用 `webkitRelativePath` 还原相对路径（浏览器不会提供空文件夹）。
- 拖拽：`DataTransferItem.webkitGetAsEntry()` 递归遍历 `FileSystemDirectoryEntry`（`readEntries` 需循环直到返回空数组），可得到空文件夹。拖入多个顶层项时，每个顶层文件夹是一个任务，散落文件归入同一个“文件”任务。
- 遍历在主线程分批进行（每批让出事件循环），10 万项内界面不卡顿；遍历中显示“正在扫描… 已发现 N 个文件”。
- 忽略列表（大小写不敏感）：`.DS_Store`、`Thumbs.db`、`desktop.ini`、`._*`（macOS 资源分叉）、`~$*`（Office 锁文件）、`.~lock.*#`。
- 单个任务上限 50,000 个文件；超过时提示用户拆分，不开始。

### D2 目标树的增量合并

1. 在当前文件夹下处理任务根目录名：若已存在同名文件夹 → 复用；不存在 → 创建。
2. 自上而下逐层：列出该层已有子节点（解密名称，按 NFC + 小写比较，与服务端 name token 规则一致），已存在的同名文件夹复用，缺失的创建。并发创建时若返回 `name_conflict`，重新列出该层并复用（他处刚创建）。
3. 同名“文件”与“文件夹”冲突（本地是文件夹、云端是同名文件或反之）：该项跳过并在摘要中报告，不覆盖。
4. 文件：云端已有同名文件 → 比较内容摘要（见 D3），相同则跳过，不同则作为新版本上传；云端不存在 → 新建文件上传。

### D3 内容未变化判定

- 新增加密元数据字段 `content_sha256`：上传时客户端计算明文 SHA-256，写入版本的加密元数据（与名称、大小同在密文元数据中，服务端不可见）。
- 再次备份时：先比较大小与本地 `lastModified`（元数据里已有 `mtime` 则用之）；大小不同 → 直接视为变化；大小相同 → 计算本地 SHA-256 与云端当前版本元数据中的 `content_sha256` 比较。
- 旧版本没有 `content_sha256` 的文件：大小相同且 `mtime` 相同视为未变化，否则上传新版本。
- 实现位置：`crates/crypto-core/src/node.rs` 的 `VersionMetadata` 增加 `#[serde(default, skip_serializing_if = "Option::is_none")] pub content_sha256: Option<String>`（64 位小写十六进制，反序列化时校验格式），追加在 JSON 契约末尾：`{"content_id":…,"size":…,"mtime":…,"mime":…,"content_sha256":"…"}`。WASM 绑定与 TS 类型同步。
- 兼容性：字段为 `None` 时序列化结果与现有契约逐字节相同（现有测试向量必须不变）；当前结构未启用 `deny_unknown_fields`，已部署的旧客户端解密含新字段的元数据时会忽略它。AAD、帧格式、HKDF info、密钥层级全部不变（遵守 bootstrap design D4/D5 不可变约束）。这是对版本元数据 JSON 契约的唯一一次向后兼容扩展，审查者已批准。
- `Debug` 输出不得包含 `content_sha256`（与 mime 一样脱敏）。

### D4 编排与并发

- `FolderJob`（主线程）：状态 `scanning → planning → running → done/cancelled`，持有任务树、计数（新增/更新/跳过/失败）与字节进度。
- 计划阶段先完成 D2 的目录合并与 D3 的判定（判定中的哈希计算在 Worker 中流式进行），再把待上传文件交给现有 TransferClient 队列；文件级并发沿用现有设置（默认 3）。
- 预检：待上传字节总数 > 剩余配额 → 提示并停止（不创建任何上传会话）。目录创建在预检通过后才开始。
- 取消：停止派发新文件、取消进行中的上传（释放配额预留），已完成的保留；摘要标注“已取消”。
- 页面关闭：任务进行中时注册 `beforeunload` 提示。

### D5 限流退避

- Worker 的 API 层对 `429` 统一处理：读取 `Retry-After`（秒，缺失按 5 秒），加 0–20% 抖动后等待重试，同一请求最多重试 8 次；等待期间传输面板显示“服务器繁忙，稍后自动继续”。这是通用行为，单文件上传也受益。
- 生产配置 `BACKUP_USER_WRITE_RATE_LIMIT_PER_MINUTE=1200`。

### D6 传输面板分组

- 文件夹任务在面板中是一项：文件夹图标、名称、`已完成 120 / 356 个文件 · 1.2 GB / 3.4 GB`、总体进度条；可展开显示进行中与失败的单个文件（最多渲染 200 行，其余折叠为“还有 N 个”）。
- 操作：取消任务、重试失败项、清除。结束摘要：`新增 300 · 更新 12 · 未变化 40 · 失败 4`，失败项可查看原因。
- 测试钩子：`data-testid="folder-job"`，`data-status`（scanning/planning/running/done/cancelled/failed），`data-done`、`data-total`。

### D7 文件夹打包下载

- 入口：文件夹行“更多”菜单与顶部栏（当前文件夹）中的“下载为 .zip”。
- 客户端遍历目标子树（分页列出、解密名称），生成条目清单；用 `client-zip` 的 `downloadZip()` 产生 `ReadableStream`，每个条目的内容来自现有逐块解密下载流（解密失败的文件中止整个打包并报错，不输出部分明文——与单文件下载一致）。
- 输出：优先 File System Access API（`showSaveFilePicker`）写入；否则走 Service Worker 流式下载；两者都不可用时，总大小 ≤ 500 MiB 才允许 Blob 回退，否则提示换浏览器。
- ZIP 内路径使用解密后的相对路径，空文件夹写入目录条目；文件修改时间取元数据 `mtime`。条目不压缩（STORE），速度优先、内存恒定。
- 文件名：`{文件夹名}.zip`，根目录为 `加密备份.zip`。进度显示在传输面板（下载任务分组，同 D6）。

### D8 名称

- 本地名称先做与现有上传相同的校验（长度、禁止字符、保留名）；不合法的跳过并列入摘要，不尝试改名。

## Risks / Trade-offs

- **大量文件触发限流导致整体变慢** → D5 自动退避 + 提高生产限额；1000 个小文件约 3–4 分钟完成，可接受。
- **增量判定需要读完同大小文件计算哈希** → 只有大小相同时才计算；在 Worker 中流式计算，不阻塞界面。
- **新增元数据字段** → 可选字段、向后兼容，不改变已有密文格式；审查时重点检查 AAD 与帧格式未改动。
- **zip 打包中途某文件失败** → 整体中止并提示（不生成看似完整但缺文件的 zip）。
- **浏览器差异**：`webkitdirectory` 与 `webkitGetAsEntry` 在 Chrome/Edge/Firefox/Safari 均可用；File System Access API 仅 Chromium，其余走 Service Worker 流。

## Migration Plan

纯前端 + 配置变更。部署新镜像，并在 `/opt/backup/backup.env` 设置 `BACKUP_USER_WRITE_RATE_LIMIT_PER_MINUTE=1200`。旧文件没有 `content_sha256`，按 D3 的回退规则处理，无需迁移。
