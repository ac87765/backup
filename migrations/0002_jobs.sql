-- 0002：后台任务（第 5 组）。
--
-- object_deletions / email_outbox 都是持久化 outbox：业务事务只写队列，
-- 后台 worker 在事务外执行副作用，失败按退避重试。claim_token 用于防止
-- 租约过期后旧 worker 误删/误更新新 worker 已重新领取的行。
-- users 表第 6 组才建立，因此这里的 user_id 暂不加外键。

CREATE TABLE object_deletions (
    id BIGSERIAL PRIMARY KEY,
    object_key TEXT NOT NULL,
    not_before TIMESTAMPTZ NOT NULL DEFAULT now(),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    last_error TEXT,
    claimed_until TIMESTAMPTZ,
    claim_token UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX object_deletions_due_idx ON object_deletions (not_before, id);

CREATE TABLE email_outbox (
    id BIGSERIAL PRIMARY KEY,
    user_id UUID NOT NULL,
    purpose TEXT NOT NULL,
    -- 只允许非敏感参数（例如语言、重发原因）；令牌绝不写入本表。
    params JSONB NOT NULL DEFAULT '{}'::jsonb,
    not_before TIMESTAMPTZ NOT NULL DEFAULT now(),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    last_error TEXT,
    claimed_until TIMESTAMPTZ,
    claim_token UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX email_outbox_due_idx ON email_outbox (not_before, id);

-- 提前建立（原计划 6.1）：发送任务只写入令牌哈希，验证接口在第 6 组实现。
CREATE TABLE email_tokens (
    id BIGSERIAL PRIMARY KEY,
    user_id UUID NOT NULL,
    purpose TEXT NOT NULL,
    token_hash BYTEA NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX email_tokens_user_purpose_idx ON email_tokens (user_id, purpose, created_at DESC);
