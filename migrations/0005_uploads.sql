-- 0005：文件传输（第 8 组 tasks 8.1–8.6）。
--
-- 设计约束（design D3/D5/D8/D9/D10）：
-- - 上传会话在创建时预留配额（users.reserved_bytes），提交时转为 used_bytes；
-- - content_id 由客户端生成并作为创建请求的幂等键；版本/节点 ID 同样由客户端生成；
-- - 每块对象键由服务端在 presign 时生成（v1/{user_id}/{object_uuid}），
--   同一块重新 presign 会换新键，旧键保存在 superseded_object_keys 中，
--   提交/中止/过期时统一写入 object_deletions 队列；
-- - 会话状态只能 open → committed / aborted / expired 单向转换。转换语句一律带
--   `WHERE status = 'open'` 条件，因此应用层无法把已关闭会话改回 open；
--   对应测试见 `tests/uploads.rs` 的状态机断言；
-- - 沿用第 7 组的复合外键（带 user_id），保证跨用户引用不可能成立。
--
-- 本迁移只新增表，不修改任何既有表；旧版本代码可以在新 schema 上继续运行。

CREATE TABLE upload_sessions (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- 客户端生成的内容 ID（UUIDv4），创建请求的幂等键。全局唯一。
    content_id UUID NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('new_file', 'new_version')),
    -- 新文件：即将创建的节点 ID（客户端生成）；新版本：已有文件的节点 ID。
    node_id UUID NOT NULL,
    -- 仅新文件：目标父文件夹。父文件夹被删除后由外键置空，提交时返回
    -- target_unavailable（会话保持 open，见 file-transfer spec）。
    parent_id UUID,
    -- 仅新文件：包裹的节点密钥 / 加密节点元数据 / 名称令牌。
    wrapped_key BYTEA CHECK (
        wrapped_key IS NULL OR octet_length(wrapped_key) = 73
    ),
    encrypted_meta BYTEA CHECK (
        encrypted_meta IS NULL OR octet_length(encrypted_meta) >= 42
    ),
    name_token BYTEA CHECK (
        name_token IS NULL OR octet_length(name_token) = 32
    ),
    -- 客户端生成的新版本 ID 与该版本元数据密文。
    version_id UUID NOT NULL,
    version_meta BYTEA NOT NULL CHECK (octet_length(version_meta) >= 42),
    chunk_count INTEGER NOT NULL CHECK (chunk_count >= 1),
    -- 密文字节总数（计配额的单位，等于各块密文大小之和）。
    declared_size BIGINT NOT NULL CHECK (declared_size >= 41),
    -- 创建时预留的字节数；提交/中止/过期时释放。始终等于 declared_size。
    reserved_bytes BIGINT NOT NULL CHECK (reserved_bytes >= 0),
    status TEXT NOT NULL DEFAULT 'open' CHECK (
        status IN ('open', 'committed', 'aborted', 'expired')
    ),
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- 只在 committed 状态写入；用于提交幂等返回同一个结果。
    committed_at TIMESTAMPTZ,
    CONSTRAINT upload_sessions_content_id_key UNIQUE (content_id),
    -- 供 upload_chunks 的复合外键引用。
    CONSTRAINT upload_sessions_user_id_key UNIQUE (user_id, id),
    -- 两种目标形态互斥：新文件必须带节点密钥/元数据/名称令牌；
    -- 新版本不得携带这些字段，也不允许父节点。新文件的 parent_id 可因
    -- 父文件夹被删除而置空（提交时按目标不可用处理）。
    CONSTRAINT upload_sessions_target_shape_check CHECK (
        (
            kind = 'new_file'
            AND wrapped_key IS NOT NULL
            AND encrypted_meta IS NOT NULL
            AND name_token IS NOT NULL
        )
        OR (
            kind = 'new_version'
            AND parent_id IS NULL
            AND wrapped_key IS NULL
            AND encrypted_meta IS NULL
            AND name_token IS NULL
        )
    ),
    CONSTRAINT upload_sessions_committed_at_check CHECK (
        (status = 'committed') = (committed_at IS NOT NULL)
    ),
    CONSTRAINT upload_sessions_reserved_matches_declared_check CHECK (
        reserved_bytes = declared_size
    ),
    -- 复合外键（带 user_id）：父文件夹必须属于同一用户。
    -- 父文件夹被永久删除时只置空 parent_id，保留会话本身（提交时返回
    -- target_unavailable，会话保持 open）。
    CONSTRAINT upload_sessions_parent_fkey FOREIGN KEY (user_id, parent_id)
        REFERENCES nodes (user_id, id)
        ON DELETE SET NULL (parent_id)
);

CREATE INDEX upload_sessions_expiry_idx
    ON upload_sessions (expires_at, id)
    WHERE status = 'open';
CREATE INDEX upload_sessions_user_status_idx
    ON upload_sessions (user_id, status, created_at DESC);
CREATE INDEX upload_sessions_node_idx
    ON upload_sessions (user_id, node_id);

CREATE TABLE upload_chunks (
    session_id UUID NOT NULL,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    idx INTEGER NOT NULL CHECK (idx >= 0),
    -- 当前对象键（服务端生成）。全局唯一，便于孤儿巡检。
    object_key TEXT NOT NULL UNIQUE,
    -- 该块曾经生成过、但已被重新 presign 替换的对象键。提交/中止/过期时
    -- 与当前键一起写入删除队列；已上传的旧对象因此不会泄漏。
    superseded_object_keys TEXT[] NOT NULL DEFAULT '{}',
    -- 声明大小；创建时由会话布局推出，确认时必须等于对象实际大小。
    declared_size BIGINT NOT NULL CHECK (declared_size > 0),
    -- 客户端提供的 SHA-256；只在校验通过后与 confirmed_at 一起写入。
    sha256 BYTEA CHECK (sha256 IS NULL OR octet_length(sha256) = 32),
    confirmed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (session_id, idx),
    -- 复合外键（带 user_id）：块只能属于同一用户的会话。
    CONSTRAINT upload_chunks_session_fkey FOREIGN KEY (user_id, session_id)
        REFERENCES upload_sessions (user_id, id)
        ON DELETE CASCADE,
    CONSTRAINT upload_chunks_confirmation_check CHECK (
        (sha256 IS NULL) = (confirmed_at IS NULL)
    )
);

-- 孤儿巡检按对象键反查“未关闭会话是否仍引用它”（包括被替换的旧键）。
CREATE INDEX upload_chunks_superseded_keys_idx
    ON upload_chunks USING GIN (superseded_object_keys);

-- 扫描型任务的持久化进度游标（目前只有孤儿对象巡检使用）。
-- 每次运行从 cursor 之后继续列举对象，扫到末尾后回到开头，保证大 bucket
-- 也能在多次运行中逐步扫完。
CREATE TABLE job_state (
    job_id INTEGER PRIMARY KEY,
    cursor TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
