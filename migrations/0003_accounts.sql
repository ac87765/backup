-- 0003：用户账号与会话（第 6 组）。
--
-- users.email 使用 citext 保证大小写不敏感的唯一性；写入前服务端仍会做
-- trim + lowercase 规范化，使 prelogin 的伪造盐对同一邮箱具有确定性。
--
-- kdf_params 是 crypto-core 的 KdfParams JSON 契约，必须包含 algorithm 字段：
--   {"algorithm":"argon2id","m_cost_kib":65536,"t_cost":3,"p_cost":1}
--
-- root_node_id 暂时允许为 NULL：nodes 表在第 7 组（7.1）才建立，注册事务里
-- 创建根节点的逻辑也在那里接入。这里先保留列，避免后续再加一次迁移。
--
-- pepper_version 与 recovery_pepper_version 分开记录：登录时只能用 auth_key
-- 把 auth_hash 升级到当前 pepper，而 recovery_auth_hash 要等下一次恢复重置或
-- 轮换恢复密钥（拿到 rk_auth）时才能升级。两者共用一个版本会让旧 pepper
-- 轮换后恢复密钥失效。

CREATE TABLE users (
    id UUID PRIMARY KEY,
    email CITEXT NOT NULL UNIQUE,
    email_verified_at TIMESTAMPTZ,
    kdf_params JSONB NOT NULL,
    kdf_salt BYTEA NOT NULL CHECK (octet_length(kdf_salt) = 16),
    auth_hash BYTEA NOT NULL CHECK (octet_length(auth_hash) = 32),
    pepper_version INTEGER NOT NULL CHECK (pepper_version > 0),
    recovery_auth_hash BYTEA NOT NULL CHECK (octet_length(recovery_auth_hash) = 32),
    recovery_pepper_version INTEGER NOT NULL CHECK (recovery_pepper_version > 0),
    wrapped_mk BYTEA NOT NULL,
    recovery_wrapped_mk BYTEA NOT NULL,
    quota_bytes BIGINT NOT NULL DEFAULT 10737418240 CHECK (quota_bytes >= 0),
    used_bytes BIGINT NOT NULL DEFAULT 0 CHECK (used_bytes >= 0),
    reserved_bytes BIGINT NOT NULL DEFAULT 0 CHECK (reserved_bytes >= 0),
    failed_logins INTEGER NOT NULL DEFAULT 0 CHECK (failed_logins >= 0),
    locked_until TIMESTAMPTZ,
    root_node_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX users_root_node_idx ON users (root_node_id) WHERE root_node_id IS NOT NULL;

CREATE TABLE sessions (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- 只保存 SHA-256(token)，绝不保存令牌明文。
    token_hash BYTEA NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- 绝对过期时间（创建时 now() + 30 天）；空闲 7 天由 last_seen_at 判定。
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    ip TEXT NOT NULL,
    user_agent TEXT
);

CREATE INDEX sessions_user_active_idx
    ON sessions (user_id, last_seen_at DESC)
    WHERE revoked_at IS NULL;
CREATE INDEX sessions_expiry_idx ON sessions (expires_at);

-- email_tokens 在 0002 中先行建立；这里补 users 外键与清理索引。
-- NOT VALID 避免历史孤儿行阻止迁移；新写入仍会强制校验。
ALTER TABLE email_tokens
    ADD CONSTRAINT email_tokens_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE NOT VALID;
CREATE INDEX email_tokens_expiry_idx ON email_tokens (expires_at) WHERE used_at IS NULL;

-- email_outbox 同样补外键；发送任务只保存 user_id / purpose / 非敏感参数。
ALTER TABLE email_outbox
    ADD CONSTRAINT email_outbox_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE NOT VALID;
