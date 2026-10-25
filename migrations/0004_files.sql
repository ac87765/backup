-- 0004：文件管理（第 7 组 tasks 7.1–7.7）。
--
-- 设计约束（design D5/D6/D9）：
-- - nodes / contents / file_versions 的 ID 由客户端生成（UUIDv4），服务端只校验格式；
-- - 同一目录内不重名靠 name_token + 部分唯一索引，服务端看不到名称；
-- - 所有业务表都带 user_id，所有请求路径上的查询都必须带 user_id 条件；
-- - 回收站 = 软删除：整棵子树共享同一 trash_batch_id 与 deleted_at，
--   trashed_from_parent_id 记录恢复时的原父节点。
--
-- 根节点在注册事务中由服务端生成（kind = folder，无包裹密钥/元数据/名称令牌），
-- 本迁移把 users.root_node_id 改为 NOT NULL，并为已有开发数据补建根节点。

CREATE TABLE nodes (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- 软删除的顶层节点可能在原父节点被永久删除后失去 parent_id；恢复时
    -- 使用 trashed_from_parent_id 判断原位置是否仍可用。ON DELETE SET NULL
    -- 让“永久删除父节点”不会被另一个回收站 batch 的悬空引用阻塞。
    -- 复合外键保证父子节点属于同一用户；只置空 parent_id，不动 user_id。
    parent_id UUID,
    kind TEXT NOT NULL CHECK (kind IN ('file', 'folder')),
    -- 包裹的节点密钥：版本字节 + AEAD 帧（与 crypto-core 的 32 字节密钥一致）。
    wrapped_key BYTEA CHECK (
        wrapped_key IS NULL
        OR octet_length(wrapped_key) = 73
    ),
    -- 加密元数据：版本字节 + AEAD 帧；长度上限由 API 请求体上限兜底。
    encrypted_meta BYTEA CHECK (
        encrypted_meta IS NULL OR octet_length(encrypted_meta) >= 42
    ),
    -- HMAC-SHA256 名称令牌。
    name_token BYTEA CHECK (
        name_token IS NULL OR octet_length(name_token) = 32
    ),
    current_version_id UUID,
    row_version INTEGER NOT NULL DEFAULT 1 CHECK (row_version > 0),
    deleted_at TIMESTAMPTZ,
    trash_batch_id UUID,
    trashed_from_parent_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- 根节点：无父节点、未删除、无包裹密钥/元数据/名称令牌；
    -- 其余节点三者必填。回收站顶层项在父节点被永久删除后允许 parent_id 为
    -- NULL（保留 trashed_from_parent_id 供恢复时判断原位置）。
    CONSTRAINT nodes_shape_check CHECK (
        (
            parent_id IS NULL
            AND deleted_at IS NULL
            AND kind = 'folder'
            AND wrapped_key IS NULL
            AND encrypted_meta IS NULL
            AND name_token IS NULL
        )
        OR (
            wrapped_key IS NOT NULL
            AND encrypted_meta IS NOT NULL
            AND name_token IS NOT NULL
            AND (parent_id IS NOT NULL OR deleted_at IS NOT NULL)
        )
    ),
    -- 回收站三列必须同时出现或同时为空。
    CONSTRAINT nodes_trash_consistency_check CHECK (
        (deleted_at IS NULL) = (trash_batch_id IS NULL)
        AND (deleted_at IS NULL) = (trashed_from_parent_id IS NULL)
    ),
    -- 复合唯一键供 contents / file_versions / 当前版本指针的复合外键引用。
    CONSTRAINT nodes_user_id_key UNIQUE (user_id, id),
    CONSTRAINT nodes_parent_fkey FOREIGN KEY (user_id, parent_id)
        REFERENCES nodes (user_id, id)
        ON DELETE SET NULL (parent_id)
        DEFERRABLE INITIALLY DEFERRED
);

-- 每个用户恰好一个未删除的根节点。
CREATE UNIQUE INDEX nodes_one_root_idx
    ON nodes (user_id)
    WHERE parent_id IS NULL AND deleted_at IS NULL;

-- 同一目录（未删除节点）内名称令牌唯一（design D6）。
CREATE UNIQUE INDEX nodes_name_unique_idx
    ON nodes (user_id, parent_id, name_token)
    WHERE deleted_at IS NULL;

-- 列目录：按 id 做基于游标的分页。
CREATE INDEX nodes_children_idx
    ON nodes (user_id, parent_id, id)
    WHERE deleted_at IS NULL;

-- 回收站按 batch 查询/恢复/清理。
CREATE INDEX nodes_trash_batch_idx
    ON nodes (user_id, trash_batch_id)
    WHERE deleted_at IS NOT NULL;
CREATE INDEX nodes_trashed_idx
    ON nodes (user_id, deleted_at)
    WHERE deleted_at IS NOT NULL;

CREATE TABLE contents (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    node_id UUID NOT NULL,
    chunk_count INTEGER NOT NULL CHECK (chunk_count >= 1),
    -- 密文字节数（计配额的单位）。
    ciphertext_size BIGINT NOT NULL CHECK (ciphertext_size >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT contents_user_id_key UNIQUE (user_id, id),
    CONSTRAINT contents_node_fkey FOREIGN KEY (user_id, node_id)
        REFERENCES nodes (user_id, id)
        ON DELETE RESTRICT
);

CREATE INDEX contents_node_idx ON contents (user_id, node_id);
CREATE INDEX contents_user_idx ON contents (user_id);

CREATE TABLE chunks (
    content_id UUID NOT NULL,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    idx INTEGER NOT NULL CHECK (idx >= 0),
    -- 对象键由服务端在预签名时生成并写入；全局唯一，便于孤儿巡检。
    object_key TEXT NOT NULL UNIQUE,
    ciphertext_size BIGINT NOT NULL CHECK (ciphertext_size > 0),
    sha256 BYTEA NOT NULL CHECK (octet_length(sha256) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (content_id, idx),
    CONSTRAINT chunks_content_fkey FOREIGN KEY (user_id, content_id)
        REFERENCES contents (user_id, id)
        ON DELETE RESTRICT
);

CREATE INDEX chunks_user_idx ON chunks (user_id);
CREATE INDEX chunks_content_idx ON chunks (user_id, content_id);

CREATE TABLE file_versions (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    node_id UUID NOT NULL,
    content_id UUID NOT NULL,
    encrypted_meta BYTEA NOT NULL CHECK (octet_length(encrypted_meta) >= 42),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT file_versions_user_id_key UNIQUE (user_id, id),
    CONSTRAINT file_versions_node_fkey FOREIGN KEY (user_id, node_id)
        REFERENCES nodes (user_id, id)
        ON DELETE RESTRICT,
    CONSTRAINT file_versions_content_fkey FOREIGN KEY (user_id, content_id)
        REFERENCES contents (user_id, id)
        ON DELETE RESTRICT
);

CREATE INDEX file_versions_node_idx
    ON file_versions (user_id, node_id, created_at DESC, id DESC);
CREATE INDEX file_versions_content_idx
    ON file_versions (user_id, content_id);

-- 当前版本指针在 file_versions 之后才能加复合外键；删除版本时只置空
-- current_version_id（user_id 保持不变）。DEFERRABLE 允许“先写版本再更新指针”
-- 与注册事务里“先插入 user 再插入根 node”的循环引用在同一事务内完成。
ALTER TABLE nodes
    ADD CONSTRAINT nodes_current_version_fkey
    FOREIGN KEY (user_id, current_version_id) REFERENCES file_versions (user_id, id)
    ON DELETE SET NULL (current_version_id)
    DEFERRABLE INITIALLY DEFERRED;

-- 对已有开发数据补建根节点。若历史数据里 root_node_id 指向不存在的节点，
-- 先清空再补建，避免 NOT NULL/外键失败。
UPDATE users
SET root_node_id = NULL
WHERE root_node_id IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM nodes WHERE nodes.id = users.root_node_id);

WITH created_roots AS (
    INSERT INTO nodes (id, user_id, parent_id, kind, row_version)
    SELECT gen_random_uuid(), users.id, NULL, 'folder', 1
    FROM users
    WHERE users.root_node_id IS NULL
    RETURNING id, user_id
)
UPDATE users
SET root_node_id = created_roots.id,
    updated_at = now()
FROM created_roots
WHERE users.id = created_roots.user_id;

ALTER TABLE users ALTER COLUMN root_node_id SET NOT NULL;

ALTER TABLE users
    ADD CONSTRAINT users_root_node_fkey
    FOREIGN KEY (id, root_node_id) REFERENCES nodes (user_id, id)
    ON DELETE RESTRICT
    DEFERRABLE INITIALLY DEFERRED;
